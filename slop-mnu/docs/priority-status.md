# One-Linux-program priority status

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

## Next in line for the Linux ABI

1. A real-time signal range with `sigqueue` and the queued values.
2. `sigaltstack` and `SA_ONSTACK` so a handler can run on its own stack.
3. The stopped-child wait status, so a parent can observe `WIFSTOPPED`.
4. The floating-point state in the ucontext, so a handler sees the registers.
5. Threads: `CLONE_VM`, `CLONE_THREAD`, per-thread `fs` base and `set_tid_address`.
6. Copy-on-write for `fork`, which removes the deep copy of the private slots.
7. `futex` waiting and waking, which the threading work depends on.
