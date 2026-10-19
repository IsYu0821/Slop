# Milestone 62: pthreads end to end (create/join, mutexes, condvars, pthread_kill, TLS)

## Goal
Run real pthread workloads under musl: `pthread_create`/`pthread_join` with
return values, contended mutexes, condition variables, `pthread_kill`
delivery to a specific thread, and per-thread TLS — with the kernel keeping
thread and process identity, signal delivery and thread reclamation correct
while threads block inside syscalls.

## Scope
- `CLONE_PARENT_SETTID`/`CLONE_CHILD_SETTID` are honored: the child tid is
  written to the parent's `parent_tid` and the child's `child_tid` pointers.
  musl's `__pthread_exit` unlocks the thread list by expecting the kernel's
  clear-child-tid write of 0 to `&__thread_list_lock`, so without the write
  every exiting thread leaked a `__tl_lock` level and the first join
  deadlocked.
- `clone3` parses the real struct offsets (`child_tid` at +16, `parent_tid`
  at +24, `stack` at +40, `tls` at +56) and forwards the tid pointers like
  `clone`.
- Thread exits no longer send `SIGCHLD` to the creator: threads are joined
  through `detach_state` futexes, and a spurious `SIGCHLD` made the main
  thread's `usleep` return immediately.
- `tkill`/`tgkill` and `getpid`/`gettid` use a per-task `tgid` (thread-group
  id): clone threads inherit the creator's `tgid`, `gettid` returns the
  task id and `getpid` the group id, so `pthread_kill` reaches the right
  task.
- Futex waiters identify their entry by `(uaddr, pid)` instead of the slot
  index they happened to get: when a woken waiter resumed, another waiter
  (the joiner) could have recycled its slot, making the waker miss and the
  woken task clobber the joiner's entry — a classic lost wakeup.
- Signal delivery from the syscall-exit path uses the live per-task stack
  frame (`mov %rsp, %rdi`) instead of the `SYSCALL_FRAME` global: a thread
  that blocked inside a syscall (nanosleep) resumed with the global pointing
  at another task's frame, so `pthread_kill`ed threads popped their pending
  signal without ever running the handler. The global remains for
  fork-frame capture, where the current task's entry has just set it.
- Dead threads' task slots are reclaimed when a new thread is spawned: the
  kernel never `wait`s for a thread, so its zombie slot is freed explicitly
  and its CPU ticks are folded into the thread-group leader's own times,
  matching Linux's per-process accounting. Without this, eight sequential
  pthreads exhausted the 8-task table and `pthread_create` failed with
  `EAGAIN`.

Out of scope: thread-directed signals blocked while a task sits purely in
user mode (delivery still happens at the next kernel entry), `CLONE_CHILD_CLEARTID`
after execve, per-thread CPU clocks for live threads (a dead thread's ticks
are only visible in the leader totals), and pthread cancellation
(`pthread_cancel` is not exercised).

## Data model
- `Task.tgid` joins all threads of a process; `spawn_thread` records it,
  `fork`/`exec` give children their own group.
- `finish()` marks a thread `Zombie` without parent notification; the slot
  lingers until `take_slot` needs it, then `PROCS[slot]` is dropped and
  utime/stime/cutime/cstime are added to the leader found by
  `pid == tgid`.
- The syscall exit path computes the signal frame address from `%rsp` at
  the point where the 15-register frame is the whole stack: frame and task
  are the same by construction, independent of scheduling history.

## Acceptance criteria
1. `./tools/acceptance-test.sh` prints `acceptance: PASS`; the
   `hello-thread` step (`thread: create, join and mutex passed`) and
   `hello-signal` handler delivery still pass with their resource
   accounting intact.
2. `./tools/differential-test.sh` prints `differential test: PASS`,
   including new host/guest parity cases:
   - `DIFF_PTHREAD_OK`: four workers under one contended mutex return join
     values and the total counter is exact.
   - `DIFF_COND_OK`: a condvar waiter parks and is woken by a signal under
     the associated mutex.
   - `DIFF_KILL_OK`: `pthread_kill` interrupts a thread looping in
     `usleep`, its handler records the right `gettid`, and the join returns
     that tid.
   - `DIFF_TLS_OK`: three threads keep independent `__thread` values across
     sleeps.
3. `./tools/check-source.sh` passes (new tokens guard `signal_setup_frame`,
   `signal_task`, `current_tgid`, `take_slot`, `sys_tgkill`, the futex wait
   path and the pthread differential markers).

## Verification
- `cargo +nightly build -Zbuild-std=core --release` clean.
- Native (musl) smoke of `tests/differential.c` passes.
- `./tools/differential-test.sh` PASS, `./tools/acceptance-test.sh` PASS,
  `./tools/check-source.sh` pass.

## Risks
- Threads are still modeled as separate tasks with their own `Process`
  copies: signal masks and handlers are per-task (handlers inherited at
  clone, matching fork semantics closely enough for musl), and signals sent
  with `kill` land on the leader only.
- Thread zombies are reclaimed lazily at the next thread spawn; a program
  that creates many short-lived threads without ever spawning another one
  after the peak can still hit the 8-task table (`EAGAIN` from
  `pthread_create`).
- Signal delivery for user-mode-only loops waits for the next kernel entry;
  a handler can be delayed until the thread makes any syscall (musl's
  `usleep`-based waits are covered).
- CPU ticks of a live thread are not visible anywhere except its own
  `CLOCK_THREAD_CPUTIME_ID`; they only reach the leader when the thread is
  reclaimed.
