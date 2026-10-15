# Linux compatibility target

The first executable target is a statically linked x86-64 Linux program using a small libc startup path. Compatibility is measured at the raw syscall boundary, ELF process image, initial stack and userspace-visible device interfaces.

## Implemented bootstrap surface

`read`, `write`, `close`, `fstat`, `openat`, `newfstatat`, `lseek`, `mmap`, `mprotect`, `munmap`, `brk`, `sched_yield`, `clock_gettime`, `nanosleep`, `getrandom`, identity calls, PID/TID calls, `uname`, `arch_prctl` FS-base state, `set_tid_address`, `set_robust_list`, `prlimit64`, `execve`, `exit`, and `exit_group`.

## Deliberately not claimed yet

signals, futex waiting/waking, path lookup completeness, dynamic linking, fork/clone and sockets. Returning `ENOSYS` is preferable to silently violating Linux semantics.

## Acceptance gate

A release only advances to "runs Linux binaries" when the same statically linked test ELF produces matching stdout, exit status, syscall results and errno values on Linux and Slop/MNU under an automated differential harness.
