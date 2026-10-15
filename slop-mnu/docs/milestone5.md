# Milestone 5: Linux process image construction

This milestone constructs the data required immediately before entering a Linux x86-64 executable.

The execution planner turns validated ELF `PT_LOAD` records into page-aligned mapping operations, derives Linux protection flags, locates the in-memory program-header table for `AT_PHDR`, reserves a 1 MiB user stack, and records whether `PT_INTERP` requires a dynamic linker.

The stack builder creates `argc`, NULL-terminated `argv`, NULL-terminated `envp`, and the auxiliary vector. It copies all strings, places 16 random bytes, includes `AT_EXECFN`, and returns a 16-byte-aligned initial RSP. It rejects embedded NUL bytes and oversized input without partially writing outside the supplied stack buffer.

Remaining integration work is deliberately separate: install PTEs, copy segment bytes, zero BSS, commit the stack page, set FS state, and transition to ring 3. Dynamic binaries remain blocked until the interpreter and file-backed mmap path exist.
