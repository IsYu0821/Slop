#!/bin/sh
set -eu
OVMF_CODE=${OVMF_CODE:-/usr/share/OVMF/OVMF_CODE.fd}
OVMF_VARS_TEMPLATE=${OVMF_VARS_TEMPLATE:-/usr/share/OVMF/OVMF_VARS.fd}
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
