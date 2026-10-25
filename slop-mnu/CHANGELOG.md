## 0.79.0 - 2026-10-26
- Dirfd-relative `*at`: `openat`, `mkdirat`, `unlinkat`, `renameat`, `linkat`, `symlinkat`, `readlinkat`, `newfstatat`, `faccessat`, `fchmodat`, `fchownat`, `statx`, `utimensat` and `execveat` now resolve relative paths against directory fds with native `AT_FDCWD`/`AT_SYMLINK_NOFOLLOW`/`AT_REMOVEDIR`/`AT_EMPTY_PATH` semantics (`ENOTDIR` for files, `EBADF` for bad fds, absolute paths ignore dirfd).
- New `renameat2` dispatch (syscall 316) with `RENAME_NOREPLACE`; `AT_EMPTY_PATH` operates on (or executes) the fd itself, and stats the cwd under `AT_FDCWD`.
- Fixed the in-kernel `statx` layout to the published UAPI (`ino` at offset 32, 16-byte timestamps); the new byte-identical battery caught the drift.
- The differential battery covers it with `DIFF_AT_OK` on native Linux and in the guest, across ext2 and root mounts.

## 0.78.0 - 2026-10-26
- `AF_UNIX` datagrams carry addresses: `bind` (`EADDRINUSE`/`EINVAL`), unconnected-`send` `ENOTCONN`, `sendto` to unknown paths `ENOENT`, `recvfrom` source reporting (empty for unbound senders), connected `send`/`recv` with `getpeername`, message-boundary preservation, abstract names (leading NUL) and `socketpair(SOCK_DGRAM)` as a connected pair.
- Per-socket datagram inbox (8 x 4 KiB messages with sender paths) blocking on the existing wait queues with `EAGAIN` for nonblocking sockets; `listen`/`accept` on datagrams report `EOPNOTSUPP` as on Linux.
- `sockaddr_un` handling keeps abstract names raw so they never collide with filesystem paths; oversize datagrams report `EMSGSIZE` (guest caps at 4 KiB; the probe uses 300 KiB where Linux agrees).
- Small ABI corrections: `NICE` is 134 (not 34, which is `PAUSE`) and `TIMES=100` now reaches the existing `sys_times` instead of `ENOSYS`.
- Fixed an ext2 block leak: a failed `mkdir` (e.g. the `EEXIST` probe) released the inode but not the data block, because the cleanup called `truncate_file`, which refuses directories. The cleanup now frees directory blocks directly; the host `e2fsck` gate is clean and free counts account exactly.
- The differential battery covers it with `DIFF_UNIXDG_OK` on native Linux and in the guest.

## 0.77.0 - 2026-10-26
- Real network servers in the guest: TCP passive open with `Listen`/`SynRcvd` states, per-listener accept queues, SYN/ACK handshake, RST refusal (`ECONNREFUSED`), retransmitted-SYN handling and listener-close resets.
- Loopback transport for 127/8 and the guest's own address: TCP segments and UDP datagrams are delivered locally through a lock-free-to-callers in-kernel queue drained by `poll()`, with symmetric addressing so both directions and `getpeername`/`recvfrom` match Linux.
- `listen`/`accept`/`accept4` now serve `AF_INET` (peer `sockaddr_in` reporting, `EINVAL` for non-listeners, `ENOTCONN` peername on listeners); `bind` rejects conflicts (`EADDRINUSE`) and rebinding (`EINVAL`); UDP `listen` stays `EOPNOTSUPP` as on Linux.
- INET sockets are freed on last close/exit instead of leaking slots.
- Cooperative-scheduler fix: blocking network waits yield regularly instead of starving the peer task they rendezvous with.
- The differential battery covers it with `DIFF_TCPSRV_OK` (bind conflicts, refused connect, echo server, orderly EOF) and `DIFF_UDPLOOP_OK` over `127.0.0.1`.

## 0.76.0 - 2026-10-25
- File plumbing for real software: `copy_file_range` (explicit/current offsets, `EINVAL` for pipes and flags, `EISDIR` for directories), `splice` (one side must be a pipe, `ESPIPE` for pipe+offset, `EINVAL` otherwise), `tee` (pipe-to-pipe duplication without consuming, flags ignored) and `fallocate` mode 0 plus `FALLOC_FL_KEEP_SIZE` and `FALLOC_FL_PUNCH_HOLE` over a new capacity-reserving `vfs::reserve_ino`.
- New `pipe::peek` primitive backs `tee` without waking writers or freeing space.
- Dispatch cleanup: live arms replace stale `ENOSYS` entries and unreachable duplicate arms for working calls are removed.
- The differential battery covers all four calls byte-for-byte with `DIFF_CFR_OK`, `DIFF_SPLICE_OK`, `DIFF_TEE_OK` and `DIFF_FALLOC_OK` on native Linux and in the guest.

## 0.75.0 - 2026-10-24
- signalfd now reports child stop, continue and exit with Linux contents: `CLD_STOPPED=5` with the stop signal, `CLD_CONTINUED=6` with `SIGCONT`, `CLD_EXITED=1` with the exit code and `CLD_KILLED=2` with the terminating signal, plus the child pid and uid in a full 128-byte `signalfd_siginfo`.
- One coalesced `SIGCHLD` slot with first-wins semantics backs signalfd, `sigwaitinfo`/`sigtimedwait` and handler delivery: consuming one path never consumes a child task state, and `wait4`/`waitid` never consume the pending bit, so read-then-wait and wait-then-read both observe the sequence.
- `SA_NOCLDSTOP` (even with `SIG_DFL`) suppresses stop/continue generation without affecting `waitpid`/`waitid` observation; the exit event still arrives.
- `signalfd4` dereferences the user mask, honors `SFD_NONBLOCK`/`SFD_CLOEXEC` and reprograms an existing fd; short reads fail `EINVAL`, empty nonblocking reads fail `EAGAIN`, blocking reads wait; `poll`/`select`/`epoll` follow `pending & mask`, and `poll` rechecks until its timeout instead of sleeping once.
- `waitid` reports `CLD_STOPPED=5` and `CLD_KILLED` for signaled children and accepts `WCONTINUED=8`; `sigtimedwait` returns the stored child pid/uid/status/code for `SIGCHLD`.
- The differential battery covers all of it with `DIFF_SIGNALFD_STOP_OK` and `DIFF_SIGNALFD_NOCLDSTOP_OK` on native Linux and in the guest.

