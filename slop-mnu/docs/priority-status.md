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
- a clock in real units: the rate of the time stamp counter is measured
  against the programmable interval timer, the length of a timer tick is
  measured against the counter, and the timer is programmed so a tick has
  the same length on every machine
- the interval timers, the processor time accounting and every other time
  conversion follow the measured length of a tick

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

## Implemented and covered by the acceptance gate

- the vector system call entry point, the page of code Linux gives every
  program, with the entry points the C library looks for, so a program
  reads the clock without entering the kernel
- a shared page that the kernel keeps up to date, read under a sequence
  lock, so the entry point can tell the time without asking the kernel
- the processor time charged to the program that spends it, like Linux
  reports it
- the stopped-child wait status: `WIFSTOPPED`/`WSTOPSIG` with `WUNTRACED`,
  `WIFCONTINUED` with `WCONTINUED`, `CLD_STOPPED`/`CLD_CONTINUED` for
  `waitid`, and `SIGCHLD` on stop and continue unless `SA_NOCLDSTOP`
- the floating-point state in the ucontext: the 512-byte FXSAVE area with
  the SSE registers at offset 160, `MXCSR` at its reset value and `fpregs`
  pointing at the area, preserved across syscalls and context switches
- signalfd child-status parity: `CLD_STOPPED`/`CLD_CONTINUED`/`CLD_EXITED`/
  `CLD_KILLED` with child pid/uid/status, `SA_NOCLDSTOP` suppressing only
  generation, first-wins coalescing shared with `sigwaitinfo` and handler
  delivery, waits never consuming the pending bit, `poll`/`select`/`epoll`
  readiness following the mask
- file plumbing: `copy_file_range`/`splice`/`tee`/`fallocate` with native
  offsets, flags and errors, backed by pipe `peek` and VFS capacity
  reservation
- TCP passive open and loopback: `listen`/`accept` with per-listener
  queues, handshake/retransmit/RST, `127.0.0.1` self-delivery for TCP
  and UDP, INET socket lifetime tied to file descriptions
- `AF_UNIX` datagrams with addresses: `bind` conflicts, unconnected
  `sendto`/`recvfrom` with source reporting, connected `send`/`recv`
  with `getpeername`, boundaries, abstract names, `socketpair` pairs
- dirfd-relative `*at`: `openat`/`mkdirat`/`unlinkat`/`renameat`(+2)/
  `linkat`/`symlinkat`/`readlinkat`/`newfstatat`/`statx`/`faccessat`/
  `fchmodat`/`fchownat`/`utimensat`/`execveat` against directory fds,
  `AT_EMPTY_PATH` fd execution, UAPI-true `statx` layout

## Open defects

- The rate of the time stamp counter is measured once at boot. A machine
  whose counter is not invariant would need the measurement repeated, and a
  machine that takes the processor away moves the counter backwards, which
  the entry point and the kernel both guard against.

## Next in line for the Linux ABI

 1. A real-time signal range with `sigqueue` and the queued values.
 3. `sigaltstack` and `SA_ONSTACK` so a handler can run on its own stack.
 4. Copy-on-write for `fork`, which is already implemented as a
    share-and-fault path and covered by `DIFF_COW_OK`.
 5. Per-CPU scheduler preparation for SMP: run queues, IPIs, per-CPU FPU
    areas, allocator sharding and TLB shootdown.
