# Milestone 28: signal ABI foundation

The kernel now accepts the Linux signal syscall surface: `kill`, `rt_sigaction`, `rt_sigprocmask` and `rt_sigreturn`. Per-process pending and blocked signal masks live in `kernel/signal.rs` alongside the handler table. `Process` carries the mask fields so `fork` and `execve` can preserve or reset them.

## Signal syscalls

- `kill(pid, sig)` delivers a signal to a task; `SIGKILL` and `SIGSTOP` are never blocked.
- `rt_sigaction(sig, act, oldact)` installs or queries a handler with `SIG_DFL`/`SIG_IGN` and flag support.
- `rt_sigprocmask(how, set, oldset)` blocks, unblocks or replaces the mask.
- `rt_sigreturn` is accepted for handler return trampolines.

## Delivery path

`signal_pending_check` runs before `SYSRET` in `arch/x86_64/syscall.S`. When a signal is pending the kernel builds a sigframe on the user stack and enters the handler through `signal_deliver_asm`. Default-action signals (`SIGPIPE`, `SIGKILL`, `SIGTERM`) terminate the process when no handler is installed.

## Deliberate limits

No `SA_SIGINFO` frames, no `signalfd`, no `sigaltstack`, no real-time signals (`SIGRTMIN`+), and `rt_sigreturn` does not yet restore the full saved context. Returning `ENOSYS` or `EINVAL` remains preferable to fabricating behavior.
