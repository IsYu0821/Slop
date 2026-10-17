#!/bin/sh
set -eu
mkdir -p build
clang -target x86_64-unknown-none-elf -c arch/x86_64/entry.S -o build/entry.o
clang -target x86_64-unknown-none-elf -c arch/x86_64/exceptions.S -o build/exceptions.o
clang -target x86_64-unknown-none-elf -c arch/x86_64/syscall.S -o build/syscall.o
clang -target x86_64-unknown-none-elf -c arch/x86_64/user.S -o build/user.o
clang -target x86_64-unknown-none-elf -c arch/x86_64/irq.S -o build/irq.o
clang -target x86_64-unknown-none-elf -c arch/x86_64/context.S -o build/context.o
cargo +nightly build -Zbuild-std=core --release
ld.lld -nostdlib -z max-page-size=0x1000 -T linker/x86_64.ld build/entry.o build/exceptions.o build/syscall.o build/user.o build/irq.o build/context.o target/x86_64-slop-mnu/release/libslop_mnu_kernel.a -o build/slop-mnu.elf
llvm-readelf -h -l build/slop-mnu.elf
