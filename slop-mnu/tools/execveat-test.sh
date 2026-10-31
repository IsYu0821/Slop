#!/bin/sh
# Minimal fork+execveat+waitpid stress (no block I/O).
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
MUSL=${MUSL_GCC:-/tmp/opencode/musl-install/bin/musl-gcc}
test -x "$MUSL" || { echo "musl-gcc not found"; exit 1; }
PROFILE=${PROFILE:-B}
BACKUP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/slop-execveat.XXXXXX")
cp tests/minimal-linux.S "$BACKUP_DIR/minimal-linux.S"
cp tools/build-initramfs.py "$BACKUP_DIR/build-initramfs.py"
cleanup() {
  cp "$BACKUP_DIR/minimal-linux.S" tests/minimal-linux.S
  cp "$BACKUP_DIR/build-initramfs.py" tools/build-initramfs.py
  rm -f tests/musl/execveat-mini tests/musl/nopie-helper
  rm -rf "$BACKUP_DIR"
}
trap cleanup EXIT
"$MUSL" -static -O1 -o build/execveat-mini tests/execveat-mini.c
"$MUSL" -static -no-pie -O1 -o build/nopie-helper tests/nopie-helper.c
cp build/execveat-mini tests/musl/execveat-mini
cp build/nopie-helper tests/musl/nopie-helper
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
p: .asciz "/bin/execveat-mini"
arg0: .asciz "execveat-mini"
arg1: .asciz "200"
.align 8
argv: .quad arg0, arg1, 0
envp: .quad 0
# /bin/hello
INNER_EOF
python3 - <<'PY'
from pathlib import Path
p=Path('tools/build-initramfs.py')
s=p.read_text()
needle="('/bin/hello-net',Path('tests/musl/hello-net').read_bytes(),0o100755),"
if needle not in s:
    raise SystemExit('could not locate hello-net initramfs entry')
s=s.replace(needle,needle+"('/bin/execveat-mini',Path('tests/musl/execveat-mini').read_bytes(),0o100755),('/bin/nopie-helper',Path('tests/musl/nopie-helper').read_bytes(),0o100755),",1)
p.write_text(s)
PY
./tools/build-uefi.sh >/tmp/scratchpad/execveat-build.log 2>&1
SERLOG=build/execveat-serial.log
rm -f "$SERLOG"
if SERIAL_LOG="$SERLOG" timeout 300 ./tools/qemu-profile.sh "$PROFILE" >/dev/null 2>&1; then
  guest_status=0
else
  guest_status=$?
fi
grep -a "EXECVEAT" "$SERLOG" | head -5 || true
if ! grep -q "EXECVEAT_OK" "$SERLOG"; then
  echo "execveat probe failed (qemu status $guest_status, expected 33)"
  exit 1
fi
if [ "$guest_status" -ne 33 ]; then
  echo "guest QEMU status $guest_status (expected 33)"
  exit 1
fi
echo "execveat test: PASS (profile $PROFILE)"
