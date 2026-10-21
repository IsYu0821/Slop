# One-Linux-program priority status

## Implemented and covered by the acceptance gate

- the published x86-64 `rt_sigframe` with the `ucontext_t` and the `siginfo_t`
- the `iretq` syscall return, so the user flags and the syscall result both
  survive a call
- `rt_sigaction` with the Linux `k_sigaction` field order, `SA_SIGINFO`,
  `SA_NODEFER`, `SA_RESETHAND` and the restorer requirement
- `EINTR` for a blocking `wait4` or `waitid` that a handled signal interrupts
- 1024 descriptors per process and 512 open file descriptions
- a virtual-memory area ledger that merges adjacent areas and reuses a
  released range
- the Linux `madvise` advice set and `rseq` reporting `ENOSYS`

## Implemented in source, pending build/runtime validation

- UEFI application source and `BOOTX64.EFI` build command
- final memory-map acquisition and bounded `ExitBootServices` retry
- `BootInfo` ABI validation and physical allocator startup
- four-level 4 KiB page-table operations: map, unmap, protect, CR3, INVLPG
- GDT selectors for kernel/user code and data
- 64-bit TSS with RSP0 and IST1
- ring-0 and double-fault stacks
- IDT construction
- fatal default, double-fault and page-fault diagnostics
- Linux ELF load into a private address space with a ring-3 transition
- read-only initramfs, `execve`, a per-process address space, `fork`, `wait4`
- pipes, descriptor tables, `dup2`, `dup3` and a minimal `fcntl`
- anonymous `mmap`, `mprotect`, `munmap` and `brk`
- the syscall return path through `iretq`, which keeps the syscall result in
  RAX and restores the user flags
- Linux signal delivery: the published `rt_sigframe` on the user stack, the
  `ucontext_t` and `siginfo_t`, `rt_sigreturn`, `SA_SIGINFO`, `SA_NODEFER`,
  `SA_RESETHAND`, the `SA_RESTORER` requirement, `kill` to a process or a
  process group, `rt_sigpending` and `EINTR` for an interrupted blocking wait

## Open defects

- The allocator metadata of a musl program is still overwritten now and
  then: the BusyBox battery fails intermittently with an allocator assertion
  and the differential battery stops at the pending-signal check. The address
  layout fix removed the page table collision that made the corruption
  common, but a second source is left. The differential battery now runs
  through twenty-five checks instead of five, which makes it the better
  reproducer.
- A `tkill` with a stale thread identifier reports `ESRCH` like Linux does,
  but a C library that caches its thread identifier across a fork then
  signals the wrong thread. The pending-signal check in the differential
  battery fails for that reason.

## Next in line for the Linux ABI

1. A real-time signal range with `sigqueue` and the queued values.
2. `sigaltstack` and `SA_ONSTACK` so a handler can run on its own stack.
3. The stopped-child wait status, so a parent can observe `WIFSTOPPED`.
4. The floating-point state in the ucontext, so a handler sees the registers.
5. Threads: `CLONE_VM`, `CLONE_THREAD`, per-thread `fs` base and `set_tid_address`.
6. Copy-on-write for `fork`, which removes the deep copy of the private slots.
7. `futex` waiting and waking, which the threading work depends on.
