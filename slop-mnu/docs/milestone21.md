# Milestone 21: read-only initramfs, VFS and file descriptors

The UEFI image now embeds an initramfs rather than embedding the acceptance ELF as the initrd payload directly. `tools/build-initramfs.py` creates the allocation-free NSAR format with `/init` as an executable regular file and `/etc/motd` as a read-only data file. The UEFI loader exports the complete archive through the existing BootInfo initrd fields.

The kernel validates the NSAR magic, entry count, name length, absolute path form, metadata bounds, data bounds and alignment before publishing any inode. The mounted filesystem supports exact absolute-path lookup and offset-based reads. Boot no longer passes the initrd bytes directly to the ELF loader: it mounts the archive, resolves `/init` through VFS, then passes that inode's bytes to the existing transactional static ELF loader.

A fixed 32-entry FD table initializes descriptors 0, 1 and 2 as console endpoints. `openat(AT_FDCWD, absolute_path, O_RDONLY)` allocates descriptors 3 and above. Each regular-file descriptor carries its own offset. `read`, `lseek`, `close`, `fstat`, and `newfstatat` are connected to initramfs inodes, while writes remain limited to stdout and stderr.

The acceptance ELF now opens `/etc/motd`, reads it through fd 3+, writes the returned bytes to stdout, closes the descriptor, performs the existing EFAULT test, and exits successfully. A kernel-side mount test also resolves `/etc/motd` and verifies its exact contents before entering ring 3.

This is a deliberately read-only first VFS. It does not yet provide directories as first-class inodes, relative lookup, cwd, symlinks, `getdents64`, descriptor duplication, per-task FD-table cloning, or writable tmpfs pages.
