#!/bin/sh
# Track 4 validation: ext2 differential battery with the filesystem image
# served by the NVMe driver (no virtio-blk present). Network still uses a
# virtio-net scaffold (EXTRA_DEVICES) until the generic-PC NIC driver
# (Track 7) lands; the block device under test is NVMe only.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
MUSL=${MUSL_GCC:-/tmp/opencode/musl-install/bin/musl-gcc}
test -x "$MUSL" || { echo "musl-gcc not found"; exit 1; }
BACKUP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/slop-ahci-test.XXXXXX")
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
  echo "qemu status: ${guest_status:-n/a} (expected 33 + markers)"
  echo "--- serial tail ---"
  tail -n 20 build/ahci-serial.log 2>/dev/null || echo "(no serial log)"
  echo "--- end diagnostics ---"
}

phase "ext2 image on NVMe file"
./tools/build-ext2.sh >/tmp/scratchpad/nvme-ext2-build.log 2>&1
cp build/ext2.img build/ahci-test-ext2.img

phase "guest binary"
"$MUSL" -O1 -o build/differential tests/differential.c
"$MUSL" -static -no-pie -O1 -o build/nopie-helper tests/nopie-helper.c

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
./tools/build-uefi.sh >/tmp/scratchpad/nvme-build.log 2>&1
rm -f build/ahci-serial.log
phase "guest run on NVMe (profile B + virtio-net scaffold, 300s budget)"
if SATA_IMG=build/ahci-test-ext2.img \
   SERIAL_LOG=build/ahci-serial.log \
   EXTRA_DEVICES="-netdev user,id=net1 -device virtio-net-pci,netdev=net1" \
   timeout 300 ./tools/qemu-profile.sh C >/dev/null 2>&1; then
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
for marker in DIFF_OK DIFF_EXT2_OK DIFF_EXT4_SKIP DIFF_DONE; do
  if ! grep -F "$marker" build/ahci-serial.log >/dev/null; then
    echo "guest marker missing: $marker"
    fail_diag
    exit 1
  fi
done
if ! grep -F "ahci: selftest PASS" build/ahci-serial.log >/dev/null; then
  echo "ahci driver selftest marker missing"
  fail_diag
  exit 1
fi
phase "host-side ext2 verification on the NVMe image"
if ! e2fsck -n -f build/ahci-test-ext2.img >/tmp/scratchpad/nvme-fsck.log 2>&1; then
  echo "NVMe ext2 image failed consistency check after guest run:"
  cat /tmp/scratchpad/nvme-fsck.log >&2
  exit 1
fi
echo "ext2-on-ahci: e2fsck clean"
pristine_blocks=$(awk '/^Free blocks/{print $3}' build/ext2-pristine-counts.txt)
pristine_inodes=$(awk '/^Free inodes/{print $3}' build/ext2-pristine-counts.txt)
now_blocks=$(dumpe2fs -h build/ahci-test-ext2.img 2>/dev/null | awk '/^Free blocks/{print $3}')
now_inodes=$(dumpe2fs -h build/ahci-test-ext2.img 2>/dev/null | awk '/^Free inodes/{print $3}')
if [ "$now_blocks" != "$((pristine_blocks - 2))" ] || [ "$now_inodes" != "$((pristine_inodes - 2))" ]; then
  echo "free-count mismatch: blocks $now_blocks (want $((pristine_blocks - 2))), inodes $now_inodes (want $((pristine_inodes - 2)))" >&2
  exit 1
fi
echo "ext2-on-ahci: free counts account exactly (blocks=$now_blocks inodes=$now_inodes)"
printf '0123456789abcdef' > /tmp/scratchpad/nvme-expect.bin
if ! debugfs -R "dump xdiag/keep.bin /tmp/scratchpad/nvme-got.bin" build/ahci-test-ext2.img >/dev/null 2>&1; then
  echo "debugfs could not dump the guest-written file" >&2
  exit 1
fi
if ! cmp -s /tmp/scratchpad/nvme-expect.bin /tmp/scratchpad/nvme-got.bin; then
  echo "guest-written bytes differ on the platter" >&2
  exit 1
fi
echo "ext2-on-ahci: guest bytes verified on the platter"
echo "ahci ext2 test: PASS"
