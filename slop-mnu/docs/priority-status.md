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
- the virtio 1.0 notification path, so a default `virtio-blk-pci` and
  `virtio-net-pci` device model works and a packet really leaves the guest
- a TCP stack that acknowledges its window, sends the close request and
  reaches the final state of a connection
- every user region (the program break, the anonymous mappings, the dynamic
  linker, the position independent image) in a page table slot of its own

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
- the vector register file preserved across an exception and an interrupt

## Open defects

- The differential battery stops at the processor time accounting. The
  reason is the missing vector system call entry point: on Linux a program
  that reads the clock does not enter the kernel, so the loop that measures
  the clock spends its time in user mode and the user time of the process
  grows. This kernel has no such entry point, so every clock reading is a
  system call and the time is charged to the kernel, which leaves the user
  time far below the value Linux reports. The clock itself is correct: the
  process clock, the thread clock and the wall clock agree.
- The local APIC timer is not calibrated. The kernel counts one tick as a
  millisecond, but the timer of the test machine runs at a different rate, so
  the monotonic clock and the processor time are scaled wrongly. The rate has
  to be measured against a real time source, the programmable interval timer
  is the obvious candidate.

## Next in line for the Linux ABI

1. A vector system call entry point, so a program can read the clock without
   entering the kernel. This closes the gap above and gives every Linux
   program the fast clock path it expects.
2. A calibrated local APIC timer, so the monotonic clock and the processor
   time are in real units.
3. A real-time signal range with `sigqueue` and the queued values.
4. `sigaltstack` and `SA_ONSTACK` so a handler can run on its own stack.
5. The stopped-child wait status, so a parent can observe `WIFSTOPPED`.
6. The floating-point state in the ucontext, so a handler sees the registers.
7. Copy-on-write for `fork`, which removes the deep copy of the private slots.
