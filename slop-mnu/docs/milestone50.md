# Milestone 50: ext2 read-only filesystem

Added a read-only ext2 driver for virtio-blk, including superblock, group
descriptor, inode and directory parsing, plus `/mnt` integration in the VFS.

Acceptance suite remains green.