## 0.74.0 - 2026-10-24
- Added stopped/continued job control: a `Stopped` scheduler state for `SIGSTOP`/`SIGTSTP`/`SIGTTIN`/`SIGTTOU` with the default disposition, `WIFSTOPPED` with `WUNTRACED` and `WIFCONTINUED` with `WCONTINUED` in `wait4`, `CLD_STOPPED`/`CLD_CONTINUED` in `waitid`, and `SIGCHLD` on stop and continue unless `SA_NOCLDSTOP`.
- Added the floating-point state to the signal `ucontext_t`: the 512-byte FXSAVE area with the SSE registers at offset 160, `MXCSR` at its reset value and `fpregs` pointing at the area, round-tripped through `rt_sigreturn`.
- User SSE registers now survive system calls: the syscall entry parks them in the running task's per-task FPU area and restores them before `iretq`, so handlers observe the interrupted registers and their edits take effect.
- Fixed `pending()` masking `SIGKILL` and `SIGSTOP` out of the deliverable set, so neither was ever delivered through the normal path.
- The differential battery covers both with `DIFF_STOP_OK` and `DIFF_FP_OK` on native Linux and in the guest.

## 0.71.0 - 2026-10-22
- Added the vector system call entry point, the page of code Linux gives every program so it can read the clock without entering the kernel. The page carries the entry points `__vdso_clock_gettime`, `__vdso_gettimeofday`, `__vdso_time` and `__vdso_getcpu`, the same names Linux uses, and the auxiliary vector points at it, so the C library finds them in the dynamic symbol table and uses them.
- The entry point reads the time from a shared page that the kernel keeps up to date: the rate of the time stamp counter, the value of the counter at the last update, and the monotonic and the wall clock at that moment. The page is read under a sequence lock, so a program either sees a complete update or the one that follows it. The page is the same physical frame in every process, so the kernel writes it without knowing which program runs.
- A program that reads the clock now spends its time in user mode, so the processor time of the program is charged to the program and not to the kernel, exactly as Linux reports it.
- Fixed the interrupt path, which read the code segment of the interrupted context from the wrong place and therefore charged nearly every timer tick to the kernel. The vector register save area and the general registers lie between the stack pointer and the frame of the processor, so the code segment is 384 bytes above the stack pointer.
- The entry point and the shared page are released with the address space, and the frame of the shared page is counted, so the last program that goes away does not take the page from the kernel.

## 0.71.0 - 2026-10-22
- The clock is now measured with the time stamp counter. The kernel moves the clock forward at every timer tick by the time the counter has passed, and the length of a tick is measured the same way, so the length of a tick follows the counter and does not drift away from it. The monotonic clock, the wall clock and the page of code that reads the clock now describe the same time.
- The processor time is counted in nanoseconds instead of ticks. Counting ticks and converting them afterwards was wrong as soon as the length of a tick changed, because the conversion used the length that was measured last for every tick that came before.
- The page of code clamps the time since the last update to zero. The counter can move backwards when the machine takes the processor away, and the difference would otherwise wrap around and report a time far in the future.
- The page of code bounds the number of times it reads the shared page again, so a reader never waits for ever when the kernel is in the middle of an update.
- The futex reads the word and registers the waiter under the same lock the waker uses to take the waiter out of the table. A wakeup that came between the two was lost before: the waker found no waiter and the waiter blocked after the word had changed.
- The timeout of a futex wait is a length of time and not a point in time, which is how Linux reads it.

## 0.73.0 - 2026-10-22
- Fixed the real-time signal path. The kernel read the value of a queued signal from the wrong offset of the published siginfo, and the signal was not delivered because the kernel treated the timeout of a futex wait as a point in time instead of a length of time.
- Fixed the timer signal's value. The kernel stored the value at the offset the C library reads, and the signal now carries it to the handler.
- The differential battery runs to the end with every marker present, twenty four times in a row.

## 0.72.0 - 2026-10-22
- Added the real-time signal range, from 32 to 64. A real-time signal carries a value and is queued, so more than one of them can wait at the same time. The C library sends them with `sigqueue`, and the handler reads the value from the `siginfo` the kernel builds.
- Added `sigaltstack` and `SA_ONSTACK`, so a handler can run on a stack of its own. The kernel builds the frame on the alternate stack, marks it as in use while the handler runs, and frees it when the handler returns.
- The signal number 65 is refused, which is what Linux does for a signal outside the range.
- The value of a queued signal is reported at the offset the C library reads, and the alternate stack is reported with the size a program asks for.

