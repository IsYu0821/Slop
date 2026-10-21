# Milestone 58: nonblocking I/O, fcntl status flags, FD_CLOEXEC, flock

## Goal
Make descriptor status flags behave like Linux: `O_NONBLOCK` on opens, pipes
and sockets, `O_APPEND` at write time, `F_GETFL`/`F_SETFL` round trips,
`F_GETFD`/`F_SETFD` close-on-exec, and `flock(2)` advisory locking. Programs
that set descriptors nonblocking and poll them (or rely on `flock`) must see
the same behavior on the host and in the guest.

## Scope
- Per-open-file status flags: `O_APPEND`, `O_NONBLOCK`, `FD_CLOEXEC`.
- `fcntl`: `F_DUPFD`, `F_DUPFD_CLOEXEC`, `F_GETFD`, `F_SETFD`, `F_GETFL`,
  `F_SETFL`, `F_GETLK` (always reports `F_UNLCK` — no POSIX record locks).
- `open`/`openat`: `O_APPEND`, `O_NONBLOCK`, `O_CLOEXEC`, `O_TRUNC`.
- `socket`/`accept4`: `SOCK_NONBLOCK`, `SOCK_CLOEXEC`.
- `dup3` with `O_CLOEXEC`; `execve` closes `FD_CLOEXEC` descriptors.
- `flock(2)`: `LOCK_SH`/`LOCK_EX`/`LOCK_NB`/`LOCK_UN`, blocking and
  nonblocking contention via the existing wait-queue machinery.
- `sendto`/`recvfrom`/`sendmsg`/`recvmsg`: `MSG_DONTWAIT`.
- Kernel stack hardening discovered while testing: 64 KiB kernel stacks and a
  syscall-entry overflow guard.

Out of scope (later milestones): POSIX record locks (`F_SETLK`/`F_SETLKW`),
`O_ASYNC`, `F_SETOWN`, file leases, `flock` across `fork` inheritance nuance
beyond per-open-file sharing.

## Data model

### `fd.rs` — open-file status flags
- `OpenFile` gains `flags: u32` (`O_APPEND|O_NONBLOCK`, `STATUS_MASK`), shared
  by reference: `dup`/`fork` copy the flags through the shared `OpenFile`, and
  `F_SETFL` on one fd is visible through all aliases, as on Linux.
- `FdTable` gains a parallel `close_exec` table; `close_on_exec()` clears both
  the slot and the cached `OpenFile` reference of `FD_CLOEXEC` descriptors.
- `flags(id)`, `flags_set(id, set, clear)`, `is_nonblock(id)` centralize flag
  updates; `F_SETFL` replaces the `O_APPEND|O_NONBLOCK` bits (set and clear in
  one call).
- `alloc_file_flags`, `open_flags`, `create_open_flags`, and `pipe2` carry the
  flag through creation.
- `write` with `O_APPEND` resolves the start offset through
  `vfs::inode_size(ino)` instead of the cached inode snapshot, and `seek`
  `SEEK_END` does the same, so appends land at the live end of file.

### `flock.rs` — advisory locks
- Fixed 32-entry table keyed by `(fd-identity, inode)`; `LOCK_SH`/`LOCK_EX`
  replace the caller's previous lock then scan for conflicts (other
  open-file's) locks, so upgrade/downgrade is not deadlock-prone.
- Contending blocking calls register on a per-entry wait queue and park; the
  holder's `release` wakes all waiters. Nonblocking (`LOCK_NB`) contention
  returns `EWOULDBLOCK`.
- Non-inode kinds (console, eventfd, ...) accept the call and report success
  without state; char/block devices use a pseudo-inode so `flock` on
  `/dev/...` nodes still works.

### `vfs.rs`
- `inode_size(ino) -> Option<u64>` looks up the live size for tmpfs and root
  inodes; used by append writes and `SEEK_END`.

### `linux_abi`
- `open_common(p, flags, mode)` is shared by `sys_open` and `sys_openat`
  (musl's `open()` issues syscall 2, so both paths must honor the full flag
  set, including `O_TRUNC` through `vfs::truncate_path`).
- `accept_impl(fd, addr, addrlen, extra)` implements `accept` and `accept4`;
  the accepted descriptor inherits the listener's nonblocking mode and applies
  the `accept4` flags (`SOCK_CLOEXEC`/`SOCK_NONBLOCK`).
- `sys_socket` honors `SOCK_NONBLOCK`/`SOCK_CLOEXEC` in the type argument.
- `sys_dup3` applies `O_CLOEXEC` to the new descriptor.
- `sys_flock` wired to the flock module.
- Read/write socket paths treat a descriptor as nonblocking when either the
  open file has `O_NONBLOCK` or the call passes `MSG_DONTWAIT`.

### `task.rs` — kernel stack hardening
- `STACK_SIZE` grows to 64 KiB. While validating the new tests, the
  fork→`execve`→`wait` path was found to overflow the forked child's 32 KiB
  kernel stack slot into the adjacent slot (the parent's), corrupting the
  parent's saved `wait4` frame. Worst-case exec-path usage measured ~28 KiB
  plus untracked deeper frames; 64 KiB restores headroom without changing the
  boot-time stack.
- `stack_guard()` runs at every syscall entry and halts with a diagnostic if
  the stack pointer leaves the task's own slot, turning silent corruption into
  a deterministic failure.

## Acceptance criteria
1. `./tools/acceptance-test.sh` prints `acceptance: PASS` with all resource
   reclamation markers intact.
2. `./tools/differential-test.sh` prints `differential test: PASS`, including
   new host/guest parity cases:
   - `DIFF_FLOCK_OK`: `LOCK_EX|LOCK_NB`/`LOCK_SH|LOCK_NB` contention reports
     `EWOULDBLOCK`, unlock releases, and a shared lock is granted after the
     exclusive holder exits.
   - `DIFF_NONBLOCK_OK`: `pipe2(O_NONBLOCK)` read returns `EAGAIN`;
     `F_GETFL` shows `O_NONBLOCK`; `F_SETFL` clears it;
     `F_GETFD`/`F_SETFD` round trip `FD_CLOEXEC`.
   - `DIFF_APPEND_OK`: `O_APPEND` writes land at end after `lseek` back.
   - `DIFF_CLOEXEC_OK`: a `FD_CLOEXEC` descriptor is gone after `execve`
     (child re-execs itself and reports `EBADF`), exercising fork → exec →
     wait end to end.
3. `./tools/check-source.sh` passes.

## Verification
- `cargo +nightly build -Zbuild-std=core --release` clean.
- `OVMF_CODE=... OVMF_VARS_TEMPLATE=... ./tools/acceptance-test.sh` PASS.
- `./tools/differential-test.sh` PASS (native Linux vs QEMU guest).
- `./tools/check-source.sh` pass.

## Risks
- `flock` is a fixed 32-entry table; exhaustion returns `ENOMEM`.
- `F_GETLK` reports `F_UNLCK` unconditionally: no POSIX record locks exist, so
  programs mixing `fcntl` locks with `flock` see locks they set through
  `fcntl` as absent. Acceptable until record locks are implemented.
- `F_SETFL` only tracks `O_APPEND`/`O_NONBLOCK`; other status bits are
  ignored rather than rejected.
- `F_GETFL` reports `O_RDWR` as the access mode regardless of how the
  descriptor was opened; musl callers mask the access mode, so this is only
  visible through raw `fcntl`.
- 64 KiB per task × 8 tasks grows the static kernel BSS by 256 KiB; the EFI
  loader image stays well within the QEMU memory budget.
