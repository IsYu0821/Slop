#!/bin/sh
# Read-only ext4 guest test: builds an ext4 image with a seed tree, runs
# the differential battery natively (against a seed copy) and in the guest
# (against the ext4 mount), then proves the image is untouched.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
MUSL=${MUSL_GCC:-/tmp/opencode/musl-install/bin/musl-gcc}
test -x "$MUSL" || { echo "musl-gcc not found"; exit 1; }
BACKUP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/slop-ext4.XXXXXX")
cp tests/minimal-linux.S "$BACKUP_DIR/minimal-linux.S"
cp tools/build-initramfs.py "$BACKUP_DIR/build-initramfs.py"
cleanup() {
  cp "$BACKUP_DIR/minimal-linux.S" tests/minimal-linux.S
  cp "$BACKUP_DIR/build-initramfs.py" tools/build-initramfs.py
  rm -f tests/musl/differential
  rm -f tests/musl/nopie-helper
  rm -rf "$BACKUP_DIR"
}
trap cleanup EXIT

phase() { printf '[%s] %s\n' "$(date +%H:%M:%S)" "$1"; }
fail_diag() {
  echo "--- failure diagnostics ---"
  echo "qemu status: ${guest_status:-n/a} (expected 33)"
  echo "guest markers reached:"
  for m in DIFF_OK DIFF_EXT4_OK DIFF_EXT2_SKIP DIFF_DONE; do
    if grep -qF "$m" build/ext4-serial.log 2>/dev/null; then echo "  present: $m"; else echo "  MISSING from here on: $m"; break; fi
  done
  echo "--- serial tail ---"
  tail -n 20 build/ext4-serial.log 2>/dev/null || echo "(no serial log)"
  echo "--- end diagnostics ---"
}

mkdir -p /tmp/scratchpad
phase "native build"
"$MUSL" -static -O1 -o build/differential-host tests/differential.c
"$MUSL" -O1 -o build/differential tests/differential.c
"$MUSL" -static -no-pie -O1 -o build/nopie-helper tests/nopie-helper.c
./tools/build-ext4.sh >/tmp/scratchpad/ext4-build.log 2>&1
rm -rf /tmp/xdiag4
cp -r build/ext4-seed/xdiag4 /tmp/xdiag4
if ! ./build/differential-host > build/ext4-native.out; then
  echo "native ext4 test failed"
  tail -n 10 build/ext4-native.out 2>/dev/null || echo "(no native log)"
  exit 1
fi
phase "native markers"
for marker in DIFF_EXT4_OK DIFF_EXT2_OK DIFF_DONE; do
  if ! grep -F "$marker" build/ext4-native.out >/dev/null; then
    echo "native marker missing: $marker"
    exit 1
  fi
done

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
cp build/nopie-helper tests/musl/nopie-helper
phase "uefi image"
python3 - <<'PY'
from pathlib import Path
p=Path('tools/build-initramfs.py')
s=p.read_text()
needle="('/bin/hello-net',Path('tests/musl/hello-net').read_bytes(),0o100755),"
if needle not in s:
    raise SystemExit('could not locate hello-net initramfs entry')
s=s.replace(needle,needle+"('/bin/differential',Path('tests/musl/differential').read_bytes(),0o100755),('/bin/nopie-helper',Path('tests/musl/nopie-helper').read_bytes(),0o100755),",1)
p.write_text(s)
PY
./tools/build-uefi.sh >/tmp/scratchpad/ext4-uefi-build.log 2>&1
OVMF_CODE=${OVMF_CODE:-/usr/share/OVMF/OVMF_CODE_4M.fd}
OVMF_VARS_TEMPLATE=${OVMF_VARS_TEMPLATE:-/usr/share/OVMF/OVMF_VARS_4M.fd}
cp "$OVMF_VARS_TEMPLATE" build/OVMF_VARS.fd
rm -f build/ext4-serial.log
phase "guest run (240s budget)"
if timeout 240 qemu-system-x86_64 -machine q35,accel=tcg -cpu max -m 256M -smp 1 \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
  -drive if=pflash,format=raw,file=build/OVMF_VARS.fd \
  -drive format=raw,file=fat:rw:build/efi \
  -drive file=build/ext4.img,format=raw,if=none,id=hd0 \
  -device virtio-blk-pci,drive=hd0 \
  -device virtio-net-pci,netdev=net0 -netdev user,id=net0 \
  -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
  -serial file:build/ext4-serial.log -display none -no-reboot -no-shutdown >/dev/null 2>&1; then
  guest_status=0
else
  guest_status=$?
fi
if [ "$guest_status" -ne 33 ]; then
  echo "guest QEMU status $guest_status (expected 33)"
  fail_diag
  exit 1
fi
phase "guest markers"
for marker in DIFF_EXT4_OK DIFF_EXT2_SKIP DIFF_DONE; do
  if ! grep -F "$marker" build/ext4-serial.log >/dev/null; then
    echo "guest marker missing: $marker"
    fail_diag
    exit 1
  fi
done
phase "host-side ext4 verification"
# The guest mount is read-only: e2fsck must be clean and the free counts
# must be byte-identical to the pristine image built above.
if ! e2fsck -n -f build/ext4.img >/tmp/scratchpad/ext4-fsck.log 2>&1; then
  echo "ext4 image failed consistency check after guest run"
  cat /tmp/scratchpad/ext4-fsck.log >&2
  exit 1
fi
echo "ext4: e2fsck clean"
pristine_blocks=$(awk '/^Free blocks/{print $3}' build/ext4-pristine-counts.txt)
pristine_inodes=$(awk '/^Free inodes/{print $3}' build/ext4-pristine-counts.txt)
now_blocks=$(dumpe2fs -h build/ext4.img 2>/dev/null | awk '/^Free blocks/{print $3}')
now_inodes=$(dumpe2fs -h build/ext4.img 2>/dev/null | awk '/^Free inodes/{print $3}')
if [ "$now_blocks" != "$pristine_blocks" ] || [ "$now_inodes" != "$pristine_inodes" ]; then
  echo "ext4 free-count mismatch: blocks $now_blocks (want $pristine_blocks), inodes $now_inodes (want $pristine_inodes)" >&2
  exit 1
fi
echo "ext4: free counts unchanged (blocks=$now_blocks inodes=$now_inodes)"
echo "ext4 test: PASS"