## 0.70.0 - 2026-10-22
- The kernel counted one timer tick as a millisecond, but the local APIC timer of a machine does not run at a rate the processor reports. The monotonic clock, the wall clock, the processor time and every interval timer were therefore scaled by a wrong unit.
- The time stamp counter is now measured against the programmable interval timer, whose rate is fixed by the hardware. The rate of the counter on the test machine is 2.92 GHz, and the local APIC timer ran at a rate that made a tick 1.6 milliseconds long instead of the assumed millisecond.
- The length of a timer tick is measured inside the interrupt, where the counter is read a few cycles after the interrupt arrives, so the delay of the interrupt cancels out over a run of ticks. The timer is then programmed so that a tick lasts a quarter of a millisecond, the rate a Linux kernel with a 250 hertz timer uses, and the tick has the same length on every machine.
- The interval timers used the tick as a millisecond, so an alarm of five seconds asked for five thousand ticks and the time that was left was reported wrong. They use the measured length of a tick now.
- The wall clock timer rounds its deadline down, so the time that is left is never longer than the time that was asked for. That is what Linux reports when a timer is cancelled, and it is the reason the cancellation of an alarm of five seconds reports five seconds.
- The processor time accounting reports real nanoseconds.

## 0.69.0 - 2026-10-22
- Fixed the virtio 1.0 notification capability, which was read with the field offsets of the configuration capabilities. The multiplier, the offset and the bar of the notification area were therefore wrong, the device was never notified, and no transmit ever completed. The notification register of a queue now sits at the area base plus the offset plus the multiplier times the queue index, and the queue index is written there.
- The feature negotiation accepts the version 1 feature and nothing else. Accepting a feature the driver does not implement would let the device use a path that is not served here, such as a merged receive buffer, a flush or a discard.
- The queue size of a modern device is read back after the driver offers it, so the ring layout follows the size the device actually uses.
- The virtio drivers keep their state behind an interrupt safe lock and no longer hold it while waiting for a transmit to complete. The timer interrupt polls the network, so a plain spin lock deadlocked the guest as soon as a transmit was outstanding.
- Fixed the TCP send window: the acknowledged base of the window never moved, so the same bytes were retransmitted for ever and the connection was never closed. The close of a socket now completes.
- The user address space gives the program break and the dynamic interpreter a page table slot of their own, so a mapping in one region can no longer overwrite the page table entry of another.
- The SSE register file now survives an exception and an interrupt, so a faulting instruction that uses the vector registers is retried with the values the user program left there.
- A blocked fatal signal no longer ends the process that sent it, which is what Linux does.
- Removed a leftover debug print from the resource usage syscall. It wrote to the console while the process lock was held, which deadlocked the guest.

## 0.68.0 - 2026-10-21
- The running task identifier now comes from the scheduler instead of a cached copy, so a signal or a wait that names the current task cannot name a task that ran in between.

## 0.67.0 - 2026-10-21
- Fixed the user address layout: the program break, the anonymous mappings, the dynamic linker and the position independent image shared page table entries, so a mapping in one region overwrote the page table entry of another. The program break now lives in page table slot 131, the anonymous mappings in slot 130, the image in slot 129 and the linker in slot 128, and every region is alone in its slot.
- The loader accepts a dynamic image whose interpreter and main image live in different page table slots.
- This removes the intermittent memory corruption that broke the BusyBox and differential batteries: a program that used both the heap and an anonymous mapping lost the heap contents, and a C library allocator aborted on its own consistency check.

## 0.66.0 - 2026-10-21
- Added a virtio transport that speaks both device interfaces: the legacy port I/O device (version 0.9.5) and the modern memory mapped device (virtio 1.0). The modern interface is negotiated with the `VIRTIO_F_VERSION_1` feature and confirmed through the `FEATURES_OK` status bit.
- The virtio-block and virtio-net drivers now use that transport, so a default `virtio-blk-pci` or `virtio-net-pci` device model works. Before this the drivers only understood the legacy interface, so every device had to be started with `disable-modern=on`.
- The test harness no longer asks the emulator for a legacy device: the acceptance, differential and BusyBox batteries now run against the modern device models.
- Fixed the split virtqueue offsets: the available and used rings start with a flags word, so the index follows it.

## 0.65.0 - 2026-10-21
- Implemented the published x86-64 `rt_sigframe`: the restorer slot, the `ucontext_t` with the machine context in the published `REG_*` order and the `siginfo_t` with the sender in the `_kill` member. A handler with `SA_SIGINFO` now receives the signal number, the siginfo pointer and the ucontext pointer, like Linux.
- Replaced the `sysretq` syscall return with an `iretq` return: `SYSRET` loads RFLAGS from RAX, so the old path replaced the user flags with the syscall result. The user flags and the syscall result now both survive the call, and a signal handler is entered with the published argument registers.
- The delivered signal leaves the pending set, the pre-handler mask is restored on return, and `SA_RESETHAND`, `SA_NODEFER` and `SA_SIGINFO` behave like Linux.
- A blocking `wait4` or `waitid` reports `EINTR` for a signal with a handler, while a signal with the default "ignore" disposition (`SIGCHLD`) no longer interrupts the wait.
- A fatal signal that a process sends to itself ends the process before the syscall returns, but only when the default disposition is in effect.
- Raised the descriptor limits to the Linux values: 1024 descriptors per process and 512 open file descriptions, with a bitmap for the close-on-exec flags.
- The virtual-memory area ledger merges adjacent areas, reuses a released range and stores the inode number instead of a whole inode, so a process stays small and a long-running program no longer exhausts the table.
- `madvise` follows the Linux advice set (`MADV_DONTNEED` drops the pages, `MADV_FREE` keeps them, an unknown advice reports `EINVAL`) and `rseq` reports `ENOSYS` so a C library does not assume a registration that never happened.
- Added `tests/heap.c` and `tools/heap-test.sh`, a heap stress program that reproduces the allocator setup of a C library inside the guest.
- The firmware runner picks the first OVMF image pair that exists, so the acceptance gate works on distributions that only ship the 4 MiB build.

