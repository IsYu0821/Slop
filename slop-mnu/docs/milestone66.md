# Milestone 66: the published Linux signal ABI and a closer Linux userspace

## Goal

Raise Linux compatibility where the gap was largest: the signal ABI that every
real Linux program depends on, the descriptor limits a shell runs into, the
virtual-memory area bookkeeping and a set of smaller syscall behaviours that
differ from Linux.

## Signal delivery follows the published rt_sigframe

The kernel now builds the published x86-64 `rt_sigframe` on the user stack:

- the lowest slot holds the restorer address, so a handler that returns with
  `ret` lands in the restorer and performs `rt_sigreturn`;
- the `ucontext_t` follows, with the machine context in the published `REG_*`
  order (`gregs[REG_RIP]` is the interrupted instruction pointer);
- the `siginfo_t` closes the frame and reports the sender in the `_kill`
  member.

A handler installed with `SA_SIGINFO` receives the signal number, the siginfo
pointer and the ucontext pointer, exactly like Linux. The `ucontext_t` layout
matches the published one: flags, link, stack, the machine context (23
registers, the floating-point pointer and the reserved words), the signal
mask and the floating-point area.

The kernel also enforces the parts of the ABI that programs rely on:

- `rt_sigaction` uses the Linux `k_sigaction` field order
  (handler, flags, restorer, mask);
- the delivered signal leaves the pending set, so a handler never sees it
  twice;
- the pre-handler mask is restored when the handler returns;
- `SA_RESETHAND`, `SA_NODEFER` and `SA_SIGINFO` behave like Linux;
- a blocking `wait4` or `waitid` reports `EINTR` when a signal with a handler
  arrives, while a signal with the default "ignore" disposition (such as
  `SIGCHLD`) does not interrupt the wait;
- a fatal signal that the caller sends to itself ends the caller before the
  syscall returns, but only when the default disposition is in effect.

## The syscall return restores the user flags

`SYSRET` loads RFLAGS from RAX, so the previous return path replaced the user
flags with the syscall result. The return now uses `IRETQ`, which restores
RIP, CS, RFLAGS, RSP and SS and leaves RAX alone. The user flags and the
syscall result both survive the call, and the signal delivery rewrites the
same register frame, so a handler is entered with the published argument
registers.

A handler that returns restores the interrupted context through
`rt_sigreturn`. The restored value reaches the user through a dedicated cell,
because the dispatcher writes the syscall result after the syscall returns.

## Descriptor limits follow Linux

A process may now hold 1024 descriptors and the system may hold 512 open file
descriptions, the values Linux uses. The previous limits (32 and 64) made
ordinary shells fail with `EMFILE` once a few pipelines were running. The
close-on-exec flags became a bitmap so that the larger table still fits in a
kernel stack.

## The virtual-memory ledger behaves like the Linux area list

- Adjacent areas with the same protection and backing are merged, and a
  released range becomes available for the next mapping, so a long-running
  process no longer exhausts the table.
- The area record stores the inode number instead of a whole inode, which
  keeps a process small enough for the kernel stack.
- `madvise` follows the Linux advice set: `MADV_DONTNEED` drops the pages,
  `MADV_FREE` keeps them, the advisory hints do nothing and an unknown advice
  reports `EINVAL`.
- `rseq` reports `ENOSYS`, so a C library that supports it takes the
  registration-free thread-local storage path instead of assuming a
  registration that never happened.

## Tools

- `tools/heap-test.sh` runs a musl program in the guest that reproduces the
  allocator setup of a C library (the program break, a guard page below the
  heap, many allocation rounds and a forked child) and checks that the heap
  survives.
- `tools/check-source.sh` verifies the new signal structures and the
  `iretq` return path.
- The firmware runner picks the first OVMF image pair that exists, so the
  acceptance gate works on distributions that only ship the 4 MiB build.

## Known issue

The BusyBox and differential batteries still fail intermittently: a musl
allocator internal assertion fires because the allocator metadata in the heap
page is overwritten. The failure is not deterministic and predates this
milestone; the analysis in `docs/priority-status.md` records what is known
about it.

## Acceptance criteria

1. `./tools/acceptance-test.sh` prints `acceptance: PASS`, including the
   signal acceptance program that checks the handler, the `siginfo`, the
   `ucontext`, the mask, the ignored signal, the default action and the
   stop/continue path.
2. `./tools/check-source.sh` passes.
3. `cargo +nightly build -Zbuild-std=core --release` is clean.
