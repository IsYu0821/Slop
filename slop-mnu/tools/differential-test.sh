#!/bin/sh
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
MUSL=${MUSL_GCC:-/tmp/opencode/musl-install/bin/musl-gcc}
test -x "$MUSL" || { echo "musl-gcc not found"; exit 1; }
BACKUP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/slop-differential.XXXXXX")
cp tests/minimal-linux.S "$BACKUP_DIR/minimal-linux.S"
cp tools/build-initramfs.py "$BACKUP_DIR/build-initramfs.py"
cleanup() {
  cp "$BACKUP_DIR/minimal-linux.S" tests/minimal-linux.S
  cp "$BACKUP_DIR/build-initramfs.py" tools/build-initramfs.py
  rm -f tests/musl/differential
  rm -rf "$BACKUP_DIR"
}
trap cleanup EXIT

"$MUSL" -static -O1 -o build/differential-host tests/differential.c
"$MUSL" -O1 -o build/differential tests/differential.c

if ! ./build/differential-host > build/differential-native.out; then
  echo "native differential test failed"
  exit 1
fi
for marker in DIFF_OK DIFF_ERRNO_OK DIFF_PROC_OK DIFF_CAP_OK DIFF_SECCOMP_OK DIFF_INOTIFY_OK DIFF_PTHREAD_OK DIFF_COND_OK DIFF_KILL_OK DIFF_TLS_OK DIFF_DONE; do
  if ! grep -F "$marker" build/differential-native.out >/dev/null; then
    echo "native marker missing: $marker"
    exit 1
  fi
done
if ! grep -F DIFF_IOURING_OK build/differential-native.out >/dev/null && ! grep -F DIFF_IOURING_SKIP build/differential-native.out >/dev/null; then
  echo "native io_uring result marker missing"
  exit 1
fi

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
p: .asciz "/bin/differential"
arg0: .asciz "differential"
env0: .asciz "SLOP_MNU_DIFF=1"
.align 8
argv: .quad arg0, 0
envp: .quad env0, 0
# /bin/hello
INNER_EOF
cp build/differential tests/musl/differential
python3 - <<'PY'
from pathlib import Path
p=Path('tools/build-initramfs.py')
s=p.read_text()
needle="('/bin/hello-net',Path('tests/musl/hello-net').read_bytes(),0o100755),"
if needle not in s:
    raise SystemExit('could not locate hello-net initramfs entry')
s=s.replace(needle,needle+"('/bin/differential',Path('tests/musl/differential').read_bytes(),0o100755),",1)
p.write_text(s)
PY
./tools/build-uefi.sh >/tmp/scratchpad/diff-build.log 2>&1
OVMF_CODE=${OVMF_CODE:-/usr/share/OVMF/OVMF_CODE_4M.fd}
OVMF_VARS_TEMPLATE=${OVMF_VARS_TEMPLATE:-/usr/share/OVMF/OVMF_VARS_4M.fd}
cp "$OVMF_VARS_TEMPLATE" build/OVMF_VARS.fd
rm -f build/differential-serial.log
if timeout 40 qemu-system-x86_64 -machine q35,accel=tcg -cpu max -m 256M -smp 1 \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
  -drive if=pflash,format=raw,file=build/OVMF_VARS.fd \
  -drive format=raw,file=fat:rw:build/efi \
  -device virtio-net-pci,netdev=net0,disable-modern=on -netdev user,id=net0 \
  -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
  -serial file:build/differential-serial.log -display none -no-reboot -no-shutdown >/dev/null 2>&1; then
  guest_status=0
else
  guest_status=$?
fi
if [ "$guest_status" -ne 33 ]; then
  echo "guest QEMU status $guest_status (expected 33)"
  exit 1
fi

for marker in DIFF_OK DIFF_ERRNO_OK DIFF_PROC_OK DIFF_CAP_OK DIFF_SECCOMP_OK DIFF_INOTIFY_OK DIFF_IOURING_OK DIFF_PTHREAD_OK DIFF_COND_OK DIFF_KILL_OK DIFF_TLS_OK DIFF_DONE; do
  if ! grep -F "$marker" build/differential-serial.log >/dev/null; then
    echo "guest marker missing: $marker"
    exit 1
  fi
done
echo "differential test: PASS"
