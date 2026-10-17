#!/bin/sh
set -eu
mkdir -p build/efi/EFI/BOOT
./tools/check-toolchain.sh
./tools/build-test-elf.sh
./tools/build-libc-test.sh
./tools/build-initramfs.py
cargo +nightly build -Zbuild-std=core,compiler_builtins -Zbuild-std-features=compiler-builtins-mem --target x86_64-pc-windows-msvc --release
KLIB=$(find target/x86_64-pc-windows-msvc/release -maxdepth 1 -type f \( -name '*slop_mnu_kernel*.lib' -o -name '*slop_mnu_kernel*.a' \) | head -n 1)
test -n "$KLIB"
clang -target x86_64-pc-win32-coff -ffreestanding -fshort-wchar -mno-red-zone -fno-stack-protector -c loader/main.c -o build/loader.obj
for src in loader/entry.S arch/x86_64/exceptions.S arch/x86_64/syscall.S arch/x86_64/user.S arch/x86_64/irq.S arch/x86_64/context.S; do
  obj=build/$(basename "$src" .S)-coff.obj
  clang -target x86_64-pc-win32-coff -ffreestanding -mno-red-zone -c "$src" -o "$obj"
done
lld-link /subsystem:efi_application /entry:efi_main /nodefaultlib /machine:x64 \
  /out:build/efi/EFI/BOOT/BOOTX64.EFI \
  build/loader.obj build/entry-coff.obj build/exceptions-coff.obj build/syscall-coff.obj build/user-coff.obj build/irq-coff.obj build/context-coff.obj \
  build/initramfs.obj "$KLIB"
printf 'created build/efi/EFI/BOOT/BOOTX64.EFI\n'