## 0.64.0 - 2026-10-20
- Made `mmap` with `MAP_FIXED` replace the previous mapping instead of failing with `EEXIST`, as Linux does; BusyBox's allocator relies on it to place a guard page below its heap.
- Made the program break grow repeatedly: the heap area is now replaced when it grows, so the second and later `brk` extensions succeed instead of failing with `EEXIST`.
- Fixed BusyBox's own allocator dying inside `malloc` after a failed heap setup; a sixty-command `sed` loop in the guest went from eight crashes to two.

## 0.63.0 - 2026-10-20
- Measured Linux compatibility with a real userspace: a static, non-PIE BusyBox 1.36.1 (musl) now runs in the guest and its output is compared byte for byte with the same binary on Linux (`tools/busybox-test.sh`, 115 checks over shell builtins, the filesystem, the text tools and process control).
- Fixed the syscall result being lost when a signal arrived: `rt_sigreturn` restores the interrupted register frame, but the dispatcher overwrote it, so `wait4` returned 0 and shells lost the exit status of their children.
- Path resolution now follows symlinks in every component, absolute link targets resolve from the root, and `readlinkat` reports the link itself instead of its target.
- Fixed short reads on regular files, which truncated every initramfs file to its first page.
- Implemented `rt_sigsuspend` properly (x86-64 passes the mask pointer in `rdi`; the old code stored the pointer as the mask) so shells waiting for children wake up, and added the missing `vfork` syscall.
- Fixed `execve` reusing the frames of the image it replaced (the teardown then freed the new image), and made `create` fail with `ENOENT` when the parent directory is missing.
- Made the initramfs files read-only so a write can no longer corrupt the kernel image, and made `fstat` report the live file size.
- Preserved the SSE register file across context switches (the sixteen XMM registers are saved per task) and enabled `CR4.OSFXSR`.

## 0.62.0 - 2026-10-19
- Relocated the kernel to a high-half image base at boot: a pre-relocation stage clones the firmware PML4, maps the image at `0xffff_9000_0000_0000` and a 1 GiB direct map at `0xffff_8000_0000_0000`, copies the loaded PE image, applies its `DIR64` base relocations, jumps high and drops every other PML4 slot.
- BootInfo ABI 2 reports the loader-chosen `image_base`/`image_size`, so self-relocation works regardless of where OVMF placed the image.
- Added a physical-access abstraction (`EarlyPhys` identity during table construction, direct-map `Identity` afterwards, `phys_to_virt`, page-walking `virt_to_phys`) and converted virtio-net/virtio-blk DMA, APIC/IOAPIC MMIO, initrd and VFS/process/seccomp frame access to it.
- Removed the `exec` slot-0 rejection: standard non-PIE static binaries linked at `0x400000` now run unmodified; user address-space clones keep the direct map and kernel image as supervisor mappings.
- Test stubs and `/init` now link at `0x400000`, and a `-no-pie` musl helper (`DIFF_NOPIE_OK`) verifies fork+exec of a low-address `ET_EXEC` binary.
- Fixed `virt_to_phys` to preserve the in-page offset, which had aliased DMA buffers within a page and broken ARP/network traffic.

## 0.61.0 - 2026-10-19
- Made pthreads work end to end under musl: `pthread_create`/`pthread_join`, contended mutexes, condition variables, `pthread_kill` and per-thread TLS.
- Implemented `CLONE_PARENT_SETTID`/`CLONE_CHILD_SETTID` tid writes (including correct `clone3` struct offsets), so musl's exiting threads release `__thread_list_lock` through the kernel's clear-child-tid write instead of deadlocking the first join.
- Added a per-task `tgid`: threads inherit their creator's group id, `gettid` returns the task id while `getpid` returns the group id, and `tkill`/`tgkill` deliver `pthread_kill` to the addressed thread.
- Fixed signal delivery from the syscall-exit path to use the live per-task stack frame, so a thread that blocked inside a syscall (for example in `nanosleep`) runs its handler instead of losing the signal against another task's stale frame.
- Stopped sending `SIGCHLD` for thread exits and switched futex waiters to `(uaddr, pid)` identity so woken waiters cannot lose their wakeup to a recycled wait-table slot.
- Reclaimed dead thread slots at the next thread spawn, folding each exited thread's CPU ticks into its thread-group leader like Linux does.
- Expanded native/guest differential tests with pthread create/join, condvar, pthread_kill and TLS cases.

## 0.60.0 - 2026-10-19
- Added per-process CPU-time accounting: the timer interrupt attributes each 1ms tick to user or kernel time from the interrupted CS.
- Implemented CPU clocks: `clock_gettime(CLOCK_PROCESS_CPUTIME_ID/CLOCK_THREAD_CPUTIME_ID)` now return accounted CPU time and `clock_getres` reports the tick resolution.
- Implemented real `getrusage` (`RUSAGE_SELF`, `RUSAGE_THREAD`, `RUSAGE_CHILDREN`) and `times()`, and `wait4` now returns the reaped child's usage in `struct rusage`, folded into the parent's children totals.
- Made `ITIMER_VIRTUAL` and `ITIMER_PROF` functional: they count user (and kernel) ticks, fire `SIGVTALRM`/`SIGPROF` with interval reload, and `setitimer`/`getitimer` accept all three timers.
- `/proc/<pid>/stat` reports utime/stime/cutime/cstime and fixes the priority/nice/num_threads field positions.
- Expanded native/guest differential tests with CPU-clock, rusage and virtual-timer cases.

