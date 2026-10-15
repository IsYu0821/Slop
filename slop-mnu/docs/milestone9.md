# Milestone 9: recoverable memory boundaries

The first Linux program uses `write`, whose buffer is a user pointer. Earlier versions validated only the numeric lower-half range and could fault the whole kernel on an unmapped address. Usercopy now walks the active page tables before every chunk, verifies effective ancestor and leaf permissions, translates to physical memory, and copies only up to the current page boundary. Invalid, supervisor-only, non-present, or read-only destinations return `EFAULT` without dereferencing the virtual pointer.

ELF installation is now transactional. Every newly mapped `(virtual, physical)` page is recorded. If segment copying, BSS initialization, stack allocation, or stack construction fails, pages are unmapped in reverse order and frames enter a bounded recycle stack. Successful installation explicitly commits the transaction.

This does not yet recover from machine-checks or stale firmware mappings, and the identity physical accessor remains the principal bootstrap limitation. It does close the normal unmapped-user-pointer failure path for the first process.
