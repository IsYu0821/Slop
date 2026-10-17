# Milestone 27: real processes, pipes and wait4

The acceptance chain now runs a multi-process C program. `/bin/hello-posix` creates a pipe, forks, and the child maps the pipe write end onto stdout with `dup2`, writes `posix: child message 42` and exits with status 7. The parent reads the pipe to EOF, compares the bytes exactly, reaps the child with `wait4` and verifies `WIFEXITED`/`WEXITSTATUS`. The in-tree `libc/` grew a minimal `errno` (`__errno_location` plus wrappers that translate negative returns to `-1`/`errno`) and `fork`, `pipe`, `dup2` and `waitpid` wrappers.

## Process model

- `kernel/process.rs` owns the complete per-process state: CR3 and bootstrap CR3, image/stack/mmap PML4 slots, the VMA map, brk, the clear-child-TID pointer and the file-descriptor table. Teardown activates the bootstrap CR3 first, then destroys the private slots and frees the root.
- `fork` deep-copies the three private PML4 slots so parent and child have fully separated address spaces (no copy-on-write yet). The child resumes at the saved syscall return address with `rax=0` through `resume_user_asm` in `arch/x86_64/user.S`.
- Tasks gained a process group ID, a terminating-signal field and a wake-pending latch. Wait queue users register before blocking, and the latch closes the lost-wakeup window between registration and sleep.
- `wait4` supports positive PIDs, `-1`, `0` (own process group) and `<-pid`, with `WNOHANG` and `WUNTRACED`. Exit status uses the Linux wait encoding, including signal termination, and `ECHILD` is returned when no child matches.

## File descriptors and pipes

- `kernel/fd.rs` now stores shared open-file descriptions with reference counts. Descriptor tables are per-process, while offsets and pipe ends follow the shared description. `fork` increments references and teardown releases them.
- `kernel/pipe.rs` implements 16 fixed pipes with 4 KiB ring buffers and blocking `read`/`write`: EOF when the last writer closes, `EPIPE` when the last reader closes. `SIGPIPE` is not delivered yet.
- `dup2`/`dup3`, `pipe`/`pipe2`, and a minimal `fcntl` (only `F_DUPFD` and the no-op descriptor-flag forms) complete the descriptor surface. `execve` keeps descriptors open across an image replacement, now with an 8-page (32 KiB) user stack.

## Deliberate limits

No `CLONE_THREAD`/`CLONE_VM` (threads), no copy-on-write, no signals beyond `fork`'s `SIGCHLD` bookkeeping, no `SIGPIPE`, no `select`/`poll`/`epoll`, and fixed table sizes (8 tasks, 32 FDs per process, 64 open descriptions, 16 pipes). Returning `ENOSYS` or `EINVAL` remains preferable to fabricating behavior.
