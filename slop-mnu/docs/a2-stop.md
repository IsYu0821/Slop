# A2 remote stop/continue: implementation and stress record

Date: 2026-10-27. Kernel 0.86.0. Battery: `tools/stop-test.sh`
(`NCPUS="1 2 4 8" ITER=3`, musl-static `tests/smp-stop.c` + Linux native
reference: 25 stop/cont rounds on a CPU-bound child, no user progress
while stopped, exact resume, native-identical wait statuses).

## Design

- A stop request never touches a running task's state or ownership. Remote
  SIGSTOP on a Running target only sets the pending bit and IPIs the owning
  CPU; the target stops itself at its next boundary. Direct Stopped is used
  only for queued targets (Sleeping/Runnable), with run-queue accounting
  balanced (no push on direct stop; a consumed queue slot is paid back).
- Preemption for CPU-bound tasks: timer/IPI interrupt returns check for a
  pending stop (or other claimable work) and set TF, trapping into #DB on
  the task's own kernel stack (IST handlers cannot schedule). #DB stops or
  yields there. Syscall entry and return check for pending stops and
  reschedule requests. 1ms bound, no unbounded spinning in user code.
- Lost-CONT safety: the park loop breaks on a cont-epoch newer than the
  mark recorded at stop time, so a SIGCONT racing the stop can never park
  forever; stale SIGCHLD from a cancelled stop is cleared.
- SIGCONT discards pending stop signals (Linux); resume is idempotent
  (single Runnable transition, single waitpid Continued report).

## Fixes found by the stress (correct under all interleavings)

- Timer preemption did not exist (`take_need_resched` had no callers):
  any CPU-bound task starved its CPU forever. Now: timer trap + #DB yield,
  syscall-return yield on explicit request, lifecycle kicks on wake/resume.
- Per-CPU syscall cells (`exec_rip/pending`, `syscall_frame`, `sigreturn`)
  were not parked across switches: a syscall-return yield let another task
  clobber them (exec returned into old musl text instead of the new image).
  Parked/restored per task like `user_rsp`.
- TLB: CR3 reload was skipped on PML4-frame reuse (stale mappings faulted
  executable pages); `destroy_private_slot` never invalidated TLBs.
  Always activate on switch; invlpg on bulk unmap.
- ROOT-store inode numbers (`len+2`) were reused after unlink compaction,
  so two live files shared an ino and I/O resolved to the wrong node
  (EOF on live data). Monotonic counter, disjoint from initramfs/ext2/ext4
  and tmpfs-tagged numbers.
- Socket lifecycle (found via exec battery): `close()` freed fork-shared
  sockets (EPIPE on live peers); last-close never freed (re-lookup after
  entry removal), leaking the 16-slot pool; reused slots kept
  `peer_closed`/waiters. Freed only on true last reference via the saved
  kind; full reset on alloc; pipe reset clears wait queues.

## Battery results (all green, 2026-10-27)

- `tools/stop-test.sh`: 1/2/4/8 vCPU x 3 iters PASS, STOP_OK rounds=25,
  double_exec=0 lost_owner=0 delta=0 runnable=0, no faults.
- `tools/smp-test.sh` (A1 regression): 16/16 PASS with exact counts.
- Native references match (STOP_OK markers=26; wait statuses identical).

## Not claimed

General scheduler optimality, fairness bounds beyond the 1ms trap, NUMA or
real-hardware timing, driver coexistence with heavy preemption, hardlink
ino sharing (links still get fresh inos). The every-syscall yield
heuristic was tried and removed: under extreme switch churn it exposed an
unidentified file/pipe interleaving anomaly; preemption relies on timer
traps and explicit kicks instead (see known-issues if filed).
