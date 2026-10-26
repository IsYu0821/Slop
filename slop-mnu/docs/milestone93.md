# Milestone 93 — True SMP parallel execution (shared run queue)

APs no longer park after attestation. Each AP enters a shared run-queue
scheduler, claims runnable tasks atomically, and runs user processes with
full syscall, FPU, and address-space switching. A per-CPU idle loop
(`sti`/`hlt`) parks CPUs without work and wakes on the timer tick or on a
reschedule IPI from a waker.

## Scheduler

- One global task table (`MAX_TASKS = 32`), shared by all CPUs.
- Per-CPU current slot (`cpu_current`), idle context (dedicated 32 KiB
  idle stack + fake switch frame), and `NEED_RESCHED` flag.
- Claims are atomic under the scheduler lock with a task-ownership record
  (`TASK_OWNER`). A claim that finds a live owner counts a double-exec
  instead of running the task twice.
- Run-queue accounting: every transition into `Runnable` bumps `RUNQ_PUSH`,
  every claim out bumps `RUNQ_POP`. Quiescent audits require
  `PUSH - POP == #Runnable`, zero lost owners, zero double-execs.
- Switching away from a dead (`Zombie`/`Unused`) task uses a no-save switch
  so no write lands in a slot that another CPU may reap and reuse
  concurrently.
- Idle CPUs set an idle bitmask; wakers (`wake_pid`, `wake_parent_for_wait`,
  `spawn_*`, timer `wake_sleepers`) poke one idle CPU with a reschedule IPI.
- Per-CPU stats: tasks run, context switches, syscalls, IPIs sent/received,
  idle loops, halts, timer ticks. Printed once at shutdown with the audit.

## AP bring-up fixes (found by parallel execution)

- The trampoline leaves APs with its own CS (0x18). The kernel GDT
  describes 0x18 as data, so the first interrupt return on an AP raised
  #GP(0x18). APs now reload CS (`retfq`) after loading their GDT, mirroring
  what `descriptors::init` already did for the BSP.
- The `ap_enter` stack switch was written with AT&T operand order in
  Intel-syntax inline asm (`mov rsp` clobbered the target instead of
  loading it). APs now enter the idle loop on their own stack with
  call-conformant alignment.
- TSS RSP0 is per-CPU: the BSP keeps `BOOT_TSS`, APs use their `CPU_TSS`
  slot. Task switches and idle entry update only the switching CPU's TSS.
- The AP Local APIC timer is started on scheduler entry (calibrated count
  from the BSP), so ticks, sleepers, and accounting run on every CPU. The
  RX ring and TCP timers stay BSP-only.

## Scope and limits

- Kernel threads (`spawn_kernel`) and the boot task stay pinned to the BSP;
  user tasks (fork/thread/reaped children and the smp-stress battery) migrate
  freely and have been observed running user code and syscalls on APs.
- `spawn_user` (the `/init` exec chain) is pinned to the BSP for stability:
  exec-heavy early boot on APs trips an under-investigation crash (BSP jumps
  to zero while an AP runs the exec chain). Fork/thread children migrate and
  pass file, pipe, socket, and pthread batteries. Exec migration is deferred.
- Stop/continue (`SIGSTOP`/`SIGCONT` with `waitpid`) is flaky under load on
  4+ vCPUs (remote-running targets are marked stopped without preemption);
  1–2 vCPU acceptance passes consistently.
- 8 vCPU boots and attests but acceptance is unstable (under investigation).
- Load balancing is work-conserving first-claim (no work stealing). With an
  IPI-driven AP wakeup the AP often wins races against a halted BSP; the
  smp-stress battery proves parallel CPU-bound execution on 4 vCPUs.

## Proof

- `tools/smp-test.sh`: musl `tests/smp-stress.c` (fork 8 CPU-bound children
  reporting `getcpu`, pipe + file I/O, exit statuses) runs natively and in
  the guest on 1/2/4 vCPUs. 4 vCPU shows 3 distinct CPUs running user code
  with `SMP_OK`; 2 vCPU shows AP-executed children with `SMP_OK`.
- Acceptance passes on 1 and 2 vCPUs with APs active (`ap_user_tasks=8`,
  AP syscalls served, `double_exec=0`, `push==pop`, audit clean), then
  `report_smp_stats` prints the per-CPU accounting.
- `check-source` updated only by addition (new tokens); all prior gates
  (heap, differential, busybox 115, ext2/e2fsck, ext4 read battery) unaffected.
