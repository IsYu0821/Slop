# Milestone 31: futex, writable VFS and directory listing

Three subsystems that standalone Linux programs lean on now work: futex, file
creation with a writable backing store, and `getdents64`.

## Futex

`kernel/futex.rs` implements `FUTEX_WAIT` and `FUTEX_WAKE` (with the
`FUTEX_PRIVATE_FLAG` bit honoured as a no-op qualifier). A fixed 64-entry waiter
table keyed by user address backs both operations:

- `FUTEX_WAIT` reads the u32 at the user address and returns `EAGAIN` when it
  differs from the caller's expected value, matching the Linux value race
  check. Otherwise the task registers a waiter and blocks; a waker clears the
  slot, and a timeout converts the absolute `timespec` deadline into a tick
  wake. `ETIMEDOUT` is returned when the deadline elapses first.
- `FUTEX_WAKE` clears up to `count` waiters on the address and wakes them,
  returning how many were woken.

`REQUEUE`, `CMP_REQUEUE` and `PI` variants return `ENOSYS`. A non-aligned or
null address returns `EFAULT`.

## Writable VFS

`Inode` gained a `capacity` field. `create` allocates and zeroes a physical
frame as the backing page, so created files hold up to 4 KiB, and `write`
appends at the open-file offset, growing `size`. `unlink` frees the backing
page and removes the node. `openat` now accepts `O_CREAT` (creating the node
when missing) and the full Linux open-flag range, and `write` on an inode
descriptor goes through the VFS. `unlinkat` was added, handling the plain
unlink form. `mount_initramfs` preserves `S_IFDIR` modes from the archive, so
directory nodes load as directories.

## getdents64

`vfs::readdir` now enumerates the entries directly below a directory through a
callback, and `sys_getdents64` renders `linux_dirent64` records (ino, offset,
reclen, type, name) using the descriptor offset as a cursor across calls. The
initramfs carries `/bin` and `/etc` directory nodes.

## libc

`read`, `write`, `close` and `openat` now funnel failures through `sys_fail` so
`errno` is set; previously they returned the raw negative syscall value. Added
`unlink`, `getdents64`, `struct dirent64`, the `O_CREAT`/`O_DIRECTORY` flags
and the common `E*` errno constants.

`/bin/hello-file` covers the new surface: create/write/read-back/unlink,
`getdents64` over `/bin`, and the `FUTEX_WAIT` `EAGAIN` path.
