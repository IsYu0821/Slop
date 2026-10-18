# Milestone 34: threads

`CLONE_VM` threads now run. `/bin/hello-thread` is a musl-linked program that
creates two threads, has them increment a shared counter under a mutex two
hundred times, joins them, and checks the total.

## clone

`sys_clone` accepts the `CLONE_VM` family: `CLONE_VM`, `CLONE_FS`,
`CLONE_FILES`, `CLONE_SIGHAND`, `CLONE_THREAD`, `CLONE_SYSVSEM`,
`CLONE_SETTLS`, `CLONE_PARENT_SETTID`, `CLONE_CHILD_CLEARTID`,
`CLONE_CHILD_SETTID` and musl's private `CLONE_DETACHED` (0x400000, which musl
uses instead of `CLONE_CHILD_SETTID`). Anything else is `EINVAL`.
`clone3` dispatches to the same paths based on `CLONE_VM`.

`task::spawn_thread` builds a child that shares the caller's CR3 and
descriptor table (reference-counted clone), runs on the caller-supplied stack
with `fs_base` set from the `CLONE_SETTLS` value, and records the
`CLONE_CHILD_CLEARTID` address. The child resumes at the saved syscall return
point with `rax=0`, exactly as a forked child does, so musl's `__clone`
trampoline (`pop arg; call *%r9`) runs unchanged.

## Thread exit

`Task` gained `is_thread`. `task::exit` checks it first: a thread clears its
`clear_child_tid` word, wakes any joiner blocked on that address through the
futex table, and leaves the shared address space and descriptors intact.
Only the last task of a process tears down the image.

## Per-task user RSP

`SYSCALL_USER_RSP` was a single global, which is correct only while one thread
owns the syscall path. With threads, a blocked joiner's user RSP was clobbered
by the thread it was waiting for, and the joiner returned to a garbage stack.
`Task` now carries `user_rsp`; `schedule` parks the outgoing task's value and
restores the incoming task's value around the context switch.

## MAP_STACK

`mmap` rejected `MAP_STACK` (0x20000) and `MAP_GROWSDOWN` (0x100), which
`pthread_create` passes when allocating its stack. Both are accepted and
ignored; only `MAP_FIXED`, `MAP_ANONYMOUS` and the private flag were allowed
before.

## Known issue

Tearing down a process that ran threads prints `image/stack/mmap slot teardown
failed` and `process PML4 recycle failed`. The affected pages are leaked, so a
single acceptance run completes, but the reclaim path needs a follow-up fix.

## Deliberate limits

No `CLONE_SETTLS` validation beyond canonical range, no thread-local
destructors, no `set_robust_list` per-thread registration, no cancellation
points, and no `pthread` affinity or scheduling control. Detached-thread
reaping relies on the joiner futex, matching `CLONE_CHILD_CLEARTID`.
