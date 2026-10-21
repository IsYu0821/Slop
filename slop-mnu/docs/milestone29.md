# Milestone 29: signal delivery and a correct syscall return

Two defects blocked the Linux userspace ABI. Both are fixed here.

## The syscall return path

`arch/x86_64/syscall.S` returned to user space with `sysretq`. `SYSRET` loads
RFLAGS from RAX, and RAX holds the syscall result, so every syscall replaced
the user flags with the return value. A return value with bit 1 clear also
raised `#GP`, and the cleared interrupt flag stopped the timer from waking the
process again.

The return now uses `iretq`, the same mechanism the kernel already uses for the
first user entry and for a fork resume. `IRETQ` restores RIP, CS, RFLAGS, RSP
and SS and leaves RAX alone, so the syscall result and the user flags survive
at the same time. The saved user RFLAGS live in the frame slot that the
`SYSCALL` instruction filled with R11.

## The syscall entry state

The user stack pointer and the register frame of the running syscall lived in
two global words. A task that blocked inside a syscall, for example in
`wait4` or in a pipe read, therefore resumed with the stack pointer of the
task that ran in between, and the return to user space used the wrong stack.
Both words now belong to the task: the scheduler stores them with the task it
switches away from and restores them with the task it switches to.

## Signal delivery

`kernel/signal.rs` builds the published Linux x86-64 `rt_sigframe` on the user
stack:

- the restorer address occupies the lowest slot, so a handler that returns with
  `ret` lands in the restorer;
- the `ucontext_t` follows, with the machine context in the published `REG_*`
  register order;
- the `siginfo_t` closes the frame.

The handler is entered with the signal number in RDI, the siginfo pointer in
RSI and the ucontext pointer in RDX. `rt_sigreturn` reads the frame back,
restores the registers, the flags, the stack pointer and the signal mask, and
returns to the interrupted code.

Signal behaviour now follows the Linux rules:

- `rt_sigaction` uses the `k_sigaction` layout (handler, flags, mask,
  restorer) and rejects a handler without `SA_RESTORER`;
- `rt_sigprocmask` reads the mask from the user pointer instead of treating
  the pointer as the mask;
- `SA_SIGINFO` delivers the siginfo and the ucontext, `SA_NODEFER` keeps the
  signal unblocked, `SA_RESETHAND` resets the action after the delivery;
- `SIG_IGN` discards the signal, `SIGCHLD` and `SIGCONT` are ignored by
  default, `SIGSTOP` and `SIGTSTP` stop the task and every other signal
  terminates it;
- `kill` reaches the requested process or process group instead of the caller;
- a blocked signal stays pending and `rt_sigpending` reports the pending set;
- a blocking `wait4` or pipe read reports `EINTR` when a signal is queued.

`fork` clears the pending set of the child and `execve` clears the pending set
and the mask, like Linux does.

## Acceptance

`/bin/hello-signal` installs a `SA_SIGINFO` handler, takes a signal from a
child process, checks the siginfo and the ucontext, verifies that an ignored
signal is discarded, that a blocked signal stays pending until the mask is
cleared, that the default action of `SIGKILL` kills a child and that a stopped
child continues after `SIGCONT`.

`tools/check-abi.py` checks the published layouts of `k_sigaction`,
`sigcontext`, `ucontext_t` and `siginfo_t` and the `REG_*` index list against
the kernel structures and the libc header. The gate needs no compiler, so a
layout drift is caught before a build.

## Deliberate limits

Signals are delivered in signal number order, not in arrival order. There is
no `sigaltstack`, no real-time signal, no `SA_ONSTACK`, no `sigqueue`, no
`signalfd` and no `WUNTRACED` reporting for a stopped child. The kernel does
not save the floating-point state, so the `ucontext` reports a zeroed area.
