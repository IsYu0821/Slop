#!/bin/sh
# Python 3.12 official test-suite harness for Slop (Track 1).
#
# Runs selected CPython stdlib regression tests (unmodified, from
# libpython3.12-testsuite + host stdlib) inside the Slop guest and records
# structured results. Also runs the same tests natively on Linux for
# comparison so Slop-specific deltas can be isolated.
#
# Usage:
#   ./tools/python-official-test.sh [options] [test_bool test_int ...]
# Options:
#   TAG=<tag>        result tag (default: pyoff-<fsmode>)
#   FSMODE=ext4|ext2 guest fs (default ext4)
#   PROFILE=A|B      qemu profile (default A)
#   HOSTONLY=1       only run native-Linux comparison, skip guest
#   GUESTONLY=1      only run guest, skip native comparison
#   TIMEOUT=<secs>   per-test guest timeout (default 120)
#
# Outputs:
#   build/pyofficial-<tag>.log      full serial log (guest)
#   build/pyofficial-<tag>.results  parsed per-test results + provenance
#   build/pyofficial-<tag>.host     native-Linux comparison results
#
# Serial execution: this script must not run concurrently with other
# python/glibc/initramfs/disk-image tests (shared artifacts).
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
export CARGO_TARGET_DIR=${SCRATCH:-/tmp/slopwork}/slop-target
FSMODE=${FSMODE:-ext4}
PROFILE=${PROFILE:-A}
TAG=${TAG:-pyoff-$FSMODE}
HOSTONLY=${HOSTONLY:-0}
GUESTONLY=${GUESTONLY:-0}
TIMEOUT=${TIMEOUT:-120}
SEEDMODE=${SEEDMODE:-disk}
MUSL=${MUSL_GCC:-/usr/bin/musl-gcc}

if [ "$#" -gt 0 ]; then
  # shellcheck disable=SC2124
  TESTS="$@"
else
  TESTS="test_bool test_int test_float test_str test_bytes test_list test_dict test_set test_tuple"
fi
# shellcheck disable=SC2086
set -- $TESTS
echo "official tests: $* (fsmode=$FSMODE profile=$PROFILE tag=$TAG)"

SERLOG="build/pyofficial-$TAG.log"
RESULTS="build/pyofficial-$TAG.results"
HOSTLOG="build/pyofficial-$TAG.host"

# ---- provenance (host side) ----
PYBIN=/usr/bin/python3.12
{
  echo "# provenance (host-recorded)"
  echo "python_version=$($PYBIN --version 2>&1)"
  echo -n "python_sha256="; sha256sum $PYBIN | awk '{print $1}'
  echo "glibc=$(ldd --version 2>&1 | head -1)"
  echo "dynamic_deps=$(ldd $PYBIN 2>/dev/null | tr '\n' ';')"
  echo "stdlib_src=/usr/lib/python3.12 (libpython3.12-stdlib 3.12.3-1ubuntu0.17)"
  echo "testsuite_src=${SCRATCH:-/tmp/slopwork}/py-testsuite (libpython3.12-testsuite 3.12.3-1ubuntu0.17)"
  echo "slop_changes=none (unmodified host binary, stdlib, extensions, tests)"
  echo "guest_cmd=/bin/python3 -u -S -m test.<name> (per-test subprocess)"
  echo "guest_timeout=$TIMEOUT"
  echo "git_head=$(git rev-parse HEAD)"
} | tee "$RESULTS"
# extension module provenance
{
  echo "# extension modules (lib-dynload)"
  (cd /usr/lib/python3.12/lib-dynload && sha256sum -- *.so 2>/dev/null | head -60) || true
} >> "$RESULTS"

if [ "$HOSTONLY" -eq 0 ]; then
"$MUSL" -static -O1 -o build/pyrun tests/pyrun.c
cp build/pyrun tests/musl/pyrun
mkdir -p tests/glibc/lib64 tests/glibc/lib-x86_64
cp -L $PYBIN tests/glibc/python3.12
cp -L /lib64/ld-linux-x86-64.so.2 tests/glibc/lib64/
cp /lib/x86_64-linux-gnu/libc.so.6 tests/glibc/lib-x86_64/
cp -L /lib/x86_64-linux-gnu/libm.so.6 tests/glibc/ 2>/dev/null || true
cp -L /lib/x86_64-linux-gnu/libz.so.1 tests/glibc/ 2>/dev/null || true
cp -L /lib/x86_64-linux-gnu/libexpat.so.1 tests/glibc/ 2>/dev/null || true
BACKUP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/slop-pyoff.XXXXXX")
cp tests/minimal-linux.S "$BACKUP_DIR/minimal-linux.S"
cp tools/build-initramfs.py "$BACKUP_DIR/build-initramfs.py"
cp build/ext2.img "$BACKUP_DIR/ext2.img"
cleanup() {
  cp "$BACKUP_DIR/minimal-linux.S" tests/minimal-linux.S
  cp "$BACKUP_DIR/build-initramfs.py" tools/build-initramfs.py
  cp "$BACKUP_DIR/ext2.img" build/ext2.img
  rm -rf "$BACKUP_DIR"
}
trap cleanup EXIT
SEED=${SCRATCH:-/tmp/slopwork}/pyoff-seed
IMG=${SCRATCH:-/tmp/slopwork}/pyoff.img
rm -rf "$SEED"
mkdir -p "$SEED/lib"
# 1. host stdlib (minus stale pycache)
(cd /usr/lib/python3.12 && find . -type f ! -path "*__pycache__*") | while read -r f; do
  mkdir -p "$SEED/lib/$(dirname "$f")"
  cp "/usr/lib/python3.12/$f" "$SEED/lib/$f"
