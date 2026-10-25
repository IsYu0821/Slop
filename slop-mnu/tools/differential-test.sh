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
  rm -f tests/musl/nopie-helper
  rm -rf "$BACKUP_DIR"
}
trap cleanup EXIT

"$MUSL" -static -O1 -o build/differential-host tests/differential.c
"$MUSL" -O1 -o build/differential tests/differential.c
"$MUSL" -static -no-pie -O1 -o build/nopie-helper tests/nopie-helper.c

if ! ./build/differential-host > build/differential-native.out; then
  echo "native differential test failed"
  exit 1
fi
for marker in DIFF_OK DIFF_ERRNO_OK DIFF_PROC_OK DIFF_CAP_OK DIFF_SECCOMP_OK DIFF_INOTIFY_OK DIFF_PTHREAD_OK DIFF_COND_OK DIFF_KILL_OK DIFF_TLS_OK DIFF_STOP_OK DIFF_FP_OK DIFF_SIGNALFD_STOP_OK DIFF_SIGNALFD_NOCLDSTOP_OK DIFF_CFR_OK DIFF_SPLICE_OK DIFF_TEE_OK DIFF_FALLOC_OK DIFF_TCPSRV_OK DIFF_UDPLOOP_OK DIFF_UNIXDG_OK DIFF_EXT2_OK DIFF_DONE; do
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
cp build/nopie-helper tests/musl/nopie-helper
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
./tools/build-uefi.sh >/tmp/scratchpad/diff-build.log 2>&1
./tools/build-ext2.sh >/tmp/scratchpad/ext2-build.log 2>&1
OVMF_CODE=${OVMF_CODE:-/usr/share/OVMF/OVMF_CODE_4M.fd}
OVMF_VARS_TEMPLATE=${OVMF_VARS_TEMPLATE:-/usr/share/OVMF/OVMF_VARS_4M.fd}
cp "$OVMF_VARS_TEMPLATE" build/OVMF_VARS.fd
rm -f build/differential-serial.log
if timeout 240 qemu-system-x86_64 -machine q35,accel=tcg -cpu max -m 256M -smp 1 \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
  -drive if=pflash,format=raw,file=build/OVMF_VARS.fd \
  -drive format=raw,file=fat:rw:build/efi \
  -drive file=build/ext2.img,format=raw,if=none,id=hd0 \
  -device virtio-blk-pci,drive=hd0 \
  -device virtio-net-pci,netdev=net0 -netdev user,id=net0 \
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

for marker in DIFF_OK DIFF_ERRNO_OK DIFF_PROC_OK DIFF_CAP_OK DIFF_SECCOMP_OK DIFF_INOTIFY_OK DIFF_IOURING_OK DIFF_PTHREAD_OK DIFF_COND_OK DIFF_KILL_OK DIFF_TLS_OK DIFF_STOP_OK DIFF_FP_OK DIFF_SIGNALFD_STOP_OK DIFF_SIGNALFD_NOCLDSTOP_OK DIFF_CFR_OK DIFF_SPLICE_OK DIFF_TEE_OK DIFF_FALLOC_OK DIFF_TCPSRV_OK DIFF_UDPLOOP_OK DIFF_UNIXDG_OK DIFF_EXT2_OK DIFF_NOPIE_OK DIFF_RTSIG_OK DIFF_ALTSTACK_OK DIFF_DONE; do
  if ! grep -F "$marker" build/differential-serial.log >/dev/null; then
    echo "guest marker missing: $marker"
    exit 1
  fi
done

# Host-side proof that the guest wrote a real ext2 filesystem: the journal
# of record is the platter itself, checked with the host e2fsprogs.
if ! command -v e2fsck >/dev/null 2>&1 || ! command -v debugfs >/dev/null 2>&1; then
  echo "missing required tools: e2fsck/debugfs (e2fsprogs)" >&2
  exit 1
fi
if ! e2fsck -n -f build/ext2.img >/tmp/scratchpad/ext2-fsck.log 2>&1; then
  echo "ext2 image failed consistency check after guest run:"
  cat /tmp/scratchpad/ext2-fsck.log >&2
  exit 1
fi
echo "ext2: e2fsck clean"
# The guest test leaves exactly the base dir and keep.bin allocated, so the
# free counts must be the pristine ones minus 2 blocks and 2 inodes.
pristine_blocks=$(awk '/^Free blocks/{print $3}' build/ext2-pristine-counts.txt)
pristine_inodes=$(awk '/^Free inodes/{print $3}' build/ext2-pristine-counts.txt)
now_blocks=$(dumpe2fs -h build/ext2.img 2>/dev/null | awk '/^Free blocks/{print $3}')
now_inodes=$(dumpe2fs -h build/ext2.img 2>/dev/null | awk '/^Free inodes/{print $3}')
if [ "$now_blocks" != "$((pristine_blocks - 2))" ] || [ "$now_inodes" != "$((pristine_inodes - 2))" ]; then
  echo "ext2 free-count mismatch: blocks $now_blocks (want $((pristine_blocks - 2))), inodes $now_inodes (want $((pristine_inodes - 2)))" >&2
  exit 1
fi
echo "ext2: free counts account exactly (blocks=$now_blocks inodes=$now_inodes)"
printf '0123456789abcdef' > /tmp/scratchpad/ext2-expect.bin
if ! debugfs -R "dump xdiag/keep.bin /tmp/scratchpad/ext2-got.bin" build/ext2.img >/dev/null 2>&1; then
  echo "ext2: debugfs could not dump the guest-written file" >&2
  exit 1
fi
if ! cmp -s /tmp/scratchpad/ext2-expect.bin /tmp/scratchpad/ext2-got.bin; then
  echo "ext2: guest-written bytes differ on the platter" >&2
  exit 1
fi
echo "ext2: guest bytes verified on the platter"
echo "differential test: PASS"