## 0.59.0 - 2026-10-19
- Implemented POSIX record locks: `fcntl` `F_GETLK`/`F_SETLK`/`F_SETLKW` with per-process ownership, range replacement with trimming and splitting, owner pid reporting, blocking waits that return `EINTR` on signals, release when any descriptor of the inode closes and at process exit.
- Implemented open-file-description locks `F_OFD_GETLK`/`F_OFD_SETLK`/`F_OFD_SETLKW`, owned by the description so `dup`/`fork` share them and the last close releases them.
- Added robust-futex death handling at task exit: the robust list is walked, still-owned words get `FUTEX_OWNER_DIED` with the TID cleared, and a waiter is woken when `FUTEX_WAITERS` was set, mirroring Linux.
- Fixed `MAP_SHARED|MAP_ANONYMOUS` mappings so forked processes keep sharing the same frames instead of copy-on-write splitting.
- Expanded native/guest differential tests with record-lock, OFD-lock and robust-futex cases.

## 0.58.0 - 2026-10-18
- Implemented the POSIX timer family (`timer_create/settime/gettime/getoverrun/delete`) with signal notification, `si_value`, overrun counting and `TIMER_ABSTIME`.
- Made `alarm`, `getitimer` and `setitimer` share a per-process `ITIMER_REAL` timer; `ITIMER_VIRTUAL`/`ITIMER_PROF` report `EINVAL` pending CPU-time accounting.
- Added `clock_settime` plus a CMOS RTC seed for `CLOCK_REALTIME`, truthful clock-id classification in `clock_gettime`/`clock_getres`, and `TIMER_ABSTIME` support in `clock_nanosleep`.
- Implemented `rt_sigpending`, `rt_sigtimedwait` with timer siginfo delivery, and `pause` returning `EINTR`.
- Added `set_robust_list`/`get_robust_list` storage and round trip.
- Fixed the timer IRQ stub to align the kernel stack dynamically before the Rust handler; a `movaps` in the new tick code exposed the statically assumed alignment.
- Expanded native/guest differential tests with timer, itimer, clock, robust-list and sigpending cases.

## 0.57.0 - 2026-10-18
- Added per-open-file status flags: `O_NONBLOCK` for opens, pipes and sockets, live-`O_APPEND` writes, and `FD_CLOEXEC` cleared on `execve`.
- Implemented the descriptor-control surface of `fcntl` (`F_DUPFD`, `F_DUPFD_CLOEXEC`, `F_GETFD`/`F_SETFD`, `F_GETFL`/`F_SETFL`, `F_GETLK`) and flag handling in `open`/`openat`, `socket`/`accept4`, and `dup3`.
- Added `flock(2)` advisory locks with blocking and `LOCK_NB` contention, wake-on-release, and per-open-file sharing.
- Routed `sendto`/`recvfrom`/`sendmsg`/`recvmsg` through `MSG_DONTWAIT`.
- Fixed a latent fork→exec→wait kernel stack overflow by doubling kernel stacks to 64 KiB and adding a syscall-entry stack guard.
- Expanded native/guest differential tests with flock contention, nonblocking pipe and fcntl flag round trips, append-after-lseek content, and close-on-exec across `execve`.

## 0.56.0 - 2026-10-18
- Replaced placeholder `/proc` data with truthful per-process cmdline, environ, status, stat, statm and maps, including `/proc/<pid>` introspection.
- Added dynamic `/proc/self/fd` enumeration and readlink targets for pipes, sockets, devices, inodes and anon inodes; generalized readlink/readlinkat over all proc links.
- Derived system files from live state: meminfo (frame allocator), uptime (monotonic clock), loadavg and mounts (scheduler and mount registry).
- Implemented `madvise(MADV_DONTNEED|MADV_FREE)` page discard, `MAP_POPULATE` eager fault-in, and truthful `mincore` residency reporting.
- Expanded native/guest differential tests with procfs truth, madvise zeroing and mincore/populate residency cases.

## 0.55.0 - 2026-10-18
- Added per-process descriptor, open-file, VM-area/page and scheduler-state accounting with exit-time reclamation checks.
- Moved mount routing into the VFS and added namespace-cloned tmpfs mounts with longest-prefix path selection.
- Increased recycled-frame capacity to cover the configured 256 MiB guest and fixed process-exit cleanup failures.
- Implemented capget/capset, capability-checked identity changes, `PR_SET_NO_NEW_PRIVS`, and stricter seccomp filter validation and lifetime management.
- Hardened inotify watch matching, event records, overflow, close cleanup and create/modify delivery.
- Hardened io_uring parameter/ring bounds, descriptor lookup, read/write usercopy, and mapping lifetime; added native/guest NOP coverage.
- Expanded native/guest tests for errno behavior, `/proc/self/status`, capabilities, seccomp, inotify and io_uring; made the differential runner validate QEMU status and restore inputs on failure.

## 0.54.0 - 2026-10-18
- Expanded differential syscall coverage across file, directory, process, memory, time, identity and notification APIs.
- Added native versus guest execution comparison.
- Kept the full acceptance suite green.

## 0.53.0 - 2026-10-18
- Added a differential syscall test harness.
- Added tests for file, directory, process, stat, cwd and identity syscalls.
- Compared native and guest execution results.

## 0.52.0 - 2026-10-18
- Added per-namespace UTS hostname state.
- Added mount-namespace filtering for mount tables.
- Improved mount-aware path resolution.

## 0.51.0 - 2026-10-18
- Added per-namespace UTS hostname state.
- Improved sethostname propagation across namespaces.
- Delivered inotify events for filesystem changes.

## 0.50.0 - 2026-10-18
- Added read-only ext2 support on virtio-blk.
- Added /mnt integration and ext2 file reads.
- Parsed ext2 superblocks, group descriptors, inodes and directories.

## 0.49.0 - 2026-10-18
- Added io_uring ring allocation and parameter reporting.
- Added synchronous NOP, read and write submission processing.
- Added completion queue entry posting.

## 0.48.0 - 2026-10-18
- Added classic BPF evaluation for seccomp filters.
- Supported seccomp allow, errno, trap and kill actions.
- Validated and stored user-supplied filter programs.