done
# 2. overlay official test files from testsuite package
(cd ${SCRATCH:-/tmp/slopwork}/py-testsuite/usr/lib/python3.12/test && find . -type f ! -path "*__pycache__*") | while read -r f; do
  mkdir -p "$SEED/lib/test/$(dirname "$f")"
  cp "${SCRATCH:-/tmp/slopwork}/py-testsuite/usr/lib/python3.12/test/$f" "$SEED/lib/test/$f"
done
# 3. driver + runlist + per-test runner + seed mode
cp tests/pyofficial-driver.py "$SEED/test.py"
cp tests/pyofficial-runone.py "$SEED/runone.py"
printf '%s\n' "$SEEDMODE" > "$SEED/seedmode.txt"
printf '%s\n' "$TIMEOUT" > "$SEED/timeout.txt"
printf '%s\n' "$@" > "$SEED/runtests.txt"
echo "seed files: $(find "$SEED" -type f | wc -l)"
rm -f "$IMG"
if [ "$FSMODE" = "ext2" ]; then
 mke2fs -t ext2 -b 4096 -N 16384 -d "$SEED" -q "$IMG" 131072
else
 mke2fs -t ext4 -b 4096 -N 16384 -O extent,^64bit,^metadata_csum,^dir_index,^huge_file,^flex_bg,^uninit_bg -d "$SEED" -q "$IMG" 131072
fi
cp "$IMG" build/ext2.img
cat > tests/minimal-linux.S <<'INNER_EOF'
.global _start
.section .text
_start:
    mov $59, %rax
    lea p(%rip), %rdi
    lea argv(%rip), %rsi
    lea envp(%rip), %rdx
    syscall
1:  jmp 1b
# mov $59,%rax
.section .rodata
p: .asciz "/bin/pyrun"
arg0: .asciz "pyrun"
.align 8
argv: .quad arg0, 0
envp: .quad 0
# /bin/hello
INNER_EOF
PYOFF_TIMEOUT=$TIMEOUT python3 - <<'PYEOF'
import os
from pathlib import Path
p=Path('tools/build-initramfs.py')
s=p.read_text()
needle="('/bin/hello-net',Path('tests/musl/hello-net').read_bytes(),0o100755),"
assert needle in s
entries={
 "'/bin/pyrun'": "('/bin/pyrun',Path('tests/musl/pyrun').read_bytes(),0o100755),",
 "'/bin/python3'": "('/bin/python3',Path('tests/glibc/python3.12').read_bytes(),0o100755),",
 "'/lib64/ld-linux-x86-64.so.2'": "('/lib64/ld-linux-x86-64.so.2',Path('tests/glibc/lib64/ld-linux-x86-64.so.2').read_bytes(),0o755),",
 "'/lib/x86_64-linux-gnu/libc.so.6'": "('/lib/x86_64-linux-gnu/libc.so.6',Path('tests/glibc/lib-x86_64/libc.so.6').read_bytes(),0o755),",
 "'/lib/x86_64-linux-gnu/libm.so.6'": "('/lib/x86_64-linux-gnu/libm.so.6',Path('tests/glibc/libm.so.6').read_bytes(),0o755),",
 "'/lib/x86_64-linux-gnu/libz.so.1'": "('/lib/x86_64-linux-gnu/libz.so.1',Path('tests/glibc/libz.so.1').read_bytes(),0o755),",
 "'/lib/x86_64-linux-gnu/libexpat.so.1'": "('/lib/x86_64-linux-gnu/libexpat.so.1',Path('tests/glibc/libexpat.so.1').read_bytes(),0o755),",
}
add="".join(v for k,v in entries.items() if k not in s)
s=s.replace(needle,needle+add,1)
p.write_text(s)
print("injected entries:", sum(1 for k in entries if k in s))
PYEOF
./tools/build-uefi.sh >/dev/null 2>&1
rm -f "$SERLOG"
set +e
# overall budget: per-test timeout * count + boot margin
NTESTS=$#
BUDGET=$(( NTESTS * TIMEOUT + 420 ))
SERIAL_LOG="$SERLOG" timeout "$BUDGET" ./tools/qemu-profile.sh "$PROFILE" >/dev/null 2>&1
LOG="$SERLOG"
echo "qemu exit status $?"
echo "guest summary:"
grep -a "^PYOFF_RESULT" "$LOG" || echo "(no results; guest may have failed to boot)"
STATUS=0
grep -aq "PYOFF_DONE" "$LOG" || STATUS=1
{
  echo "# guest results (parsed from serial log)"
  grep -a "^PYOFF_RESULT" "$LOG" || echo "# none"
} >> "$RESULTS"
echo "results appended to $RESULTS"
exit $STATUS
fi
if [ "$GUESTONLY" -eq 0 ]; then
echo "native Linux comparison:"
: > "$HOSTLOG"
# shellcheck disable=SC2086
for t in $TESTS; do
  start=$(date +%s)
  set +e
  /usr/bin/python3.12 -u -S -m test."$t" > ${SCRATCH:-/tmp/slopwork}/pyoff-host-$t.out 2>&1
  rc=$?
  set -e
  secs=$(( $(date +%s) - start ))
  tailout=$(tail -c 1500 ${SCRATCH:-/tmp/slopwork}/pyoff-host-$t.out | tr '\n' '|' | tail -c 400)
  if [ "$rc" -eq 0 ]; then hstatus=PASS; else hstatus=FAIL; fi
  echo "HOST_RESULT $t $hstatus $secs rc=$rc :: $tailout" | tee -a "$HOSTLOG"
done
echo "host comparison in $HOSTLOG"
fi
echo "done: tag=$TAG"
...[truncated 3070 chars]