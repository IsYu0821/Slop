# Linux compatibility target

The first executable target is a statically linked x86-64 Linux program using a small libc startup path. Compatibility is measured at the raw syscall boundary, ELF process image, initial stack and userspace-visible device interfaces.

## Implemented bootstrap surface

`read`, `write`, `close`, `fstat`, `openat`, `newfstatat`, `lseek`, `mmap`, `mprotect`, `munmap`, `brk`, `sched_yield`, `clock_gettime`, `nanosleep`, `getrandom`, identity calls, PID/TID calls, `uname`, `arch_prctl` FS-base state, `set_tid_address`, `set_robust_list`, `prlimit64`, `execve`, `exit`, `exit_group`, `clone` (fork subset), `wait4`, `pipe`, `pipe2`, `dup2`, `dup3`, `fcntl` (minimal).

## In-tree C runtime

`libc/` is a clean-room static C runtime for this ABI surface: freestanding startup, a syscall shim, string functions, a `printf` subset, a first-fit allocator over anonymous mappings, `getenv`, `errno` via `__errno_location`, and file, process and pipe wrappers. Errors are reported through `-1` plus `errno` like POSIX libc, on top of raw syscall returns. `/bin/hello-c` and `/bin/hello-posix` are the acceptance programs built with it.

## Deliberately not claimed yet

signals beyond `SIGCHLD` bookkeeping, `SIGPIPE` delivery, threads (`CLONE_VM`/`CLONE_THREAD`), copy-on-write, futex waiting/waking, `select`/`poll`/`epoll`, sockets, path lookup completeness, and dynamic linking. Returning `ENOSYS` is preferable to silently violating Linux semantics.

## Acceptance gate

A release only advances to "runs Linux binaries" when the same statically linked test ELF produces matching stdout, exit status, syscall results and errno values on Linux and Slop/MNU under an automated differential harness.
