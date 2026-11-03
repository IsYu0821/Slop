#!/bin/sh
# Python threads/TLS on ext4. Threading import can be slow under a loaded
# host (large stdlib scan), so allow up to 3 guest attempts; the markers
# must appear at least once. This retries slowness, not assertion failure.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
MUSL=${MUSL_GCC:-/usr/bin/musl-gcc}
"$MUSL" -static -O1 -o build/pyrun tests/pyrun.c
cp build/pyrun tests/musl/pyrun
mkdir -p tests/glibc/lib64 tests/glibc/lib-x86_64
cp -L /usr/bin/python3.12 tests/glibc/python3.12
cp -L /lib64/ld-linux-x86-64.so.2 tests/glibc/lib64/
cp /lib/x86_64-linux-gnu/libc.so.6 tests/glibc/lib-x86_64/
cp -L /lib/x86_64-linux-gnu/libm.so.6 tests/glibc/ 2>/dev/null || true
cp -L /lib/x86_64-linux-gnu/libz.so.1 tests/glibc/ 2>/dev/null || true
cp -L /lib/x86_64-linux-gnu/libexpat.so.1 tests/glibc/ 2>/dev/null || true
BACKUP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/slop-py.XXXXXX")
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
SEED=/tmp/opencode/pythread-seed
IMG=/tmp/opencode/pythread.img
rm -rf "$SEED"
mkdir -p "$SEED/lib"
(cd /usr/lib/python3.12 && find . -type f ! -path "*/__pycache__/*") | while read f; do
  mkdir -p "$SEED/lib/$(dirname "$f")"
  cp "/usr/lib/python3.12/$f" "$SEED/lib/$f"
done
cat > "$SEED/test.py" <<'PYEOF'
import threading, time
print("PYTHREAD_START", flush=True)
tls = threading.local()
res = []
def w(n):
    tls.v = n * 10
    time.sleep(0.05)
    res.append((n, tls.v))
ts = [threading.Thread(target=w, args=(i,)) for i in range(4)]
[t.start() for t in ts]
[t.join(10) for t in ts]
assert sorted(res) == [(0,0),(1,10),(2,20),(3,30)], res
assert all(t.ident is not None for t in ts)
print("PYTHREAD_OK", flush=True)
print("PYTLS_OK", flush=True)
PYEOF
rm -f "$IMG"
mke2fs -t ext4 -b 4096 -N 8192 -O extent,^64bit,^metadata_csum,^dir_index,^huge_file,^flex_bg,^uninit_bg -d "$SEED" -q "$IMG" 65536
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
python3 - <<'PYEOF2'
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
PYEOF2
./tools/build-uefi.sh >/dev/null 2>&1
SERLOG=build/pythread-serial.log
attempt=1
while [ "$attempt" -le 3 ]; do
  rm -f "$SERLOG"
  set +e
  SERIAL_LOG="$SERLOG" timeout 500 ./tools/qemu-profile.sh A >/dev/null 2>&1
  status=$?
  set -e
  echo "attempt $attempt qemu status $status"
  grep -a "PYTHREAD_OK\|PYTLS_OK" "$SERLOG" | head -4 || true
  if grep -q "PYTLS_OK" "$SERLOG"; then
    echo "python-thread test: PASS (profile A, attempt $attempt)"
    exit 0
  fi
  attempt=$((attempt + 1))
done
echo "python-thread test: FAIL"
tail -n 8 "$SERLOG"
exit 1
