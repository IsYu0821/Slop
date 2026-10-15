# Early security invariants

- Firmware pointers are untrusted until BootInfo validation succeeds.
- Integer overflow is rejected while importing memory ranges.
- Only memory marked usable by the loader enters the frame allocator.
- No raw userspace pointer is dereferenced by a syscall implementation; data crosses through usercopy.
- User virtual ranges must be canonical lower-half addresses and may not include the null guard page.
- The current usercopy implementation validates ranges but cannot yet recover from a page fault. Page-table presence checks and exception-table recovery are milestone 2B release blockers.
- Spin locks are SMP-safe but do not yet mask interrupts; interrupt-context use is prohibited until IRQ guards are added.
