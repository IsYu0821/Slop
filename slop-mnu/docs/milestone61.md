# Milestone 61: CPU-time accounting, CPU clocks and virtual/profiling timers

## Goal
Account the CPU time each process actually consumes, expose it through the
CPU clock ids (`CLOCK_PROCESS_CPUTIME_ID`/`CLOCK_THREAD_CPUTIME_ID`),
`getrusage`, `times()` and `/proc/<pid>/stat`, report the usage of reaped
children through `wait4(…, rusage)`, and make `ITIMER_VIRTUAL`/`ITIMER_PROF`
real timers that fire `SIGVTALRM`/`SIGPROF` when the process burns CPU.

## Scope
- Per-process `utime`/`stime` tick counters, attributed once per 1ms timer
  tick from the interrupted CS (user bit) delivered by the IRQ stub.
- `clock_gettime(2|3)` returns the caller's accounted CPU time;
  `clock_getres` reports the 1ms tick resolution for every valid clock.
- `getrusage`: `RUSAGE_SELF`/`RUSAGE_THREAD` (own utime/stime + maxrss),
  `RUSAGE_CHILDREN` (cumulative children usage).
- `wait4` fills `struct rusage` with the reaped child's own plus inherited
  children time; reaping also folds that usage into the parent's
  `RUSAGE_CHILDREN` totals (recursively through the child's own children).
- `times()` fills `struct tms` in USER_HZ centiseconds and returns elapsed
  centiseconds since boot.
- `getitimer`/`setitimer` support all three timers: `ITIMER_REAL` keeps the
  wall-clock alarm, `ITIMER_VIRTUAL` counts user ticks and
  `ITIMER_PROF` counts user + kernel ticks; expiry signals
  `SIGVTALRM`/`SIGPROF` with interval reload.
- `/proc/<pid>/stat` fields 14-17 now report utime/stime/cutime/cstime in
  USER_HZ and the priority/nice/num_threads fields sit at their Linux
  positions (the old placeholder had them shifted by one).

Out of scope: per-thread accounting for clone-created tasks (each clone task
is its own process in Slop/MNU, so the thread and process CPU clocks report
the caller's own times), reaping the CPU usage of tasks that are never
waited for, and `clock_nanosleep` on CPU clocks (Linux blocks until the
process has consumed the requested CPU time; Slop/MNU rejects it — see
risks).

## Data model
- `Process.utime_ticks/stime_ticks` — own usage; `cutime_ticks/cstime_ticks`
  — waited-for descendants. Fork children start at zero (Linux semantics)
  and execve preserves the counters.
- The IRQ stub passes the interrupted CS in `rdi` (offset 128 from the
  saved-register frame base) and `on_timer_tick(user)` attributes the tick;
  the same hook drives the CPU interval timers, so virtual time only accrues
  while the process is on-CPU, by definition.
- `Alarm` entries carry a `which` discriminator (32 slots): REAL stores an
  absolute wall tick deadline, VIRTUAL/PROF store countdowns decremented by
  the owner's ticks; `cpu_tick` coalesces at most one signal of each kind.
- `WaitOutcome::Reaped` now carries the child's CPU ticks so `wait4` can
  fill rusage after the slot is torn down; accounting happens under the
  same process-table lock as the reap, so no extra lifetime is needed.

## Acceptance criteria
1. `./tools/acceptance-test.sh` prints `acceptance: PASS` with resource
   accounting intact.
2. `./tools/differential-test.sh` prints `differential test: PASS`,
   including new host/guest parity cases:
   - `DIFF_CPUTIME_OK`: CPU consumed during a 150ms user spin shows up in
     the process and thread CPU clocks (30-50ms tolerance band), is never
     ahead of wall time, appears in `getrusage(RUSAGE_SELF)` and in
     `/proc/self/stat` field 14.
   - `DIFF_RUSAGE_OK`: a spinning child's usage comes back through
     `wait4` rusage and lands in `RUSAGE_CHILDREN`.
   - `DIFF_VTIMER_OK`: one-shot `ITIMER_VIRTUAL` and `ITIMER_PROF` fire
     `SIGVTALRM`/`SIGPROF` during a spin; invalid `which` is rejected with
     `EINVAL`.
3. `./tools/check-source.sh` passes (new tokens guard the IRQ CS handoff,
   `account_tick`, `rusage_buf` and the accounting fields).

## Verification
- `cargo +nightly build -Zbuild-std=core --release` clean.
- Native (musl) smoke of `tests/differential.c` passes.
- `./tools/differential-test.sh` PASS, `./tools/acceptance-test.sh` PASS,
  `./tools/check-source.sh` pass.

## Risks
- Tick-granular (1ms) accounting: a short burst of CPU between ticks can
  round to zero; the differential tests use generous windows (>= 20-30ms)
  to stay robust on both bare metal and TCG.
- Attribution counts a whole tick as user or kernel based on where the
  interrupt landed, so syscall-heavy loads can overweight `stime` by up to
  a tick per sample — Linux has the same granularity caveat.
- `clock_nanosleep` on CPU clocks is rejected with `EINVAL`; Linux instead
  blocks until the requested CPU time is consumed (which deadlocks when the
  process has nothing else to run).
- `RUSAGE_CHILDREN` only accumulates children that are actually reaped by
  their parent; orphans are not reparented, and a child that is never
  waited for leaves its usage unaccounted.
- `ru_maxrss` is a live RSS snapshot for `RUSAGE_SELF` (KB, 4K pages) and
  zero for children; the remaining rusage fields stay zero (no page-fault
  or block-I/O counters yet).
- The thread CPU clock equals the process CPU clock because clone tasks are
  modeled as separate processes; pthread-based CPU-time expectations hold
  for the main thread only.
