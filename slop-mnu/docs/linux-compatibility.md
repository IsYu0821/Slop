# Linux compatibility target

The first executable target is a statically linked x86-64 Linux program using a small libc startup path. Compatibility is measured at the raw syscall boundary, ELF process image, initial stack and userspace-visible device interfaces.

## Implemented bootstrap surface

`read`, `write`, `close`, `fstat`, `openat`, `newfstatat`, `lseek`, `mmap`, `mprotect`, `munmap`, `brk`, `sched_yield`, `clock_gettime`, `nanosleep`, `getrandom`, identity calls, PID/TID calls, `uname`, `arch_prctl` FS-base state, `set_tid_address`, `set_robust_list`, `prlimit64`, `execve`, `exit`, `exit_group`, `clone` (fork subset), `wait4`, `pipe`, `pipe2`, `dup2`, `dup3`, `fcntl` (minimal), `kill`, `rt_sigaction`, `rt_sigprocmask`, `rt_sigpending`, `rt_sigreturn`.

## Signal behaviour

Signals are delivered on the syscall return path. The handler frame follows the published Linux x86-64 `rt_sigframe` layout: the restorer address, then the `ucontext_t` with the machine context in the `REG_*` order, then the `siginfo_t`. `SA_SIGINFO` passes the siginfo and the ucontext, `SA_NODEFER` keeps the signal unblocked during the handler, `SA_RESETHAND` resets the action and `SA_RESTORER` is required for every installed handler. A blocked signal stays pending, `rt_sigpending` reports the pending set and `SIGCONT` resumes a stopped task. A blocking `wait4` or pipe read reports `EINTR` when a signal is queued.

## In-tree C runtime

`libc/` is a clean-room static C runtime for this ABI surface: freestanding startup, a syscall shim, string functions, a `printf` subset, a first-fit allocator over anonymous mappings, `getenv`, `errno` via `__errno_location`, and file, process, pipe, time and signal wrappers. Errors are reported through `-1` plus `errno` like POSIX libc, on top of raw syscall returns. `/bin/hello-c`, `/bin/hello-posix` and `/bin/hello-signal` are the acceptance programs built with it.

## Deliberately not claimed yet

real-time signals (`SIGRTMIN`+), `sigaltstack`, `SA_ONSTACK`, `sigqueue`, `signalfd`, `sigpending` reporting for a stopped child (`WUNTRACED` wait status), signals beyond `SIGCHLD` bookkeeping, `SIGPIPE` delivery, threads (`CLONE_VM`/`CLONE_THREAD`), copy-on-write, futex waiting/waking, `select`/`poll`/`epoll`, sockets, path lookup completeness, and dynamic linking. Returning `ENOSYS` is preferable to silently violating Linux semantics.

## Acceptance gate

A release only advances to "runs Linux binaries" when the same statically linked test ELF produces matching stdout, exit status, syscall results and errno values on Linux and Slop/MNU under an automated differential harness.
