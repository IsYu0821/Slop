# Milestone 38: Broad Linux ABI compatibility boost

Added a large set of Linux syscall numbers and handlers or safe stubs:
path/root VFS support for `/`, `readlink`, `statfs`, `fstatfs`, `close_range`,
`mincore`, `fallocate`, `sendfile`, `sync`, `getcpu`, `sched_setaffinity`,
`mlock2`, `personality`, `nice`, `sync_file_range`, xattr stubs, fadvise
stubs, and process/session helpers. `fallocate` grows initramfs files within
capacity; `sendfile` copies between descriptors. `opendir`/`readdir` now work
on `/`.

Acceptance suite remains green.
