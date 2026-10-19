# Milestone 60: POSIX record locks, OFD locks and robust-futex death handling

## Goal
Make `fcntl` record locking real: byte-range locks shared across processes
with `F_GETLK`/`F_SETLK`/`F_SETLKW`, their open-file-description variants
`F_OFD_*` with per-description ownership, and the exit-side half of the
robust-mutex protocol (walk the robust list, mark `FUTEX_OWNER_DIED`, wake
the waiter). Wiring the robust test exposed that `MAP_SHARED|MAP_ANONYMOUS`
pages were being COW-split at fork; shared anon pages now stay shared.

## Scope
- `fcntl(fd, F_GETLK/F_SETLK/F_SETLKW)` — POSIX record locks owned by the
  process, released when any descriptor of that inode closes in the process,
  at process exit, and when `F_SETLK`/`F_SETLKW` replaces the owner's own
  ranges.
- `fcntl(fd, F_OFD_GETLK/F_OFD_SETLK/F_OFD_SETLKW)` — locks owned by the
  open-file description: `dup`/`fork` share the owner, an independent
  `open(2)` contends in the same process, release happens at the last close.
- `set_robust_list` data is now acted on at exit: robust walks bounded,
  `FUTEX_OWNER_DIED` set preserving `FUTEX_WAITERS`, one waiter woken only
  when `FUTEX_WAITERS` was observed (Linux semantics), `list_op_pending`
  included, walk aborts on the first unreadable word.
- `MAP_SHARED|MAP_ANONYMOUS` areas stay physically shared (writable) across
  fork instead of being made COW.
- Blocking `F_SETLKW` returns `EINTR` for deliverable signals; `pause()`
  now shares that signal check.

Out of scope: deadlock detection (`F_SETLKW` never returns `EDEADLK`),
mandatory locking, `F_SETOWN`-style lock signals, `FUTEX_LOCK_PI`/priority
inheritance, real-time lock fairness ordering.

## Data model

### `record_lock.rs` — one table for both lock families
- Up to 64 entries `{ino, owner, exclusive, [start, end)}`; `owner` is
  `Pid(pid)` for POSIX locks or `Ofd(open-file id)` for `F_OFD_*`.
  `end == u64::MAX` represents end-of-file; `l_len == 0` maps to it.
- `F_SETLK` clears the owner's overlapping ranges first (trimming or
  splitting into two entries when the request lies strictly inside an
  existing one) and then places the new range; `l_len < 0` locks the
  backward range `[start+len, start)`.
- `F_GETLK` reports the first conflicting other-owner range with
  `l_whence = SEEK_SET` and the owner pid (0 for OFD locks, matching Linux)
  or `F_UNLCK` when no conflict exists.
- Non-blocking conflicts return `EAGAIN`; the table is fixed size and
  reports `ENOLCK` when full.
- One global wait queue serves `F_SETLKW`; any release wakes it and each
  waiter re-checks. A signal that becomes deliverable while waiting turns
  into `EINTR`.

### `fd.rs` — close semantics
- `release` (allocation unwind) stays silent, but real closes now go
  through `release_closed`: every close of an inode drops that process's
  POSIX locks on it (including implicit `CLOEXEC` closes at execve and
  `dup2` replacement), while OFD locks fall only when the description's
  last reference goes. `close_all` stays plain; `teardown_exit` calls
  `record_lock::release_pid` explicitly, so lock release does not depend
  on which context eventually drops the `Process`.

### `robust.rs` — exit-time protocol
- Walks `struct robust_list_head { list, futex_offset, list_op_pending }`
  in the exiting task's address space before teardown (the address space
  is still active), marks words whose low 30 bits equal the exiting pid
  with `OWNER_DIED`, and wakes one waiter only when `WAITERS` was set,
  exactly as Linux `handle_futex_death` does. The walk stops at the first
  unreadable word, so a corrupted list cannot loop or fault in kernel.

### `vm.rs` / `process.rs` — shared anonymous mappings
- `VmFlags::SHARED` marks `MAP_SHARED|MAP_ANON` areas at `mmap` time; after
  the fork clone makes every leaf read-only, the shared ranges are put
  back to writable on both sides, so the frames stay shared with no COW.

## Acceptance criteria
1. `./tools/acceptance-test.sh` prints `acceptance: PASS` with resource
   accounting intact.
2. `./tools/differential-test.sh` prints `differential test: PASS`,
   including new host/guest parity cases:
   - `DIFF_RECORDLOCK_OK`: cross-process `EAGAIN`, `F_GETLK` reporting
     (`l_start`/`l_len`/`l_pid`), `F_SETLKW` blocked until the owner
     unlocks, release on child exit, release on closing any descriptor.
   - `DIFF_OFDLOCK_OK`: two descriptions conflict in one process,
     `F_OFD_GETLK` reports owner pid 0, `dup` shares the owner, the lock
     survives closing one descriptor, dies at the last close.
   - `DIFF_ROBUSTFUTEX_OK`: a waiter with `FUTEX_WAITERS` armed is woken
     when the owner exits, the word carries `FUTEX_OWNER_DIED` with the
     TID bits cleared.
3. `./tools/check-source.sh` passes.

## Verification
- `cargo +nightly build -Zbuild-std=core --release` clean.
- Native (musl) smoke of `tests/differential.c` passes.
- `./tools/differential-test.sh` PASS, `./tools/acceptance-test.sh` PASS,
  `./tools/check-source.sh` pass.

## Risks
- Threads created with `clone` are modeled as separate tasks, so POSIX
  record locks are owned per task rather than per process; single-threaded
  musl usage (and the tests) is unaffected.
- Shared anonymous mappings only stay shared for pages that existed at
  fork time; a page first touched after fork diverges (no shared anon
  object/anon_vma equivalent yet).
- The lock table holds 64 entries (shared by both lock families); heavy
  lockers see `ENOLCK` rather than allocation.
- `F_SETLK` replaces the owner's overlapping ranges with no merge of
  adjacent same-type ranges, so many small requests can exhaust the table.
- Blocking waits use one global queue, so every release wakes all waiters
  for a re-check (bounded by the 8-task scheduler).
- No deadlock detection: a circular `F_SETLKW` chain sleeps instead of
  returning `EDEADLK`.
- Robust death handling covers the non-PI protocol only; `FUTEX_LOCK_PI`
  remains unsupported, so PI mutexes cannot be created in the first place.
