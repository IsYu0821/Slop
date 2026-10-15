#!/bin/sh
set -eu
missing=0
for tool in cargo rustup clang ld.lld lld-link llvm-objcopy llvm-readelf python3; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    printf 'missing required tool: %s\n' "$tool" >&2
    missing=1
  fi
done
if [ "$missing" -ne 0 ]; then exit 1; fi
rustup run nightly rustc --version
clang --version | head -n 1
lld-link --version | head -n 1
./tools/check-source.sh
printf 'toolchain and source preflight passed\n'