## 0.47.0 - 2026-10-18
- Added per-process hostname state and sethostname support.
- Added seccomp strict-mode enforcement.
- Reflected hostname changes in uname.

## 0.46.0 - 2026-10-18
- Added a mount table and mount-aware path resolution.
- Implemented mount and umount2 handling.
- Enabled bind-style path translation through mountpoints.

## 0.45.0 - 2026-10-18
- Added pidfd_open and pidfd_send_signal.
- Made private file-backed mmap writable.
- Added setsockopt, getsockopt, sendmsg, recvmsg, accept4, sendmmsg and recvmmsg.

## 0.44.0 - 2026-10-18
- Added /proc/self/exe, root and cwd symlinks.
- Added /proc/self/ns with namespace identifier links.
- Improved process introspection through readlink.

## 0.43.0 - 2026-10-18
- Added setsockopt, getsockopt, sendmsg, recvmsg and accept4.
- Aggregated iovec buffers in sendmsg/recvmsg.
- Accepted common socket option calls.

## 0.42.0 - 2026-10-18
- Expanded `/sys` with class, block, device and kernel entries.
- Added network interface and block device sysfs attributes.
- Added `sethostname` and hostname reporting through `uname`.

## 0.41.0 - 2026-10-18
- Added virtio-blk legacy driver and `/dev/vda`.
- Added block device VFS/file operations and seek support.
- Added synchronous disk read/write through virtio descriptor chains.

## 0.40.0 - 2026-10-18
- Added credentials, groups, capabilities, umask and namespace state to processes.
- Added setuid/setgid/setreuid/setregid/setresuid/setresgid/getgroups/setgroups/capget/capset/unshare/setns/seccomp syscalls.
- Added netlink sockets, inotify and signalfd support.
- Added a `/sys` virtual filesystem with kernel and device entries.
- Propagated namespace flags through clone and fork.

## 0.39.0 - 2026-10-18
- Added symlink-aware path resolution, `symlink`, `link`, and `readlink`.
- Added a minimal `/proc` filesystem with common self and system files.
- Added `timerfd_create`, `timerfd_settime`, and `timerfd_gettime`.
- Extended file descriptors with `TimerFd`.

## 0.38.0 - 2026-10-18
- Added root `/` VFS node support and working `opendir`/`readdir` on `/`.
- Added compatibility syscalls/stubs: statfs, fstatfs, readlink, close_range, mincore, fallocate, sendfile, sync, getcpu, sched_setaffinity, mlock2, personality, nice, sync_file_range, xattr, fadvise, inotify/timerfd/signalfd stubs.
- Implemented `fallocate` growth and descriptor-to-descriptor `sendfile`.

## 0.37.0 - 2026-10-18
- Added cwd-aware path resolution across VFS syscalls and process state.
- Added filesystem syscalls: mkdir, rmdir, rename, chmod, fchmod, truncate, ftruncate, stat, lstat, unlink, dup.
- Added process/session syscalls: setpgid, getpgid, setsid, getsid, prctl, sysinfo.
- Added time/select/rlimit syscalls: gettimeofday, getrlimit, setrlimit, getrusage, times, select, pselect6, clock_getres, clock_nanosleep, utimensat.
- Added read-only file-backed mmap support.
- Extended libc wrappers and headers accordingly.

# Changelog

## 0.36.0 - 2026-10-18
- Added a CAM PCI scanner with multi-function and 64-bit BAR support plus x86-64 port-I/O primitives.
- Added a legacy virtio-net driver: split rings, RX recycling, synchronous TX completion, and the mandatory 10-byte legacy header.
- Added an Ethernet/ARP/IPv4/ICMP/UDP stack and a minimal TCP client (go-back-N, RTO, FIN shutdown) in `kernel/net.rs`.
- Wired `AF_INET` into the socket layer and Linux ABI: `socket`, `bind`, `connect`, `sendto`, `recvfrom`, `getsockname`, `getpeername`, `shutdown`, with `sockaddr_in` translation.
- Switched the acceptance guest to `virtio-net-pci` with QEMU user networking and added `tools/net-echo.py`, a dual UDP/TCP host echo server.
- Added `/bin/hello-net` (musl) exercising UDP and TCP echo round trips, chained after `/bin/hello-socket`.

## 0.28.0 - 2026-10-18
- Added a Linux signal ABI foundation: `kill`, `rt_sigaction`, `rt_sigprocmask` and `rt_sigreturn` syscalls in `kernel/signal.rs`.
- Added per-process pending and blocked signal masks to `Process`; `fork` preserves them, `execve` resets them.
- Added `libc/signal.c` with `kill`, `sigaction`, `sigprocmask` and `sigreturn` wrappers plus a `libc/include/signal.h` header.
- Added `signal_pending_check` before `SYSRET` in `arch/x86_64/syscall.S` and a `signal_deliver_asm` trampoline in `arch/x86_64/user.S`.
- Signal delivery is gated on a pending mask check; full handler-frame delivery lands in the next milestone.

## 0.27.0 - 2026-10-17
- Added a per-process address-space and descriptor owner (`kernel/process.rs`) with deep-copy `fork` of the private PML4 slots and child resume at the saved syscall return with `rax=0`.
- Added blocking pipes (`kernel/pipe.rs`) with 4 KiB ring buffers, EOF on last writer close and `EPIPE` on last reader close.
- Reworked `kernel/fd.rs` into reference-counted open-file descriptions with per-process descriptor tables shared correctly across `fork`, `dup2`/`dup3`, `pipe`/`pipe2` and a minimal `fcntl`.
- Added `clone` (fork subset), `wait4` with wait-status encoding, `WNOHANG`/`WUNTRACED`, process-group selectors and `ECHILD`.
- Added `ECHILD`, `EMFILE` and `EPIPE`; `sched_yield` now really yields; `execve` keeps descriptors open and the user stack grew to 8 pages.
- Added `libc/` `errno` support (`__errno_location`) and `fork`, `pipe`, `dup2`, `waitpid` wrappers; `/bin/hello-posix` exercises fork, pipe, dup2 and wait4 end to end.

