#!/bin/sh
set -eu
# Builds the vector system call entry point.
#
# Linux gives every program a page of code that it can call without
# entering the kernel. The page is a shared object with the usual entry
# points, and the C library finds them through the dynamic symbol table
# that the auxiliary vector points at.
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
mkdir -p build
clang -c -nostdlib -fno-stack-protector -fno-asynchronous-unwind-tables \
  -mno-red-zone -Wa,--noexecstack vdso/vdso.S -o build/vdso.o
ld.lld -shared -nostdlib --hash-style=both --build-id=none \
  -z max-page-size=4096 -z noexecstack -soname linux-vdso.so.1 \
  -T vdso/vdso.ld build/vdso.o -o build/vdso.so
# The C library looks the entry points up in the symbol table and needs the
# hash table beside it, so the build fails when one of them is missing.
llvm-readelf -d build/vdso.so | grep -q 'HASH' || {
  echo 'vdso: the dynamic hash table is missing' >&2
  exit 1
}
llvm-readelf --dyn-syms build/vdso.so | grep -q '__vdso_clock_gettime' || {
  echo 'vdso: the clock entry point is not exported' >&2
  exit 1
}
# The C library computes the load address of the page from the last
# loadable segment, so the file offset and the address of every loadable
# segment have to be the same.
llvm-readelf -l build/vdso.so | awk '
  $1=="LOAD" { if ($2 != $3) { print "vdso: a loadable segment has a file offset that differs from its address" > "/dev/stderr"; exit 1 } }
' || exit 1
printf 'created build/vdso.so\n'
