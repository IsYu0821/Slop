# Milestone 22: hardware-backed anonymous VM and brk

Anonymous private mappings now allocate one physical frame per page, clear the frame through the bootstrap physical-access mapping, and install a user PTE in the currently active process page table. PTE permissions are derived from Linux protection bits: writable follows `PROT_WRITE`, executable pages clear NX, and non-executable pages set NX. Every mapping remains user-accessible.

`mmap` first reserves a VMA and then maps pages. If frame allocation or page-table insertion fails partway through, all successfully mapped pages are removed and recycled, and the VMA reservation is rolled back. `MAP_FIXED` currently rejects overlap rather than replacing an existing mapping, which avoids destructive partial replacement until VMA splitting is implemented.

`mprotect` updates each leaf PTE through `protect_page`, including TLB invalidation, then updates the VMA ledger. `munmap` removes every leaf PTE, invalidates it, recycles the frame, and removes the exact VMA. Partial VMA ranges are not accepted yet.

Upward `brk` growth reserves an anonymous read/write VMA and maps zero-filled pages between the old and new page boundary. Shrinking is conservatively rejected until adjacent brk extents can be merged and split transactionally. Linux semantics are preserved by returning the old break on failure.

The ring-3 acceptance program now requests a writable anonymous page, verifies zero fill, writes and reads a value, changes the page to read-only with `mprotect`, unmaps it, grows brk by one page, verifies zero fill, and writes and reads heap data. Its success marker is `VFS, VM and EFAULT passed`.
