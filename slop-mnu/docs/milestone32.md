# Milestone 32: Linux syscall surface completion

The syscall surface used by standalone Linux binaries is now largely covered.
This milestone adds the calls a musl-linked static program reaches for at
startup or during normal I/O, plus the ones that were previously stubbed or
missing.

## New syscalls

- `statx` — renders the 128-byte `struct statx` from a path or, with
  `AT_EMPTY_PATH`, from a descriptor.
- `mremap` — grows or shrinks an anonymous mapping: allocate the destination,
  copy the live bytes through user mappings, release the source. `MREMAP_MAYMOVE`
  is the only accepted flag.
- `waitid` — generalises `wait4` over `P_PID`/`P_PGID`/`P_ALL` selectors and
  fills a `siginfo` record with the signal number, code, pid and status.
- `readlinkat` — resolves `/proc/self/exe` to the image path recorded by the
  last `execve`.
- `faccessat`/`faccessat2`/`access` — existence and permission probing against
  the VFS.
- `execveat` — delegates to `execve`; `AT_EMPTY_PATH` returns `ENOSYS`.
- `sched_getaffinity` — reports the single online CPU.
- `poll`/`ppoll` — readiness over the descriptor kinds the kernel has, with a
  timeout that sleeps and reports zero ready rather than blocking forever.
- `pread64`/`pwrite64` — positional I/O that ignores the file offset, backed by
  new `read_off`/`write_off` VFS entry points that resolve the node by inode
  number so a grown size is visible.
- `clone3` — treats every request as a fork; the `CLONE_VM`/thread families are
  rejected with `EINVAL`.
- `rt_sigsuspend` — installs the mask and yields.
- `madvise`, `membarrier`, `rseq`, `pkey_alloc`, `sigaltstack` — accepted no-ops
  that return success, so libraries that probe for them at startup proceed.
- `getdents` — delegates to `getdents64`.

## readv/writev

Both were capped at eight 256-byte chunks. They now walk every vector in the
caller's array with 512-byte chunks, accumulating the total the way Linux does.

## libc

Added `struct iovec` with `readv`/`writev`, `pread64`/`pwrite64`, `access`,
`faccessat`, `readlinkat`, `mremap`, `madvise`, `membarrier`, `munmap`,
`mprotect` declarations, `strstr`, and the `MADV_NORMAL` constant. All wrappers
funnel failures through `sys_fail` so `errno` is set.

`/bin/hello-syscall` exercises the new surface: a `writev`/`readv` pipe round
trip, `pread64`/`pwrite64` with offset independence, `access` on present and
absent paths, `readlinkat` on `/proc/self/exe`, an `mremap` growth that
preserves contents, and the accepted no-ops.

## Deliberate limits

`poll` reports readiness conservatively and does not distinguish `POLLHUP`;
`epoll` accepts registration but always reports zero events; `sigaltstack` does
not install an alternate stack; `execveat` has no fd-based path; and `clone3`
does not implement `CLONE_VM`, `CLONE_THREAD` or `CLONE_SETTLS`.
