# Linux compatibility target

The first executable target is a statically linked x86-64 Linux program using a small libc startup path. Compatibility is measured at the raw syscall boundary, ELF process image, initial stack and userspace-visible device interfaces.

## Implemented bootstrap surface

`read`, `write`, `close`, `fstat`, `openat`, `newfstatat`, `lseek`, `mmap`, `mprotect`, `munmap`, `brk`, `sched_yield`, `clock_gettime`, `nanosleep`, `getrandom`, identity calls, PID/TID calls, `uname`, `arch_prctl` FS-base state, `set_tid_address`, `set_robust_list`, `prlimit64`, `execve`, `exit`, `exit_group`, `clone` (fork subset), `wait4`, `pipe`, `pipe2`, `dup2`, `dup3`, `fcntl` (minimal), `kill`, `rt_sigaction`, `rt_sigprocmask`, `rt_sigpending`, `rt_sigreturn`.

## Signal behaviour

Signals are delivered on the syscall return path. The handler frame follows the published Linux x86-64 `rt_sigframe` layout: the restorer address, then the `ucontext_t` with the machine context in the `REG_*` order, then the `siginfo_t`. `SA_SIGINFO` passes the siginfo and the ucontext, `SA_NODEFER` keeps the signal unblocked during the handler, `SA_RESETHAND` resets the action and `SA_RESTORER` is required for every installed handler. A blocked signal stays pending, `rt_sigpending` reports the pending set and `SIGCONT` resumes a stopped task. A blocking `wait4` or pipe read reports `EINTR` when a signal is queued.

Job control uses a `Stopped` scheduler state: `SIGSTOP` (unblockable, uncatchable) and an unblocked `SIGTSTP`/`SIGTTIN`/`SIGTTOU` with the default disposition park the task; the parent observes `WIFSTOPPED` with `WUNTRACED` and `WIFCONTINUED` with `WCONTINUED`, or `CLD_STOPPED`/`CLD_CONTINUED` through `waitid`, plus `SIGCHLD` unless `SA_NOCLDSTOP`. `SIGCONT` resumes a stopped task and is consumed with the default disposition.

The `ucontext_t` carries the 512-byte FXSAVE area with the SSE registers at offset 160, `MXCSR` at its reset value and `fpregs` pointing at the area; user SSE registers survive syscalls, signals and context switches.

## File plumbing

`copy_file_range` (files only), `splice` (one side a pipe), `tee`
(pipe to pipe, non-consuming) and `fallocate` (extend, `KEEP_SIZE`,
`PUNCH_HOLE`) move file content with native offset, flag and error
semantics. Page-backed files (tmpfs and the initramfs root) address
8 direct pages plus single- and double-indirect tables, up to ~1 GiB;
holes read as zeros and `ENOSPC`/`EFBIG` only fire on a genuinely
exhausted frame allocator or past the maximum.

## Dirfd-relative lookup

Every `*at` syscall (`openat`, `mkdirat`, `unlinkat`, `renameat`,
`renameat2`, `linkat`, `symlinkat`, `readlinkat`, `newfstatat`,
`faccessat`, `fchmodat`, `fchownat`, `statx`, `utimensat`,
`execveat`) resolves relative paths against directory fds with native
errors (`ENOTDIR`, `EBADF`) and flag handling (`AT_SYMLINK_NOFOLLOW`,
`AT_REMOVEDIR`, `AT_EMPTY_PATH`, `RENAME_NOREPLACE`).
`execveat(AT_EMPTY_PATH)` executes the open file. Dirfd paths are
snapshots taken at open. `mknodat` stays `ENOSYS`.

## On-disk links and renames

ext2 supports `link` (sharing inodes, `nlink` accounted, symlinks
followed), `rename` (file and empty-directory replacement with Linux
errors, `..` and parent counts fixed on moves) and symlinks (fast
inline targets up to 60 bytes, block-backed beyond, fully traversed
by path resolution and `readlink`). `stat` reports the real link
count. `mknodat` stays `ENOSYS`.

## signalfd

`signalfd`/`signalfd4` deliver masked signals as 128-byte `signalfd_siginfo` structs sharing the process pending bit with handlers and `sigwaitinfo`/`sigtimedwait`: consuming via one path consumes the shared pending state but never a child task state, and `wait4`/`waitid` never consume the pending bit. Child stop/continue/exit arrive as `SIGCHLD` with `CLD_STOPPED=5`/`CLD_CONTINUED=6`/`CLD_EXITED=1`/`CLD_KILLED=2`, the child pid/uid and the stop signal / `SIGCONT` / exit code / termsig. `SA_NOCLDSTOP` suppresses only stop/continue generation, never waiting. `poll`/`select`/`epoll` report signalfd readability exactly when a masked signal is pending. No core dumps: signal deaths report `CLD_KILLED`, never `CLD_DUMPED`.

## Network servers

`AF_INET` TCP supports passive open: `bind` (with `EADDRINUSE` conflicts),
`listen` (auto-bind when unbound) and `accept` with peer `sockaddr_in`.
`127.0.0.1` and the guest's own address loop back locally, so forked
servers and clients rendezvous inside the guest. UDP loopback works the
same way. External inbound connections still need `hostfwd` plumbing.

## Local datagrams

`AF_UNIX` `SOCK_DGRAM` supports `bind` (`EADDRINUSE` conflicts,
`EINVAL` on rebind), unconnected `sendto`/`recvfrom` with sender
reporting (empty address for unbound senders, `ENOENT` for unknown
paths), connected `send`/`recv` with `getpeername`, message-boundary
preservation, abstract names (leading NUL) and `socketpair` pairs.
`listen`/`accept` on datagrams report `EOPNOTSUPP`. Payloads cap at
4 KiB per message (8 queued per socket); larger datagrams report
`EMSGSIZE`. No `SCM_RIGHTS`/`SCM_CREDENTIALS` yet.

## In-tree C runtime

`libc/` is a clean-room static C runtime for this ABI surface: freestanding startup, a syscall shim, string functions, a `printf` subset, a first-fit allocator over anonymous mappings, `getenv`, `errno` via `__errno_location`, and file, process, pipe, time and signal wrappers. Errors are reported through `-1` plus `errno` like POSIX libc, on top of raw syscall returns. `/bin/hello-c`, `/bin/hello-posix` and `/bin/hello-signal` are the acceptance programs built with it.

## Deliberately not claimed yet

`PIDFD_GETFD`, `MKNODAT` and `USERFAULTFD` remain `ENOSYS`, full SMP bring-up, IOMMU, NVMe, AHCI, xHCI/HID, DRM and ALSA families, complete namespaces/cgroups/bpf interplay, and the remaining driver-portability factory. Returning `ENOSYS` is preferable to silently violating Linux semantics.

## Acceptance gate

A release only advances to "runs Linux binaries" when the same statically linked test ELF produces matching stdout, exit status, syscall results and errno values on Linux and Slop/MNU under an automated differential harness.
