#!/bin/sh
# Builds a small single-group ext2 image for the guest read/write tests.
# The guest differential battery mounts it at /mnt (virtio-blk) while the
# same test program runs against /tmp natively.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
IMG=build/ext2.img
if ! command -v mke2fs >/dev/null 2>&1; then
  echo "missing required tool: mke2fs (e2fsprogs)" >&2
  exit 1
fi
mkdir -p build
rm -f "$IMG"
# 8 MiB, 1 KiB blocks, one block group; plain linear directories so the
# in-kernel reader needs no dir_index handling.
mke2fs -t ext2 -b 1024 -N 128 -O ^dir_index,^uninit_bg,^extent,^huge_file,^flex_bg,^metadata_csum -q "$IMG" 8192
printf 'created %s\n' "$IMG"
# Record the pristine free counts so the harness can verify that the guest
# allocator returns every block and inode it borrowed.
dumpe2fs -h "$IMG" 2>/dev/null | grep -E '^(Free blocks|Free inodes):' > build/ext2-pristine-counts.txt || true
cat build/ext2-pristine-counts.txt
