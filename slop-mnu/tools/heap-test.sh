#!/bin/sh
# Runs a single musl-built program inside the guest and prints its output.
#
# The program is built with musl-gcc, injected into the initramfs as
# /bin/<name> and started as the guest init, so the kernel path is the same
# one a normal Linux program takes.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
MUSL=${MUSL_GCC:-/tmp/opencode/musl-install/bin/musl-gcc}
test -x "$MUSL" || { echo "musl-gcc not found"; exit 1; }
NAME=${NAME:-heap}
SRC=${SRC:-tests/heap.c}
LOG=${LOG:-build/heap-serial.log}
TIMEOUT=${TIMEOUT:-60}
mkdir -p build
BACKUP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/slop-heap.XXXXXX")
cp tests/minimal-linux.S "$BACKUP_DIR/minimal-linux.S"
cp tools/build-initramfs.py "$BACKUP_DIR/build-initramfs.py"
cleanup() {
  cp "$BACKUP_DIR/minimal-linux.S" tests/minimal-linux.S
  cp "$BACKUP_DIR/build-initramfs.py" tools/build-initramfs.py
  rm -f "tests/musl/$NAME"
  rm -rf "$BACKUP_DIR"
}
trap cleanup EXIT

"$MUSL" -static -O1 -o "build/$NAME" "$SRC"
cp "build/$NAME" "tests/musl/$NAME"
cat > tests/minimal-linux.S <<INNER_EOF
.global _start
.section .text
_start:
    mov \$59,%rax
    lea p(%rip), %rdi
    lea argv(%rip), %rsi
    lea envp(%rip), %rdx
    syscall
1:  jmp 1b
.section .rodata
p: .asciz "/bin/$NAME"
arg0: .asciz "$NAME"
.align 8
argv: .quad arg0, 0
envp: .quad 0
# /bin/hello
INNER_EOF
python3 - <<PY
from pathlib import Path
p=Path('tools/build-initramfs.py')
s=p.read_text()
needle="('/bin/hello-net',Path('tests/musl/hello-net').read_bytes(),0o100755),"
if needle not in s:
    raise SystemExit('could not locate hello-net initramfs entry')
s=s.replace(needle,needle+"('/bin/$NAME',Path('tests/musl/$NAME').read_bytes(),0o100755),",1)
p.write_text(s)
PY
if ! ./tools/build-uefi.sh >build/heap-build.log 2>&1; then
  echo "heap: build failed" >&2
  tail -20 build/heap-build.log >&2
  exit 1
fi
OVMF_CODE=${OVMF_CODE:-/usr/share/OVMF/OVMF_CODE_4M.fd}
OVMF_VARS_TEMPLATE=${OVMF_VARS_TEMPLATE:-/usr/share/OVMF/OVMF_VARS_4M.fd}
cp "$OVMF_VARS_TEMPLATE" build/OVMF_VARS.fd
rm -f "$LOG"
set +e
timeout "$TIMEOUT" qemu-system-x86_64 -machine q35,accel=tcg -cpu max -m 256M -smp 1 \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
  -drive if=pflash,format=raw,file=build/OVMF_VARS.fd \
  -drive format=raw,file=fat:rw:build/efi \
  -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
  -serial file:"$LOG" -display none -no-reboot -no-shutdown >/dev/null 2>&1
status=$?
set -e
grep -v '^trace:' "$LOG" | sed -n '/execve: committed/,$p' | head -40
echo "heap: guest qemu status $status"
if grep -q "heap: allocator stress passed" "$LOG"; then
  echo "heap: PASS"
  exit 0
fi
echo "heap: FAIL" >&2
exit 1
