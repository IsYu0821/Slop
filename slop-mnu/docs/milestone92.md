# Milestone 92: ext4 read-only mount

## Goal

Read a real ext4 filesystem without disturbing the working system:
detect the on-disk format from the superblock, walk extent trees
instead of indirect blocks, and serve files, directories and both
symlink forms through the existing VFS. Every mutating operation on
an ext4 mount fails loudly with EROFS instead of silently corrupting
a journaled filesystem the kernel does not yet replay.

## On-disk contract fixed first

- Same 0xEF53 magic as ext2; ext4 is selected when the incompatible
  feature word carries EXTENTS (0x40) and no 64BIT (0x80) bit.
  Anything else (64bit, unknown incompat bits) refuses to mount
  rather than misread.
- Inodes are the 128-byte ext2-compatible prefix (256-byte on-disk
  records); when `i_flags & EXTENTS` is set, `i_block` holds an
  extent tree (header magic 0xF30A, index nodes, leaf extents) and
  not indirect block numbers.
- Directories use `dir_entry_2`, byte-identical in layout to the
  ext2 reader, so name lookup and `readdir` share the record walk.
- Fast symlinks (target inline in `i_block`, size < 60) and
  block-backed symlinks (extent-mapped) both resolve, including
  open-through and `lstat`-vs-`stat` distinctions.

## Implementation

- `kernel/ext4.rs` (new): superblock probe, group-descriptor and
  inode table walking for arbitrary inode sizes, a recursive extent
  mapper (depth-bounded, holes read back as zeros, uninitialized
  extents read as zeros, `ee_start_hi` must be zero), and a read-only
  surface mirroring ext2: `lookup`, `read`, `readdir`, `is_dir`,
  `readlink_into`, `file_size`. No write path exists at all.
- VFS routing (`kernel/vfs.rs`): new `MountKind::Ext4` plus an
  `ext4` fstype for explicit mounts; the legacy `/mnt` mount
  auto-detects ext2 vs ext4 from the superblock. Inode numbers live
  at `0x200000+inum`, disjoint from ext2's `0x100000` range, with
  `is_ext4_ino` guards so ext4 inodes can never take the ext2 write
  path (`is_ext2_inode` now excludes them; `write_off` and the
  ino-based `truncate` return EROFS).
- All mutating VFS operations on Ext4 mounts (`create`, `unlink`,
  `mkdir`, `rmdir`, `rename`, `chmod`, `truncate`, `mknod`,
  `symlink`, `link`, write) fail with EROFS; `mountinfo` reports
  the `ext4` fstype.
- `tools/build-ext4.sh` (new): builds a 32 MiB, 4 KiB-block ext4
  image with a fixed feature set (extents on; 64bit, metadata_csum,
  dir_index, huge_file, flex_bg, uninit_bg off) and a seed tree
  (`hello.txt`, `sub/nested.txt`, fast + long symlinks, a 100 KiB
  multi-extent file), and records pristine free counts.

## Tests added

- `DIFF_EXT4_OK` battery in `tests/differential.c`: reads every
  seed file byte-for-byte, checks sizes and file types, lists the
  subdirectory, resolves both symlink forms (including open-through
  and `lstat`-vs-`stat`), and — guest-only — asserts EROFS on
  create/mkdir/unlink/rmdir/truncate/write.
- The battery runs natively against a seed-tree copy and in the
  guest against the image; image flavor is probed at runtime, so
  the ext2 battery prints `DIFF_EXT2_SKIP` on ext4 images (and vice
  versa) instead of failing.
- `tools/ext4-test.sh` (new): builds the image, runs the native and
  guest batteries (guest requires `DIFF_EXT4_OK` + `DIFF_EXT2_SKIP`
  + `DIFF_DONE`), then proves read-only operation with `e2fsck -n`
  clean and bit-identical free counts against the pristine record.

## Verification

- `ext4 test: PASS` (e2fsck clean, free counts unchanged
  blocks=7111 inodes=238); native `DIFF_EXT2_OK` + `DIFF_EXT4_OK`.
- Full gate suite green with the ext4 code in the tree:
  check-source, heap, acceptance, differential (e2fsck clean, free
  counts exact, platter bytes verified), busybox 115/115.
- 1/2/4/8 vCPU boots unchanged (SMP bring-up untouched by this
  milestone); writes are uniformly EROFS (MS93 owns read-write).
