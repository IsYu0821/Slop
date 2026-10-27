#!/bin/sh
# Builds a small single-group ext4 image for the guest read-only tests.
# The guest differential battery mounts it at /mnt (virtio-blk) while the
# same test program reads the seed tree natively. The feature set is kept
# minimal on purpose: extents on (the point of the exercise), 64bit,
# metadata_csum, dir_index, huge_file, flex_bg and uninit_bg off so the
# in-kernel reader only needs the classic 32-bit extent format.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
IMG=build/ext4.img
SEED=build/ext4-seed
if ! command -v mke2fs >/dev/null 2>&1; then
  echo "missing required tool: mke2fs (e2fsprogs)" >&2
  exit 1
fi
mkdir -p "$ROOT/$SEED/xdiag4/sub"
# Small text file with known content.
printf 'Hello from ext4.\nSecond line here.\n' > "$ROOT/$SEED/xdiag4/hello.txt"
# Nested file to exercise multi-component lookup.
printf 'nested content 0123456789\n' > "$ROOT/$SEED/xdiag4/sub/nested.txt"
# Fast symlink (target fits inline) and a long one (block-backed).
ln -sf hello.txt "$ROOT/$SEED/xdiag4/a-sym" 2>/dev/null || true
python3 - "$ROOT/$SEED" <<'PY'
import os, sys
seed = sys.argv[1]
# 100 KiB patterned file: spans many blocks / multiple extents on 4 KiB blocks.
with open(os.path.join(seed, 'xdiag4', 'big.bin'), 'wb') as f:
    for c in range(25):
        f.write(bytes(((c * 131 + i * 7 + 3) & 0xff) for i in range(4096)))
# Long symlink target (>60 bytes) for the block-backed symlink path.
tgt = 'x' * 100
try:
    os.symlink(tgt, os.path.join(seed, 'xdiag4', 'b-sym'))
except FileExistsError:
    pass
PY
rm -f "$IMG"
# 32 MiB, 4 KiB blocks; keep to one block group for a tiny image.
mke2fs -t ext4 -b 4096 -N 256 -O extent,^64bit,^metadata_csum,^dir_index,^huge_file,^flex_bg,^uninit_bg -d "$SEED" -q "$IMG" 8192
printf 'created %s\n' "$IMG"
# Record the pristine free counts so the harness can verify the guest
# never wrote to the read-only image.
dumpe2fs -h "$IMG" 2>/dev/null | grep -E '^(Free blocks|Free inodes):' > build/ext4-pristine-counts.txt || true
cat build/ext4-pristine-counts.txt
