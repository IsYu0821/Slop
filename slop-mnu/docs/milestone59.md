# Milestone 59: POSIX timers, clock_settime and robust-list round trip

## Goal
Give processes real interval timers and a settable wall clock: the
`timer_create` family delivering signals with `si_value`/`si_overrun`,
`alarm`/`setitimer` sharing the same per-process ITIMER_REAL, a
`CLOCK_REALTIME` seeded from the platform RTC and adjustable with
`clock_settime`, and `set/get_robust_list` round-tripping. While wiring the
timer tick, the timer IRQ stub's stack alignment was found to be
statically assumed; it is now dynamic.

## Scope
- `timer_create` (SIGEV_SIGNAL, SIGEV_NONE, SIGEV_THREAD_ID),
  `timer_settime` (incl. `TIMER_ABSTIME`), `timer_gettime`,
  `timer_getoverrun`, `timer_delete`.
- `alarm`, `getitimer`/`setitimer` for `ITIMER_REAL` (shared per-process
  timer; `ITIMER_VIRTUAL`/`ITIMER_PROF` report `EINVAL` — no CPU-time
  accounting yet).
- `clock_settime(CLOCK_REALTIME)` and truthful `clock_gettime`/
  `clock_getres` classification: realtime, monotonic, boottime, CPU clocks
  as zero, coarse variants.
- `clock_nanosleep` honors the clock id and `TIMER_ABSTIME`.
- RTC seeding of `CLOCK_REALTIME` at boot (binary and BCD modes, 12h/24h).
- `set_robust_list`/`get_robust_list` storage and round trip.
- `rt_sigpending` and `rt_sigtimedwait` (zero and blocking timeouts);
  `pause` returns `EINTR` for deliverable signals.
- Timer IRQ entry: dynamic 16-byte stack alignment before the Rust handler.

Out of scope: robust-futex `EOWNERDEAD`/`FUTEX_OWNER_DIED` processing on
thread exit (round trip only), `SIGEV_THREAD` at the kernel level (musl
translates it to `SIGEV_THREAD_ID|SIGEV_SIGNAL` in userspace),
`ITIMER_VIRTUAL`/`ITIMER_PROF`, CPU-time clocks, POSIX record locks.

## Data model

### `posix_timer.rs` — interval timers and alarms
- 32-entry POSIX timer table keyed by a monotonic id, plus an 8-entry
  per-PID alarm slot table (one live process each). Both are plain
  `SpinLock`s; the tick hook only mutates the tables under their locks and
  queues signals after dropping them, so no scheduler lock is taken while
  the timer table is held.
- `Notify::Signal{sig,value}` / `Notify::None` model `sigevent`.
  `SIGEV_THREAD` (2) is rejected as on Linux; `SIGEV_THREAD_ID` (4) is
  accepted and delivered process-wide (no tid targeting).
- Expiry accounting: a tick that finds `next <= now` computes the full
  number of missed expirations, folds one delivery into a signal and
  records the rest as overrun. When the signal is already pending the
  count accumulates in the timer entry; `timer_getoverrun` reports it.
- `alarm_set`/`alarm_get` implement the per-process ITIMER_REAL with
  remaining/interval in ticks; `alarm()` is the seconds-only view.

### `task.rs` / `process.rs` — delivery and siginfo storage
- `timer_signal(pid, sig, value, overrun)` queues the bit under the
  process lock, records `si_code = SI_TIMER` plus value/overrun in new
  per-process arrays, and wakes the owner; it returns whether the signal
  coalesced so the timer table can account overrun.
- `Process` gains `si_code`/`si_value`/`si_overrun` arrays (reset at exec)
  and `robust_list_head`/`robust_list_len`.
- `on_timer_tick` now calls `posix_timer::tick(now)` after waking sleepers.

### `linux_abi` — syscall surface
- `rt_sigtimedwait` writes a user-layout `siginfo_t` (timer: `si_timerid`
  @16, `si_overrun` @20, `si_value` @24, matching the host kernel for both
  musl and glibc callers), returning the signal number or `EAGAIN`.
- `rt_sigpending` copies the pending mask; `pause` returns `EINTR` when a
  deliverable signal (handler or terminating default) is pending.
- `clock_gettime/getres` classify clock ids; `CLOCK_REALTIME` is
  `monotonic + offset`, the offset seeded once from the CMOS RTC and
  replaced by `clock_settime`. `clock_nanosleep` supports `TIMER_ABSTIME`.
- `execve` deletes the process's POSIX timers (POSIX: timers are not
  preserved across exec).

### `arch/x86_64` — RTC and IRQ alignment
- `rtc.rs` reads the CMOS clock: BCD/binary status, 12h PM translation,
  century register with a year-based fallback, sanity bounds, and a
  stable-read check. Failure to read leaves the offset at zero.
- `irq.S`: the timer stub aligned the stack with a fixed `sub $8`, which
  only holds when the interrupted kernel context has a particular
  `rsp % 16`. A `movaps` in the new tick code faulted with #GP on the
  first tick. The stub now saves `rsp` in `r15`, masks to 16 bytes before
  the call, and restores afterwards — the same pattern `exceptions.S`
  already used.

## Acceptance criteria
1. `./tools/acceptance-test.sh` prints `acceptance: PASS` with all resource
   reclamation markers intact.
2. `./tools/differential-test.sh` prints `differential test: PASS`,
   including new host/guest parity cases:
   - `DIFF_TIMER_OK`: one-shot timer signal arrives, `si_code == SI_TIMER`,
     `si_value` round-trips, overrun is readable, the timer disarms and
     `timer_delete` invalidates the id, unknown clocks are rejected.
   - `DIFF_ITIMER_OK`: `setitimer(ITIMER_REAL)` fires `SIGALRM`, `getitimer`
     reads zero after expiry, `alarm(5)` then `alarm(0)` returns 5.
   - `DIFF_CLOCK_OK`: realtime resolution/shape, `EINVAL` for unknown
     clocks, monotonic `clock_settime` rejected.
   - `DIFF_ROBUST_OK`: `get_robust_list` reports length 24, `set` then
     `get` round-trips the head pointer, bad length is `EINVAL`.
   - `DIFF_SIGPEND_OK`: raised blocked signal appears in `sigpending`, a
     zero-timeout `sigtimedwait` consumes it, the next one is `EAGAIN`.
3. `./tools/check-source.sh` passes.

## Verification
- `cargo +nightly build -Zbuild-std=core --release` clean.
- `OVMF_CODE=... OVMF_VARS_TEMPLATE=... ./tools/acceptance-test.sh` PASS.
- `./tools/differential-test.sh` PASS (native Linux vs QEMU guest).
- `./tools/check-source.sh` pass.

## Risks
- Signals remain a process-wide bitmask: coalesced timer signals collapse
  to one bit, so `si_value` for a later timer sharing an already-pending
  signal is not distinguishable; per-timer overrun counting compensates in
  the common single-timer case.
- The POSIX timer table is a fixed 32 entries (`EAGAIN` when exhausted).
- `ITIMER_VIRTUAL`/`ITIMER_PROF` return `EINVAL` instead of counting CPU
  time; programs that profile themselves will fail fast rather than hang.
- RTC reads assume a PC-compatible CMOS clock; invalid values leave
  `CLOCK_REALTIME` boot-relative as before.
- `CLOCK_REALTIME` advances with the monotonic timer and is not disciplined
  for drift after `clock_settime`, and there is no `adjtimex`.
- Timers are per-process (owner PID); delivery is process-directed, so a
  `SIGEV_THREAD_ID` target thread is not specifically selected.
