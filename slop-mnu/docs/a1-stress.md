# A1 scheduler stabilization: stress battery record

Date: 2026-10-27. Kernel 0.85.0. Battery: `tools/smp-test.sh`
(`NCPUS="1 2 4 8" ITER=2`, musl-static guest binaries + Linux native reference).

## Races fixed

1. **AP exec RIP=0** (`kernel/task.rs`, `arch/x86_64/context.S`): `schedule()`
   marked the outgoing task Runnable + ownerless *before* the stack switch,
   so another CPU could claim it mid-switch and the first CPU then overwrote
   the live stack (BSP `#PF error=0x10 rip=0x0` right after AP `execve`).
   Fix: the task stays Running+owned across the switch; a `switch_hook`
   call in `switch_context` (post new-rsp load, SysV-aligned, `rcx`
   preserved) completes the transition under the scheduler lock.
2. **SCHED<->PROCS ABBA deadlock** (`kernel/task.rs`): exit/teardown held
   PROCS across scheduler reads while spawn held SCHED across table fills;
   fork/exit bursts wedged all CPUs spinning on the two locks. Fix: the
   scheduler lock is a leaf — spawns reserve a cleared slot
   (Sleeping+owned+pid 0+wake_tick MAX, invisible to claimers/scans/wakers),
   fill tables lock-free, then publish; `finish()` hoists its uid read.
3. **Stale reservation hazards**: cleared slots wholesale (a recycled
   `wake_pending` let the timer spuriously wake reservations mid-fill;
   `wake_tick=0` would too). Wake-race accounting: self-select of a woken
   outgoing task consumes its push; scanners skip mid-switch/current tasks
   silently (`skip_owned`) and count only stranded double-ownership.
4. **Socket lifecycle** (`kernel/fd.rs`, `kernel/socket.rs`): `close()` freed
   sockets unconditionally despite fork-shared references (EPIPE on live
   peers); last-close never freed (re-lookup after entry removal), leaking
   the 16-slot pool (EMFILE). Freed only on true last reference via the
   saved kind; `socket()` fully resets reused slots (`peer_closed`/waiters);
   pipe reset clears wait queues.

## Battery results (16/16 PASS, 2026-10-27)

`smp-stress` (fork CPU-bound `getcpu` children, pipe barrier + file I/O,
waitpid, native `SMP_OK` reference) and `smp-exec` (160 fork+execve,
364 fork/exit, 32 unix-socket ping-pongs, native `EXEC_OK` reference).

| test | 1vCPU x2 | 2vCPU x2 | 4vCPU x2 | 8vCPU x2 |
|---|---|---|---|---|
| smp-stress | PASS | PASS | PASS distinct=4 | PASS distinct=8 |
| smp-exec | PASS 160/364/32 | PASS | PASS | PASS |

Every run: `double_exec=0`, `lost_owner=0`, run-queue push==pop,
`audit runnable=0`, no `#PF`/`RIP=0`/panic/stack corruption, all children
reaped (`status=0`), clean QEMU shutdown. No 8vCPU old-crash recurrence.

## Not claimed

SMP overall stability beyond this battery (A2 remote stop/continue still
open), parallel-TCP/network-stack completeness, ext4 write completeness,
driver compatibility: none of these are proven by this battery.
