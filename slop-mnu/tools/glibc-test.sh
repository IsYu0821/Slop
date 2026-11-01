#!/bin/sh
# glibc dynamic binary smoke test: unmodified host-glibc dynamic hello
# (PT_INTERP /lib64/ld-linux-x86-64.so.2) + libc, no Slop rebuild of apps.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
PROFILE=${PROFILE:-B}
BACKUP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/slop-glibc.XXXXXX")
cp tests/minimal-linux.S "$BACKUP_DIR/minimal-linux.S"
cp tools/build-initramfs.py "$BACKUP_DIR/build-initramfs.py"
cleanup() {
  cp "$BACKUP_DIR/minimal-linux.S" tests/minimal-linux.S
  cp "$BACKUP_DIR/build-initramfs.py" tools/build-initramfs.py
  rm -f tests/musl/glibc-hello
  rm -rf "$BACKUP_DIR"
}
trap cleanup EXIT
gcc -O1 -o build/glibc-hello /tmp/opencode/glibc-hello.c
cp build/glibc-hello tests/musl/glibc-hello
# real host glibc files (unmodified bytes) at their canonical paths
mkdir -p tests/glibc/lib64 tests/glibc/lib-x86_64
cp -L /lib64/ld-linux-x86-64.so.2 tests/glibc/lib64/ld-linux-x86-64.so.2
cp /lib/x86_64-linux-gnu/libc.so.6 tests/glibc/lib-x86_64/libc.so.6
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
p: .asciz "/bin/glibc-hello"
arg0: .asciz "glibc-hello"
.align 8
argv: .quad arg0, 0
envp: .quad 0
# /bin/hello
INNER_EOF
python3 - <<'PY'
from pathlib import Path
p=Path('tools/build-initramfs.py')
s=p.read_text()
needle="('/bin/hello-net',Path('tests/musl/hello-net').read_bytes(),0o100755),"
assert needle in s
s=s.replace(needle,needle+"""('/bin/glibc-hello',Path('tests/musl/glibc-hello').read_bytes(),0o100755),
('/lib64/ld-linux-x86-64.so.2',Path('tests/glibc/lib64/ld-linux-x86-64.so.2').read_bytes(),0o755),
('/lib/x86_64-linux-gnu/libc.so.6',Path('tests/glibc/lib-x86_64/libc.so.6').read_bytes(),0o755),
""",1)
p.write_text(s)
print("injected")
PY
./tools/build-uefi.sh >/tmp/scratchpad/glibc-build.log 2>&1
SERLOG=build/glibc-serial.log
rm -f "$SERLOG"
if SERIAL_LOG="$SERLOG" timeout 300 ./tools/qemu-profile.sh "$PROFILE" >/dev/null 2>&1; then
  guest_status=0
else
  guest_status=$?
fi
grep -a "GLIBC_" "$SERLOG" | head -5 || true
if ! grep -q "GLIBC_HELLO" "$SERLOG"; then
  echo "glibc test failed (qemu status $guest_status)"
  exit 1
fi
if [ "$guest_status" -ne 33 ]; then
  echo "guest QEMU status $guest_status (expected 33)"
  exit 1
fi
echo "glibc test: PASS (profile $PROFILE)"
