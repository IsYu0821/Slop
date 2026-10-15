# Milestone 18: scheduler and kernel-thread foundation

Slop/MNU now has a single-CPU fixed-capacity scheduler with eight task slots. Each task records PID, scheduling state, saved kernel RSP, kernel-stack top, wake tick, and exit status. Task states include unused, running, runnable, sleeping, and zombie. PIDs are allocated dynamically from 2 while the bootstrap task remains PID 1.

Each task slot owns a 32 KiB aligned kernel stack. New kernel threads receive a synthetic switch frame containing the SysV64 callee-preserved register set and a return target for `kernel_thread_trampoline`. The initial stack includes an alignment bias so the trampoline observes the required SysV function-entry alignment.

`switch_context` saves RBP, RBX, and R12-R15, stores the old RSP, loads the next RSP, restores the next task's preserved state, and returns into that task. The scheduler selects runnable tasks round-robin while holding `IrqSpinLock`, but releases the queue lock before performing the assembly switch.

The Local APIC timer increments ticks, wakes sleeping tasks and sets an atomic `need_resched` flag. It deliberately does not switch tasks inside the IRQ handler yet. Full timer preemption requires an interrupt-frame-aware return path capable of replacing the interrupted context safely. Cooperative `yield_now` and `sleep_until` are active now.

A boot-time self-test creates two kernel threads. Each advances a counter, yields, resumes, advances again, and exits into zombie state. The test passes only after both counters reach two, demonstrating separate kernel stacks and bidirectional context switching. The automated QEMU gate now requires `scheduler: context switch self-test passed`.
