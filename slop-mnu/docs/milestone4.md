# Milestone 4: executable image and VMA semantics

This milestone moves compatibility from isolated syscall stubs toward a coherent Linux process image.

## ELF

The parser reads little-endian fields from bytes instead of casting an arbitrary buffer to an aligned Rust reference. It accepts x86-64 `ET_EXEC` and `ET_DYN`, validates the complete program-header table, plans up to 16 `PT_LOAD` segments, rejects overflow/truncation, verifies alignment congruence, records `PT_INTERP`, and requires the entry point to fall inside the image.

## Virtual memory

A fixed-capacity, allocation-free VMA ledger tracks sorted non-overlapping user ranges and Linux protection bits. Anonymous private mappings can now reserve addresses through `mmap`; exact mappings can be changed or removed through `mprotect` and `munmap`.

This is intentionally the policy layer only. A returned mapping is not usable until the architecture page-table backend installs PTEs and page-fault handling allocates/zeros frames. The next milestone must connect every ledger mutation transactionally to hardware page tables before Linux binaries execute.
