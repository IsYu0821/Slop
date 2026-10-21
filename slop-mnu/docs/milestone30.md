# Milestone 30: complete signal delivery

Signals are no longer a bookkeeping stub: pending, blocked and handler state is
per-process, and the kernel delivers real handlers to user mode.

## Per-process signal state

`Process` carries `pending`, `sigmask`, a 64-entry handler table and the address
of the active sigframe. `Action` is `#[repr(C)]` with the Linux x86-64
`kernel_sigaction` layout (handler, flags, restorer, mask), so `rt_sigaction`
copies the user structure verbatim. `kill` resolves the target through the
scheduler and marks that task's pending mask; the previous `with_current_process`
mistake that signalled only the caller is gone.

## Delivery path

Before `SYSRET`, `syscall_entry` calls `signal_setup_frame`. The function:

1. Pops the next pending, unblocked signal. `SIG_IGN` drops it, `SIG_DFL` runs
   the default disposition (`SIGHUP`, `SIGINT`, `SIGQUIT`, `SIGILL`, `SIGABRT`,
   `SIGBUS`, `SIGFPE`, `SIGKILL`, `SIGSEGV`, `SIGPIPE`, `SIGALRM`, `SIGTERM`,
   `SIGUSR1`, `SIGUSR2` terminate; the rest are ignored), and any other handler
   number enters user mode.
2. Builds a 152-byte sigframe on the user stack: the interrupted register set,
   `rip`, `rflags` (with `IF` forced on), `rsp`, signal number, magic and the
   pre-handler mask. A restorer return slot sits *below* the sigframe so handler
   pushes grow downward without touching it.
3. Rewrites the in-flight kernel frame: `rcx` becomes the handler entry, `rdi`
   the signal number, and `SYSCALL_USER_RSP` points at the return slot.

The handler returns to `__slop_sigrestore`, which runs the `rt_sigreturn`
syscall. `signal_restore_frame` validates the magic, rewrites the kernel frame
from the sigframe and sets `SIGRET_PENDING`; the exit path then restores the
interrupted `rax` and `sysretq`s back to the interrupted point.

Blocking the delivered signal while its handler runs, `SA_RESETHAND`,
`SA_NODEFER`, and restoring the pre-handler mask on return are implemented.
`SIGCHLD` is delivered to the parent on child exit, and `SIGPIPE` is raised on
`EPIPE`.

## SysV stack alignment

`syscall_entry` runs at an 8-mod-16 stack offset after the saved frame is
reached. Calling the Rust frame rewriter without padding made its SSE spills
fault with `#GP`; the call site now pads with one `push %rax`.

## libc

`libc/signal.c` gained `__slop_sigrestore`, and `sigaction` translates the
glibc/musl user structure to the kernel layout, installing the restorer
automatically. `sys/wait.h` gained `WIFSIGNALED`/`WTERMSIG`. `/bin/hello-signal`
exercises handler installation, self-delivery, `sigprocmask` blocking and
unblocking, cross-process `kill`, and default-action termination.

## Deliberate limits

No `SA_SIGINFO` frames, no `signalfd`, no `sigaltstack`, no real-time signals,
no job-control stops (`SIGSTOP`/`SIGTSTP` are accepted but not honoured), and
`wait4` status encoding stays minimal. Nested handlers reuse the single sigframe
slot, which nested `SA_NODEFER` handlers could clobber.
