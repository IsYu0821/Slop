#!/bin/sh
# File-backed execve gate: boot the guest against a test disk (ext2 or
# ext4), mount it in the guest, and execve static, static-pie and musl
# PIE binaries straight off the disk. The guest loads every segment by
# streaming through the filesystem (no memory-resident image), which is
# what the streaming ELF loader must get byte-exact: a single mis-mapped
# page relocates the whole image into the weeds.
#
# Success is asserted from the serial log: the mount result, the
# static/static-pie/dyn child outputs (matching their native Linux
# behavior) and the probe's own completion marker. The guest always
# exits 33 via isa-debug-exit on success.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
FS=${FS:-both}
MUSL=${MUSL_GCC:-/tmp/opencode/musl-install/bin/musl-gcc}
test -x "$MUSL" || MUSL=/usr/bin/musl-gcc
BACKUP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/slop-fileexec.XXXXXX")
cp tests/minimal-linux.S "$BACKUP_DIR/minimal-linux.S"
cp tools/build-initramfs.py "$BACKUP_DIR/build-initramfs.py"
cp build/ext2.img "$BACKUP_DIR/ext2.img"
cleanup() {
  cp "$BACKUP_DIR/minimal-linux.S" tests/minimal-linux.S
  cp "$BACKUP_DIR/build-initramfs.py" tools/build-initramfs.py
  cp "$BACKUP_DIR/ext2.img" build/ext2.img
  rm -f tests/musl/fileexec-probe tests/musl/fileexec-static tests/musl/fileexec-staticpie
  rm -rf "$BACKUP_DIR" "$SEED"
}
trap cleanup EXIT
SEED=$(mktemp -d "${TMPDIR:-/tmp}/slop-fileexec-seed.XXXXXX")
phase() { printf '[%s] %s\n' "$(date +%H:%M:%S)" "$1"; }
fail() { echo "fileexec test: FAIL ($1)" >&2; exit 1; }

"$MUSL" -static -O1 -o "$SEED/static" tests/fileexec-hello.c
"$MUSL" -static-pie -O1 -o "$SEED/staticpie" tests/fileexec-hello.c
cp tests/musl/hello-dyn "$SEED/dyn"
cp tests/musl/fileexec-probe "$SEED" 2>/dev/null || "$MUSL" -static -O1 -o "$SEED/fileexec-probe" tests/fileexec-probe.c
"$MUSL" -static -O1 -o tests/musl/fileexec-probe tests/fileexec-probe.c

phase "build ext2/ext4 images"
mke2fs -t ext2 -b 1024 -N 256 -O ^dir_index,^uninit_bg,^extent,^huge_file,^flex_bg,^metadata_csum -q -d "$SEED" build/fileexec-ext2.img 8192
mke2fs -t ext4 -b 4096 -O extent,^64bit,^metadata_csum,^dir_index,^huge_file,^flex_bg,^uninit_bg -q -d "$SEED" build/fileexec-ext4.img 16384

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
p: .asciz "/bin/fileexec-probe"
arg0: .asciz "fileexec-probe"
arg1: .asciz "static"
arg2: .asciz "staticpie"
arg3: .asciz "dyn"
.align 8
argv: .quad arg0, arg1, arg2, arg3, 0
envp: .quad 0
# /bin/hello
INNER_EOF
python3 - <<'PY'
from pathlib import Path
p = Path('tools/build-initramfs.py')
s = p.read_text()
needle = "('/bin/hello-net',Path('tests/musl/hello-net').read_bytes(),0o100755),"
if needle not in s:
    raise SystemExit('could not locate hello-net initramfs entry')
add = "('/bin/fileexec-probe',Path('tests/musl/fileexec-probe').read_bytes(),0o100755),"
s = s.replace(needle, needle + add, 1)
p.write_text(s)
PY

run_fs() {
  local fs="$1"
  phase "guest run (fs=$fs)"
  cp "build/fileexec-$fs.img" build/ext2.img
  ./tools/build-uefi.sh >/tmp/scratchpad/fileexec-build.log 2>&1 || fail "uefi build ($fs)"
  local serlog="build/fileexec-$fs.log"
  rm -f "$serlog"
  set +e
  SERIAL_LOG="$serlog" timeout 240 ./tools/qemu-profile.sh A >/dev/null 2>&1
  local status=$?
  set -e
  if [ "$status" -ne 33 ]; then
    echo "--- guest serial tail ($serlog) ---" >&2
    tail -n 30 "$serlog" >&2 || true
    fail "qemu exit $status (expected 33) on fs=$fs"
  fi
  grep -a -q "FILEEXEC_MOUNT fs=$fs" "$serlog" || { tail -n 30 "$serlog" >&2; fail "fs=$fs did not mount (or wrong fs)"; }
  grep -a -q "fileexec-hello: hello from disk" "$serlog" || { tail -n 30 "$serlog" >&2; fail "static/static-pie target did not run on fs=$fs"; }
  grep -a -q "dyn: dynamic linking works" "$serlog" || { tail -n 30 "$serlog" >&2; fail "musl PIE dyn target did not run on fs=$fs"; }
  grep -a -q "FILEEXEC_DONE failures=0" "$serlog" || { tail -n 30 "$serlog" >&2; fail "probe reported failures on fs=$fs"; }
  phase "fs=$fs PASS"
}
if [ "$FS" = "both" ]; then
  run_fs ext2
  run_fs ext4
else
  run_fs "$FS"
fi
echo "fileexec test: PASS (fs=$FS)"
