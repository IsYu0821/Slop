#!/bin/sh
# Track 4 validation: ext4 read-only + overwrite battery on the NVMe driver
# (no virtio-blk present). Network uses a virtio-net scaffold
# (EXTRA_DEVICES) until the generic-PC NIC driver (Track 7) lands.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
MUSL=${MUSL_GCC:-/tmp/opencode/musl-install/bin/musl-gcc}
test -x "$MUSL" || { echo "musl-gcc not found"; exit 1; }
BACKUP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/slop-nvme-ext4.XXXXXX")
cp tests/minimal-linux.S "$BACKUP_DIR/minimal-linux.S"
cp tools/build-initramfs.py "$BACKUP_DIR/build-initramfs.py"
cleanup() {
  cp "$BACKUP_DIR/minimal-linux.S" tests/minimal-linux.S
  cp "$BACKUP_DIR/build-initramfs.py" tools/build-initramfs.py
  rm -f tests/musl/differential
  rm -f tests/musl/nopie-helper
  rm -f tests/musl/ext4-write-test
  rm -rf "$BACKUP_DIR"
}
trap cleanup EXIT

phase() { printf '[%s] %s\n' "$(date +%H:%M:%S)" "$1"; }
fail_diag() {
  echo "--- failure diagnostics ---"
  echo "qemu status: ${guest_status:-n/a}"
  echo "--- serial tail ---"
  tail -n 20 "$SERLOG" 2>/dev/null || echo "(no serial log)"
  echo "--- end diagnostics ---"
}

boot_profile_b() {
  # $1 = NVMe image file, $2 = serial log
  SATA_IMG="$1" SERIAL_LOG="$2" \
    EXTRA_DEVICES="-netdev user,id=net1 -device virtio-net-pci,netdev=net1" \
    timeout 300 ./tools/qemu-profile.sh C >/dev/null 2>&1
}

# ---------- phase 1: read-only ----------
phase "ext4 image"
./tools/build-ext4.sh >/tmp/scratchpad/nvme-ext4-build.log 2>&1
cp build/ext4.img build/ahci-test-ext4.img
if [ ! -f build/ext4-pristine-counts.txt ]; then echo "missing pristine counts"; exit 1; fi

phase "guest binary (differential)"
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
./tools/build-uefi.sh >/tmp/scratchpad/nvme-ext4-uefi.log 2>&1
SERLOG=build/nvme-ext4-serial.log
rm -f "$SERLOG"
phase "guest run ro on NVMe (profile B + virtio-net scaffold)"
if boot_profile_b build/ahci-test-ext4.img "$SERLOG"; then
  guest_status=0
else
  guest_status=$?
fi
if [ "$guest_status" -ne 33 ]; then
  echo "guest QEMU status $guest_status (expected 33)"
  fail_diag
  exit 1
fi
phase "guest markers (ro)"
for marker in DIFF_OK DIFF_EXT4_OK DIFF_EXT2_SKIP DIFF_DONE; do
  if ! grep -F "$marker" "$SERLOG" >/dev/null; then
    echo "guest marker missing: $marker"
    fail_diag
    exit 1
  fi
done
if ! grep -F "ahci: selftest PASS" "$SERLOG" >/dev/null; then
  echo "ahci driver selftest marker missing"
  fail_diag
  exit 1
fi
phase "host-side ext4 verification (must be untouched)"
if ! e2fsck -n -f build/ahci-test-ext4.img >/tmp/scratchpad/nvme-ext4-fsck.log 2>&1; then
  echo "NVMe ext4 image failed consistency check after guest run:"
  cat /tmp/scratchpad/nvme-ext4-fsck.log >&2
  exit 1
fi
echo "ext4-on-ahci ro: e2fsck clean"
pristine_blocks=$(awk '/^Free blocks/{print $3}' build/ext4-pristine-counts.txt)
pristine_inodes=$(awk '/^Free inodes/{print $3}' build/ext4-pristine-counts.txt)
now_blocks=$(dumpe2fs -h build/ahci-test-ext4.img 2>/dev/null | awk '/^Free blocks/{print $3}')
now_inodes=$(dumpe2fs -h build/ahci-test-ext4.img 2>/dev/null | awk '/^Free inodes/{print $3}')
if [ "$now_blocks" != "$pristine_blocks" ] || [ "$now_inodes" != "$pristine_inodes" ]; then
  echo "ro mount modified the image: blocks $now_blocks (want $pristine_blocks), inodes $now_inodes (want $pristine_inodes)" >&2
  exit 1
fi
echo "ext4-on-ahci ro: image untouched (blocks=$now_blocks inodes=$now_inodes)"
echo "ahci ext4 ro test: PASS"

# ---------- phase 2: overwrite (disposable copy) ----------
phase "ext4 write image"
SEED=/tmp/opencode/nvme-ext4w-seed
IMG=/tmp/opencode/nvme-ext4w.img
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
cp "$IMG" build/ahci-test-ext4w.img
dumpe2fs -h build/ahci-test-ext4w.img 2>/dev/null | grep -E '^(Free blocks|Free inodes):' > /tmp/opencode/nvme-ext4w-before.txt || true
cat /tmp/opencode/nvme-ext4w-before.txt

phase "guest binary (ext4-write-test)"
"$MUSL" -static -O1 -o build/ext4-write-test tests/ext4-write-test.c
cp build/ext4-write-test tests/musl/ext4-write-test
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
./tools/build-uefi.sh >/tmp/scratchpad/nvme-ext4w-uefi.log 2>&1
SERLOG=build/nvme-ext4w-serial.log
rm -f "$SERLOG"
phase "guest run write on NVMe (profile B + virtio-net scaffold)"
if boot_profile_b build/ahci-test-ext4w.img "$SERLOG"; then
  guest_status=0
else
  guest_status=$?
fi
echo "qemu status $guest_status"
grep -a "ext4w:\|EXT4W_OK" "$SERLOG" | head -n 8 || true
if ! grep -q "EXT4W_OK" "$SERLOG"; then
  echo "guest write test failed"
  fail_diag
  exit 1
fi
phase "host-side write verification"
if ! e2fsck -n -f build/ahci-test-ext4w.img >/tmp/scratchpad/nvme-ext4w-fsck.log 2>&1; then
  echo "NVMe ext4 image failed consistency check after guest write:"
  cat /tmp/scratchpad/nvme-ext4w-fsck.log >&2
  exit 1
fi
echo "ext4-on-ahci write: e2fsck clean"
dumpe2fs -h build/ahci-test-ext4w.img 2>/dev/null | grep -E '^(Free blocks|Free inodes):' > /tmp/opencode/nvme-ext4w-after.txt || true
if ! diff /tmp/opencode/nvme-ext4w-before.txt /tmp/opencode/nvme-ext4w-after.txt >/dev/null; then
  echo "free counts changed by in-place overwrite:" >&2
  diff /tmp/opencode/nvme-ext4w-before.txt /tmp/opencode/nvme-ext4w-after.txt >&2 || true
  exit 1
fi
echo "ext4-on-ahci write: free counts unchanged"
echo "ahci ext4 write test: PASS"
echo "ahci ext4 test: PASS (ro + write)"
