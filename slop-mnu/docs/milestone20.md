# Milestone 20: wait queues and zombie reaping

Tasks now record a parent PID when created. The Linux `getppid` syscall reads this scheduler relationship instead of returning a constant zero. The bootstrap task remains parentless, while kernel threads created by it receive PPID 1.

A fixed-capacity `WaitQueue` stores up to eight waiting PIDs under `IrqSpinLock`. `sleep` registers the current task and blocks it; `wake_one` removes one waiter and changes its task state from sleeping to runnable; `wake_all` repeats that operation until the queue is empty. This is the basic synchronization primitive needed by future futexes, pipes, device reads, and child-exit waits.

The scheduler now exposes `block_current`, `wake_pid`, `reap_zombie`, and `wait_child`. Reaping verifies the caller is the recorded parent, returns the child's exit status, clears the task descriptor, clears its stored kernel-thread entry, and makes the fixed task slot reusable. The current bounded `wait_child` yields cooperatively and returns `EAGAIN` if no matching child becomes a zombie during the scan. A fully blocking Linux `wait4` will use a child-exit wait queue rather than this bounded polling loop.

The boot self-test still runs two kernel threads with distinct FS bases, but now also waits for both to become zombies, validates their zero status, and reclaims both task slots. The automated acceptance marker is `scheduler: context, FS base and zombie reap self-test passed`.
