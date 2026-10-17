#!/bin/sh
set -eu
mkdir -p build/libc
CFLAGS="-target x86_64-unknown-none-elf -ffreestanding -fno-stack-protector -fno-pic -mcmodel=large -mno-red-zone -O2 -Wall -Wextra -Ilibc/include -Ilibc"
for src in libc/start.S libc/syscall.S; do
  clang $CFLAGS -c "$src" -o "build/libc/$(basename "$src" .S).o"
done
for src in libc/string.c libc/stdio.c libc/stdlib.c libc/unistd.c tests/hello-c.c; do
  clang $CFLAGS -c "$src" -o "build/libc/$(basename "$src" .c).o"
done
ld.lld -nostdlib -static -T tests/minimal-linux.ld build/libc/*.o -o build/hello-c.elf
llvm-readelf -h -l build/hello-c.elf