## 0.26.0 - 2026-10-17
- Added a clean-room static C runtime under `libc/`: freestanding startup, Linux syscall shim, string functions, printf subset, first-fit malloc family, getenv and file I/O wrappers.
- Added `/bin/hello-c`, a C acceptance program compiled with Clang/LLD only and chained after `/bin/hello` through Linux `execve`.
- Extended the linker script with large-code-model input sections and the acceptance gate with C runtime markers.

## 0.2.0 - 2026-10-12
- Added versioned loader/kernel BootInfo ABI and memory-map records.
- Added bounded physical frame allocator with overflow checks.
- Added dependency-free spin lock.
- Added x86-64 MSR read/write primitives and syscall MSR constants.
- Replaced direct syscall pointer dereferences with chunked usercopy boundaries.
- Added early security invariants and milestone 2A roadmap entry.

## 0.3.0 - 2026-10-12
- Centralized Linux x86-64 syscall numbers and UAPI structure layouts.
- Added identity, PID/TID, close, fstat, brk, arch_prctl, set_tid_address, robust-list and prlimit bootstrap behavior.
- Added FS-base process state needed by libc thread-local storage setup.
- Added System V AMD64 auxiliary-vector builder for future ELF exec.
- Kept unsupported memory, signal, futex, filesystem and clock calls explicit as ENOSYS rather than faking success.

## 0.4.0 - 2026-10-12
- Replaced alignment-sensitive ELF casting with a bounds-checked byte parser.
- Added strict ELF64 PT_LOAD planning, overflow checks, interpreter detection and entry validation.
- Added a fixed-capacity userspace VMA ledger with placement, overlap checks, protection and removal.
- Added anonymous mmap, mprotect and munmap ABI paths without pretending hardware PTEs already exist.
- Added ENODEV and EEXIST errno values.

## 0.5.0 - 2026-10-12
- Added a complete allocation-free AMD64/Linux initial stack builder for argc, argv, envp and auxv.
- Added AT_RANDOM, AT_EXECFN and ELF metadata placement with bounds and NUL checks.
- Added a transaction-friendly ELF execution planner with page-aligned PT_LOAD mappings and a guarded user stack reservation.
- Added E2BIG and explicit limits for bootstrap argument and environment vectors.

## 0.6.0 - 2026-10-12
- Added a freestanding C/assembly UEFI application entry and BOOTX64.EFI build path.
- Added UEFI GetMemoryMap allocation, descriptor translation and bounded ExitBootServices retry loop.
- Connected validated BootInfo to the physical allocator and CPU descriptor initialization path.
- Added x86-64 four-level page-table map, unmap, protect, CR3 activation and INVLPG primitives.
- Added GDT, long-mode TSS, ring-0 stack, IST stack and IDT construction.
- Added fatal default, double-fault and page-fault assembly entry paths with CR2 diagnostics.

## 0.7.0 - 2026-10-12
- Added IA32_EFER/STAR/LSTAR/FMASK programming for 64-bit SYSCALL.
- Added assembly syscall entry, user-RSP capture, kernel-stack switch, register preservation and Rust dispatch.
- Added lower-canonical SYSRET RIP/RSP validation and fatal fallback.
- Added an iretq ring-3 entry trampoline with user selector and context validation.
- Added a no-libc static Linux acceptance ELF using only write and exit_group.

## 0.8.0 - 2026-10-12
- Added a COFF Rust-kernel build path and single-image BOOTX64.EFI link pipeline.
- Embedded the static Linux acceptance ELF into the UEFI image and exported it through BootInfo initrd fields.
- Added current-CR3 translation and bootstrap identity physical access.
- Added actual PT_LOAD frame allocation, user PTE installation, segment copying and BSS zeroing.
- Added a mapped one-page user stack, Linux initial stack copy, and automatic iretq entry.
- Made assembly/Rust boundaries explicitly SysV64 so the same kernel can be linked into a Microsoft-ABI UEFI image.

## 0.9.0 - 2026-10-12
- Replaced numeric-only user pointer checks with page-table-backed usercopy validation.
- Added effective USER/WRITABLE/PRESENT permission checks and page-boundary chunking.
- Added bounded physical-frame recycling with duplicate-free detection.
- Added transactional ELF page installation and rollback of PTEs and frames on failure.
- Removed unchecked byte writes from the ELF copy path.

## 0.10.0 - 2026-10-12
- Added a per-process bootstrap CR3 cloned from the live firmware root.
- Cleared dedicated PML4 slots before installing the Linux image and user stack.
- Moved the acceptance ELF to a separate 4 TiB PML4 slot.
- Activated the process CR3 immediately before the ring-3 IRET transition.
- Rejected ELF layouts that share the identity slot or cross the private image slot.

## 0.11.0 - 2026-10-12
- Added process address-space lifetime tracking across the CR3 switch.
- Added recursive teardown for private PML4 slots, including user frames and intermediate page tables.
- Restored the bootstrap CR3 before reclaiming the process root.
- Connected exit_group to address-space reclamation before CPU halt.
- Added a single-core QEMU/OVMF serial acceptance runner.

## 0.12.0 - 2026-10-12
- Added containment for user-mode CPU faults with process-only termination and address-space reclamation.
- Added explicit general-protection, page-fault and double-fault entry layouts.
- Recorded process exit status and terminating exception vector.
- Strengthened the Linux acceptance ELF to verify invalid write pointers return raw `-EFAULT`.
- Added distinct success and failure messages and nonzero failure exit status.

