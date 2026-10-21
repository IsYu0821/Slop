#!/bin/sh
set -eu
# The firmware files differ between distributions: some ship the 2 MiB split
# pair, others only the 4 MiB build. Pick the first pair that exists.
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
: "${OVMF_CODE:?no OVMF firmware code file found}"
: "${OVMF_VARS_TEMPLATE:?no OVMF firmware variable template found}"
export OVMF_CODE OVMF_VARS_TEMPLATE
QEMU=${QEMU:-qemu-system-x86_64}
test -f build/efi/EFI/BOOT/BOOTX64.EFI || ./tools/build-uefi.sh
test -f "$OVMF_CODE"
test -f "$OVMF_VARS_TEMPLATE"
cp "$OVMF_VARS_TEMPLATE" build/OVMF_VARS.fd
exec "$QEMU" -machine q35,accel=tcg -cpu max -m 256M -smp 1 \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
  -drive if=pflash,format=raw,file=build/OVMF_VARS.fd \
  -drive format=raw,file=fat:rw:build/efi \
  -device virtio-net-pci,netdev=net0,disable-modern=on \
  -netdev user,id=net0 \
  -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
  -serial stdio -display none -no-reboot -no-shutdown
