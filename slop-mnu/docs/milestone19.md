# Milestone 19: per-task architecture state and TLS foundation

Task descriptors now carry the active CR3, FS base, kernel-stack top, saved kernel RSP, PID, state, wake tick, and exit status. Before a cooperative context switch, the scheduler applies the next task's architecture state: CR3 when different, IA32_FS_BASE, TSS.RSP0, and the SYSCALL entry stack pointer.

The bootstrap task is initialized with the actual 64 KiB ring-0 stack configured during descriptor setup. Kernel threads continue to use their own 32 KiB stacks. This distinction matters because a later user-to-kernel privilege transition must land on the kernel stack belonging to the selected task.

`arch_prctl(ARCH_SET_FS)` now updates both the current task record and IA32_FS_BASE. `ARCH_GET_FS` continues to copy the stored value to user memory. On every scheduler switch the saved value is restored, establishing the minimum TLS behavior needed by libc once user tasks become schedulable.

The self-test now assigns FS base `0x1000` to one kernel thread and `0x2000` to another. Each thread yields, resumes, reads IA32_FS_BASE, and requires its own value. Passing the test demonstrates that context switches update task-local TLS state in addition to CPU stack state.

This is still a single-CPU design. GS-based per-CPU state and SWAPGS are not enabled, and timer IRQs set `need_resched` without replacing the interrupted return frame.