## 0.13.0 - 2026-10-12
- Corrected the Microsoft-x64 to SysV64 loader transition stack alignment.
- Added dedicated error-code exception stubs for invalid TSS, segment-not-present, stack-segment and alignment-check faults.
- Cleared DF before entering Rust exception handling.
- Added source consistency and host toolchain preflight scripts.
- Included compiler-builtins memory intrinsics in the cross-target Rust build.

## 0.14.0 - 2026-10-12
- Added QEMU isa-debug-exit signaling for deterministic guest success, ABI failure and user-fault outcomes.
- Added a timeout-bounded acceptance harness that captures and validates serial output.
- Added automated checks for EFAULT success, zero exit status and full address-space reclamation.
- Updated the QEMU runner with the debug-exit device.

## 0.15.0 - 2026-10-12
- Added UART transmitter-ready polling instead of writing bytes blindly.
- Added CRLF normalization for serial terminals.
- Added bounded serial drain before QEMU debug-exit.
- Protected acceptance markers from being lost when the VM exits immediately after process teardown.

## 0.16.0 - 2026-10-12
- Added CPUID detection for SYSCALL/SYSRET, NX, 1 GiB pages, SMEP and SMAP.
- Rejected CPUs that cannot support the required Linux x86-64 entry and NX model.
- Enabled CR0.WP so supervisor writes respect read-only PTEs.
- Added boot diagnostics for detected CPU protections.

## 0.17.0 - 2026-10-12
- Located ACPI RSDP through UEFI configuration tables and passed it through BootInfo.
- Added checksum-validated XSDT/RSDT and MADT discovery.
- Parsed Local APIC, IOAPIC, interrupt-source overrides and LAPIC address overrides.
- Initialized xAPIC, masked the legacy PIC and all IOAPIC redirection entries.
- Added a periodic Local APIC timer vector, full register-preserving IRQ entry and EOI processing.
- Enabled IF in the initial ring-3 RFLAGS.
- Added an interrupt-disabling spin lock for state shared with IRQ handlers.

## 0.18.0 - 2026-10-12
- Added fixed-capacity task descriptors with dynamic PID allocation and task states.
- Added per-task 32 KiB kernel stacks and a SysV64 callee-state context switch.
- Added a round-robin runnable queue, cooperative yield, sleeping tasks and timer wakeups.
- Added timer-driven `need_resched` bookkeeping without unsafe switching inside interrupt context.
- Added kernel-thread creation and a two-thread context-switch self-test.

## 0.19.0 - 2026-10-12
- Added per-task CR3, FS base, TSS.RSP0 and SYSCALL kernel-stack state.
- Applied architecture state before every cooperative context switch.
- Connected `arch_prctl(ARCH_SET_FS)` to the real IA32_FS_BASE MSR.
- Corrected the bootstrap task to use the actual 64 KiB ring-0 stack rather than an unused scheduler stack.
- Extended the scheduler test to verify FS-base isolation across two kernel threads.

## 0.20.0 - 2026-10-12
- Added parent PID tracking and a real task-backed `getppid` result.
- Added fixed-capacity wait queues with block, wake-one and wake-all operations.
- Added scheduler APIs for blocking and waking tasks by PID.
- Added child zombie lookup, exit-status collection and task-slot reuse.
- Extended the scheduler self-test to reap both child kernel threads.

## 0.21.0 - 2026-10-12
- Replaced direct embedded-ELF boot with a read-only initramfs containing `/init` and `/etc/motd`.
- Added checksum-independent, bounds-checked NSAR initramfs parsing and absolute path lookup.
- Added read-only inode metadata and offset-based file reads.
- Added a 32-entry bootstrap FD table with console descriptors and independent file offsets.
- Implemented `openat`, regular-file `read`, `close`, `lseek`, `fstat`, and `newfstatat` paths.
- Extended the Linux acceptance ELF to open and print `/etc/motd` before exiting.

## 0.22.0 - 2026-10-12
- Connected anonymous private `mmap` to physical-frame allocation and user PTE creation.
- Zero-filled every newly allocated anonymous and brk page before making it accessible.
- Connected `mprotect` to hardware PTE writable/NX permissions and TLB invalidation.
- Connected `munmap` to PTE removal and physical-frame recycling.
- Connected upward `brk` growth to mapped read/write heap pages.
- Added rollback when multi-page anonymous mapping fails partway through allocation.
- Extended the ring-3 acceptance program to read/write mmap and brk memory.

## 0.23.0 - 2026-10-13
- Added Linux x86-64 `execve` dispatch for static initramfs ELF files.
- Added bounded, usercopy-backed path, argv, and envp capture.
- Generalized the transactional ELF loader to construct a caller-supplied initial stack.
- Added atomic address-space replacement after the new ELF and stack are fully built.
- Added syscall-return redirection so successful exec resumes at the new entry/RSP and never returns to the old image.
- Added `/bin/hello`; `/init` now execs it with one argument and one environment variable.
- Added ring-3 validation of argc, argv, and envp after exec.

## 0.24.0 - 2026-10-13
- Integrated boot user ELF as a normal runnable child task with its own kernel stack and CR3.
- Returned user exit and faults to the scheduler as zombie tasks, then reaped them from PID 1.

## 0.25.0 - 2026-10-13
- Added bootstrap clock_gettime, nanosleep, and getrandom syscall paths.
- Extended the ring-3 acceptance program with time, sleep, and random checks.

## 0.25.2 - 2026-10-15
- Renamed the project to Slop/MNU: Synthetic Low-quality Operating Platform / Maybe Not Unix.
- Renamed package, target, symbols, artifacts, boot ABI, uname identity, initramfs messages, tests, and documentation.
