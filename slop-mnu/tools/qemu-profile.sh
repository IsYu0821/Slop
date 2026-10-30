#!/bin/sh
# Reproducible QEMU profiles for real-machine-equivalent bring-up.
# No hand-assembled command lines: every profile is a named, reviewable set.
#
#   ./tools/qemu-profile.sh A [extra qemu args]   legacy regression
#     q35 + OVMF + virtio-blk + virtio-net + EDU (existing tests)
#   ./tools/qemu-profile.sh B [extra qemu args]   generic PC, no paravirt
#     q35 + OVMF + NVMe + xHCI (+usb-kbd/mouse) + e1000e, no virtio/EDU
#   ./tools/qemu-profile.sh C [extra qemu args]   AHCI/SATA variant
#     q35 + OVMF + AHCI/SATA + xHCI + e1000e, no virtio/EDU
#   ./tools/qemu-profile.sh D [extra qemu args]   IOMMU variant
#     profile B + Intel IOMMU (DMA + interrupt remapping)
#
# Knobs (env): SMP (default 1), MEM (default 512M), ACCEL (default kvm if
# available, else tcg), OVMF_CODE/OVMF_VARS_TEMPLATE (autodetected),
# SERIAL_LOG (default build/profile-serial.log), EFI_DIR (default
# build/efi), EXTRA_DEVICES (appended verbatim, for USB hotplug tests).
# Exit status mirrors isa-debug-exit (guest value 0x10 -> host 33).
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
PROFILE=${1:-B}; shift 2>/dev/null || true
: "${SMP:=1}"
: "${MEM:=512M}"
if [ -z "${ACCEL:-}" ]; then
  if [ -c /dev/kvm ]; then ACCEL=kvm; else ACCEL=tcg; fi
fi
if [ -z "${OVMF_CODE:-}" ]; then
  for c in /usr/share/OVMF/OVMF_CODE.fd /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/edk2/ovmf/OVMF_CODE.fd; do
    [ -f "$c" ] && OVMF_CODE=$c && break
  done
fi
if [ -z "${OVMF_VARS_TEMPLATE:-}" ]; then
  for v in /usr/share/OVMF/OVMF_VARS.fd /usr/share/OVMF/OVMF_VARS_4M.fd /usr/share/edk2/ovmf/OVMF_VARS.fd; do
    [ -f "$v" ] && OVMF_VARS_TEMPLATE=$v && break
  done
fi
: "${OVMF_CODE:?no OVMF code found}"
: "${OVMF_VARS_TEMPLATE:?no OVMF vars template found}"
: "${SERIAL_LOG:=build/profile-serial.log}"
: "${EFI_DIR:=build/efi}"
mkdir -p build
VARS=$(mktemp "${TMPDIR:-/tmp}/slop-vars.XXXXXX.fd")
trap 'rm -f "$VARS"' EXIT INT TERM
cp "$OVMF_VARS_TEMPLATE" "$VARS"
QEMU=${QEMU:-qemu-system-x86_64}
CPUFLAGS="host"
[ "$ACCEL" = "tcg" ] && CPUFLAGS="max"
MACHINE="q35,accel=$ACCEL"
[ "$PROFILE" = "D" ] && MACHINE="$MACHINE,kernel-irqchip=split"
set -- -machine "$MACHINE" -cpu "$CPUFLAGS" -m "$MEM" -smp "$SMP" \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
  -drive if=pflash,format=raw,file="$VARS" \
  -drive format=raw,file=fat:rw:"$EFI_DIR" \
  -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
  -serial file:"$SERIAL_LOG" -display none -no-reboot -no-shutdown "$@"
case "$PROFILE" in
  A)
    # Legacy regression: virtio block+net and the EDU test device.
    set -- "$@" -device edu \
      -drive file=build/ext2.img,format=raw,if=none,id=hd0 \
      -device virtio-blk-pci,drive=hd0 \
      -device virtio-net-pci,netdev=net0 -netdev user,id=net0
    ;;
  B)
    # Generic PC: NVMe system disk, xHCI with keyboard+mouse, e1000e NIC.
    # No virtio, no EDU. Drives are created on demand (see below).
    [ -f build/nvme-disk.img ] || qemu-img create -f raw build/nvme-disk.img 64M
    set -- "$@" \
      -drive file=build/nvme-disk.img,format=raw,if=none,id=nvme0 \
      -device nvme,drive=nvme0,serial=nvme-generic-pc \
      -device qemu-xhci,id=xhci \
      -device usb-kbd -device usb-mouse \
      -device e1000e,netdev=net0 -netdev user,id=net0
    ;;
  C)
    # AHCI/SATA variant of the generic PC.
    [ -f build/sata-disk.img ] || qemu-img create -f raw build/sata-disk.img 64M
    set -- "$@" \
      -drive file=build/sata-disk.img,format=raw,if=none,id=sata0 \
      -device ich9-ahci,id=ahci \
      -device ide-hd,drive=sata0,bus=ahci.0 \
      -device qemu-xhci,id=xhci \
      -device usb-kbd -device usb-mouse \
      -device e1000e,netdev=net0 -netdev user,id=net0
    ;;
  D)
    # IOMMU variant of the generic PC (Intel VT-d + interrupt remapping).
    [ -f build/nvme-disk.img ] || qemu-img create -f raw build/nvme-disk.img 64M
    set -- "$@" \
      -device intel-iommu,intremap=on \
      -drive file=build/nvme-disk.img,format=raw,if=none,id=nvme0 \
      -device nvme,drive=nvme0,serial=nvme-generic-pc \
      -device qemu-xhci,id=xhci \
      -device usb-kbd -device usb-mouse \
      -device e1000e,netdev=net0 -netdev user,id=net0
    ;;
  *)
    echo "unknown profile: $PROFILE (want A, B, C or D)" >&2; exit 2 ;;
esac
if [ -n "${EXTRA_DEVICES:-}" ]; then
  # shellcheck disable=SC2086
  set -- "$@" $EXTRA_DEVICES
fi
echo "profile $PROFILE: $QEMU $*" > "${SERIAL_LOG}.cmd"
exec "$QEMU" "$@"
