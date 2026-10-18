#!/bin/sh
set -eu
mkdir -p build/libc
CFLAGS="-target x86_64-unknown-none-elf -ffreestanding -fno-stack-protector -fno-pic -mcmodel=large -mno-red-zone -O2 -Wall -Wextra -Ilibc/include -Ilibc"
for src in libc/start.S libc/syscall.S; do
  clang $CFLAGS -c "$src" -o "build/libc/$(basename "$src" .S).o"
done
for src in libc/string.c libc/stdio.c libc/stdlib.c libc/unistd.c libc/errno.c libc/signal.c; do
  clang $CFLAGS -c "$src" -o "build/libc/$(basename "$src" .c).o"
done
for src in tests/hello-c.c tests/hello-posix.c tests/hello-signal.c tests/hello-file.c; do
  clang $CFLAGS -c "$src" -o "build/libc/$(basename "$src" .c).o"
done
LIBC="build/libc/start.o build/libc/syscall.o build/libc/string.o build/libc/stdio.o build/libc/stdlib.o build/libc/unistd.o build/libc/errno.o build/libc/signal.o"
ld.lld -nostdlib -static -T tests/minimal-linux.ld $LIBC build/libc/hello-c.o -o build/hello-c.elf
ld.lld -nostdlib -static -T tests/minimal-linux.ld $LIBC build/libc/hello-posix.o -o build/hello-posix.elf
ld.lld -nostdlib -static -T tests/minimal-linux.ld $LIBC build/libc/hello-signal.o -o build/hello-signal.elf
ld.lld -nostdlib -static -T tests/minimal-linux.ld $LIBC build/libc/hello-file.o -o build/hello-file.elf
llvm-readelf -h -l build/hello-c.elf build/hello-posix.elf build/hello-signal.elf build/hello-file.elf
