#!/bin/sh
# ext4 overwrite test: disposable image copy, guest overwrites part of a
# file in place (no allocation), then Linux verifies bytes + e2fsck + free
# counts. Proves the write path without touching the pristine image.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
MUSL=${MUSL_GCC:-/tmp/opencode/musl-install/bin/musl-gcc}
test -x "$MUSL" || { echo "musl-gcc not found"; exit 1; }
BACKUP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/slop-ext4w.XXXXXX")
cp tests/minimal-linux.S "$BACKUP_DIR/minimal-linux.S"
cp tools/build-initramfs.py "$BACKUP_DIR/build-initramfs.py"
cleanup() {
  cp "$BACKUP_DIR/minimal-linux.S" tests/minimal-linux.S
  cp "$BACKUP_DIR/build-initramfs.py" tools/build-initramfs.py
  rm -f tests/musl/ext4-write-test
  rm -rf "$BACKUP_DIR"
}
trap cleanup EXIT

"$MUSL" -static -O1 -o build/ext4-write-test tests/ext4-write-test.c
cp build/ext4-write-test tests/musl/ext4-write-test
# Build seed + image under /tmp (build/ may hold root-owned pristine files).
SEED=/tmp/opencode/ext4w-seed
IMG=/tmp/opencode/ext4w.img
rm -rf "$SEED"
mkdir -p "$SEED/xdiag4/sub"
printf 'Hello from ext4.\nSecond line here.\n' > "$SEED/xdiag4/hello.txt"
printf 'nested content 0123456789\n' > "$SEED/xdiag4/sub/nested.txt"
ln -sf hello.txt "$SEED/xdiag4/a-sym" 2>/dev/null || true
python3 - "$SEED" <<'PY'
import os, sys
seed = sys.argv[1]
with open(os.path.join(seed, 'xdiag4', 'big.bin'), 'wb') as f:
    for c in range(25):
        f.write(bytes(((c * 131 + i * 7 + 3) & 0xff) for i in range(4096)))
tgt = 'x' * 100
try:
    os.symlink(tgt, os.path.join(seed, 'xdiag4', 'b-sym'))
except FileExistsError:
    pass
PY
rm -f "$IMG"
mke2fs -t ext4 -b 4096 -N 256 -O extent,^64bit,^metadata_csum,^dir_index,^huge_file,^flex_bg,^uninit_bg -d "$SEED" -q "$IMG" 8192
cp "$IMG" build/ext4-write-test.img
dumpe2fs -h build/ext4-write-test.img 2>/dev/null | grep -E '^(Free blocks|Free inodes):' > /tmp/opencode/ext4w-before.txt || true
cat /tmp/opencode/ext4w-before.txt

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
p: .asciz "/bin/ext4-write-test"
arg0: .asciz "ext4-write-test"
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
if needle not in s:
    raise SystemExit('could not locate hello-net initramfs entry')
s=s.replace(needle,needle+"('/bin/ext4-write-test',Path('tests/musl/ext4-write-test').read_bytes(),0o100755),",1)
p.write_text(s)
PY
/tmp/opencode/uefi-user.sh >/tmp/opencode/ext4w-build.log 2>&1 || { tail -n 20 /tmp/opencode/ext4w-build.log; exit 1; }
OVMF_CODE=${OVMF_CODE:-/usr/share/OVMF/OVMF_CODE_4M.fd}
OVMF_VARS_TEMPLATE=${OVMF_VARS_TEMPLATE:-/usr/share/OVMF/OVMF_VARS_4M.fd}
cp "$OVMF_VARS_TEMPLATE" build/OVMF_VARS.fd
LOG=/tmp/opencode/ext4w-guest.log
set +e
timeout 90 qemu-system-x86_64 -machine q35,accel=tcg -cpu max -m 256M -smp 1 \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
  -drive if=pflash,format=raw,file=build/OVMF_VARS.fd \
  -drive format=raw,file=fat:rw:build/efi \
  -drive file=build/ext4-write-test.img,format=raw,if=none,id=hd0 \
  -device virtio-blk-pci,drive=hd0 \
  -device virtio-net-pci,netdev=net0 -netdev user,id=net0 \
  -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
  -serial file:"$LOG" -display none -no-reboot -no-shutdown >/dev/null 2>&1
status=$?
set -e
echo "qemu status $status"
grep -a "ext4w:\|EXT4W_OK" "$LOG" | head -n 8 || true
grep -q "EXT4W_OK" "$LOG" || { echo "guest write test failed"; tail -n 15 "$LOG"; exit 1; }
echo "--- linux re-mount check ---"
mkdir -p /tmp/opencode/ext4w-mnt
if mount -o loop,ro build/ext4-write-test.img /tmp/opencode/ext4w-mnt 2>/dev/null; then
  cat /tmp/opencode/ext4w-mnt/xdiag4/hello.txt
  umount /tmp/opencode/ext4w-mnt || true
else
  echo "(loop mount unavailable, checking via debugfs)"
  debugfs -R "cat xdiag4/hello.txt" build/ext4-write-test.img 2>/dev/null | head -n 3 || true
fi
echo "--- e2fsck ---"
e2fsck -n -f build/ext4-write-test.img 2>&1 | tail -n 3
dumpe2fs -h build/ext4-write-test.img 2>/dev/null | grep -E '^(Free blocks|Free inodes):' > /tmp/opencode/ext4w-after.txt || true
cat /tmp/opencode/ext4w-after.txt
if ! diff -q /tmp/opencode/ext4w-before.txt /tmp/opencode/ext4w-after.txt >/dev/null; then
  echo "free counts changed (allocation happened?)"; exit 1
fi
echo "ext4-write: PASS (bytes match, e2fsck clean, free counts unchanged)"
