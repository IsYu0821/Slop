# Milestone 37: Linux ABI compatibility - paths, process and sessions

Path resolution is now cwd-aware: `getcwd`, `chdir`, `fchdir`, relative paths
in `openat`, `stat`, `unlink`, `execve`, `readlinkat` and the new `mkdir`,
`rmdir`, `rename`, `chmod`, `truncate` syscalls resolve `.` and `..` against
the process cwd. The VFS gained `rename`, `rmdir`, `chmod`, and `truncate`
operations. Process state now carries cwd and a prctl name.

Added syscalls: `stat`, `lstat`, `unlink`, `dup`, `gettimeofday`, `getrlimit`,
`setrlimit`, `getrusage`, `times`, `select`, `pselect6`, `clock_getres`,
`clock_nanosleep`, `utimensat`, `mkdirat`, `renameat`, `fchmodat`, `setpgid`,
`getpgid`, `setsid`, `getsid`, `prctl`, `sysinfo`, `msync`, `mlock`,
`munlock`, `mlockall`, `munlockall`. `mmap` now supports read-only file-backed
mappings in addition to anonymous mappings.

libc gained matching wrappers and `struct timeval`, `struct rlimit`.

Acceptance suite remains green.
