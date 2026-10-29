#!/bin/sh
set -eu
mkdir -p build
clang -target x86_64-unknown-none-elf -ffreestanding -mno-red-zone -c tests/minimal-linux.S -o build/minimal-linux.o
ld.lld -nostdlib -static -T tests/minimal-linux.ld build/minimal-linux.o -o build/minimal-linux.elf
llvm-readelf -h -l build/minimal-linux.elf
# Module-test /init variant: boots into /bin/modtest (Linux .ko + EDU vertical).
clang -target x86_64-unknown-none-elf -ffreestanding -mno-red-zone -DMODTEST_INIT -c tests/minimal-linux.S -o build/minimal-linux-modtest.o
ld.lld -nostdlib -static -T tests/minimal-linux.ld build/minimal-linux-modtest.o -o build/minimal-linux-modtest.elf
llvm-readelf -h -l build/minimal-linux-modtest.elf

clang -target x86_64-unknown-none-elf -ffreestanding -mno-red-zone -c tests/hello-linux.S -o build/hello-linux.o
ld.lld -nostdlib -static -T tests/minimal-linux.ld build/hello-linux.o -o build/hello-linux.elf
llvm-readelf -h -l build/hello-linux.elf
