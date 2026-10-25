# Milestone 74: signalfd reports stop, continue and exit like Linux

## Goal

`signalfd` must observe child stop, continue and exit events with the same
`signalfd_siginfo` contents Linux reports, sharing the pending-signal state
with handlers, `sigwaitinfo`/`sigtimedwait` and `wait4`/`waitid` without
consuming child states early or suppressing waits.

## Native behavior observed

Two small native probes fixed the semantics before any kernel code changed:

- Stop: `ssi_signo=17`, `ssi_errno=0`, `ssi_code=5` (`CLD_STOPPED`),
  `ssi_pid=child`, `ssi_uid=child uid`, `ssi_status=19` (`SIGSTOP`).
- Continue: `code=6` (`CLD_CONTINUED`), `status=18` (`SIGCONT`), same pid.
- Exit: `code=1` (`CLD_EXITED`), `status=exit code`.
- Signal death: `code=2` (`CLD_KILLED`), `status=termsig`.
- `SA_NOCLDSTOP` (even with a `SIG_DFL` handler) suppresses the
  stop/continue `SIGCHLD` on signalfd, but `waitpid` with `WUNTRACED` /
  `WCONTINUED` still reports the states, and the exit event still arrives.
- Consuming one standard `SIGCHLD` consumes the shared pending bit:
  after a signalfd read, `sigtimedwait` reports `EAGAIN`, and after
  `waitpid` reaps the child the pending `SIGCHLD` is still readable.
  Reading signalfd never reaps the child.
- Coalescing is first-wins on one bit: stop immediately followed by
  continue yields a single `CLD_STOPPED` read (the continue is lost);
  two exits before any read yield a single `CLD_EXITED` read.
  `waitid` uses `CLD_STOPPED=5` / `CLD_CONTINUED=6` (the old code wrote 2
  for stopped); a signaled child reports `CLD_KILLED`, not `CLD_EXITED`.
- Read semantics: short reads (`<128` bytes) fail `EINVAL`, an empty
  nonblocking read fails `EAGAIN`, a blocking read waits, and a 256-byte
  read returns exactly one struct.

## Design

- Per-process `sch_code/sch_pid/sch_uid/sch_status` hold the one coalesced
  child-status `SIGCHLD`, next to the existing `pending` bit. `store_sigchld`
  fills the slot only when no `SIGCHLD` is pending (first wins) and never
  touches `Stopped`/`cont_pending`/`Zombie`, so `wait4`/`waitid` still reap
  afterwards; waits never clear the pending bit, so signalfd still reports
  after a wait. `pop`, signalfd and `sigtimedwait` consumption all reset
  the slot.
- Generation: `finish` stores `CLD_EXITED` (status `code&0xff`) or
  `CLD_KILLED` (status termsig; no core dumps); stop stores `CLD_STOPPED`
  with the stop signal; continue stores `CLD_CONTINUED` with `SIGCONT`;
  a manual `kill(pid,SIGCHLD)` stores `SI_USER`. Stop/continue check
  `SA_NOCLDSTOP` on the parent's `SIGCHLD` flags regardless of handler
  vs `DFL` (the old check wrongly required a non-default handler).
- signalfd `read` builds the full 128-byte Linux layout
  (`ssi_signo@0 errno@4 code@8 pid@12 uid@16 status@40 int@44 ptr@48`,
  rest zeroed), consumes one pending signal in the fd mask (dequeueing one
  RT instance when more remain), loops in `block_current` when empty and
  blocking, returns `EAGAIN` when nonblocking and `EINTR` when an
  out-of-mask handled signal is deliverable. `sys_read`'s loop makes a
  multi-struct buffer return one struct per queued event.
- `sys_signalfd4` now dereferences the user mask (`sizemask==8`), honors
  `SFD_NONBLOCK`/`SFD_CLOEXEC`, and reprograms the mask for an existing
  fd. `poll`/`select`/`epoll` report a signalfd readable exactly when
  `pending & mask != 0`. `sys_poll` now rechecks until the timeout
  instead of sleeping once and returning 0 (an event arriving mid-wait,
  e.g. a child stop, was previously lost).
- `sigtimedwait` shares the same bit and returns the stored child fields
  for `SIGCHLD`; `waitid` reports `CLD_STOPPED=5`, `CLD_KILLED` for
  signaled children, and accepts `WCONTINUED=8`.

## Consumption rules

- Child-state events live in task states; `SIGCHLD` pending lives in the
  process bit plus the `sch_*` slot. The two are independent.
- signalfd read consumes pending + `sch_*` (or one RT instance), never a
  task state. Handler delivery (`pop`) consumes the same. `sigtimedwait`
  consumes the same. `wait4`/`waitid` consume only task states
  (`Zombie` on reap, `cont_pending` on continued report; `Stopped` stays
  reportable).
- Therefore: signalfd-then-wait and wait-then-signalfd both observe the
  full sequence, and `SA_NOCLDSTOP` suppresses only generation, never
  waiting.

## Tests added

- `DIFF_SIGNALFD_STOP_OK`: block `SIGCHLD`, signalfd, fork, child
  `raise(SIGSTOP)`; poll+read expects `code=5 pid=child status=19`;
  `waitpid/WUNTRACED` expects stopped; `SIGCONT`; poll+read expects
  `code=6 status=18`; `waitpid/WCONTINUED`; `waitpid` exit 43; poll+read
  expects `code=1 status=43`. Also covers `SFD_NONBLOCK` empty `EAGAIN`
  and short-read `EINVAL`.
- `DIFF_SIGNALFD_NOCLDSTOP_OK`: `SA_NOCLDSTOP` with `SIG_DFL`; child
  stops; bounded `WNOHANG` waits confirm stopped/continued/exit while
  nonblocking signalfd reads stay `EAGAIN` for stop/continue; the exit
  still arrives with `code=1 status=7`. Bounded retry loops replace
  fixed sleeps so a dilated guest clock cannot turn an early exit into a
  false leak.
- Existing `DIFF_STOP_OK` / `DIFF_FP_OK` kept and still required in both
  the native and guest marker lists.

## Verification

- `cargo +nightly build -Zbuild-std=core --release` clean.
- `./tools/acceptance-test.sh` PASS.
- `./tools/differential-test.sh` PASS three consecutive runs, guest and
  native logs contain `DIFF_SIGNALFD_STOP_OK` and
  `DIFF_SIGNALFD_NOCLDSTOP_OK`.
- `./tools/busybox-test.sh` PASS (115 checks match Linux; one run needed
  the harness's documented once-only retry for its regex-allocator heap
  flake).
- `./tools/heap-test.sh` PASS.
- `./tools/check-source.sh` pass.

## Known limits

- No core dumps: signal deaths always report `CLD_KILLED`, never
  `CLD_DUMPED`.
- `SIGCHLD` details for manually sent (non-child) signals carry sender
  pid with uid 0 and no queue beyond first-wins coalescing.
- `sigtimedwait`/`sigwaitinfo` mask handling still excludes `SIGKILL` /
  `SIGSTOP` as before; out-of-mask handled signals interrupt a blocking
  signalfd read with `EINTR`.
- signalfd mask reads only the first 8 bytes (`NSIG/8`); larger masks are
  rejected.
- Single CPU only: no SMP wakeups beyond the global scheduler.

## Next: SMP preparation

- Per-CPU run queues and IPIs instead of the single global `SCHED` lock.
- Per-CPU `CURRENT_FPU` and syscall-entry save areas.
- Frame-allocator sharding and TLB-shootdown protocol.
- Timer-tick accounting with per-CPU ticks.
- `signalfd_readable` and store/wake paths audited for per-CPU queues.
