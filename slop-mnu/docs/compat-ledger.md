# Compatibility provenance ledger

Every externally observed constant, structure layout and behavior receives an entry with: interface, architecture/version, public specification or black-box test source, implementer, reviewer and tests. No implementation contributor may consult Linux implementation source for the same component during a clean-room reimplementation.

Initial ABI facts used in this scaffold:
- ELF64 little-endian x86-64 machine ID 62.
- Linux x86-64 syscall numbers: write=1, getpid=39, exit=60, uname=63, exit_group=231.
- Linux kernel errors are represented as negative errno at the raw syscall boundary.

C runtime facts used by the in-tree `libc/`:
- ISO C function names and signatures (`memcpy`, `memmove`, `memset`, `memcmp`, `strlen`, `strcmp`, `strncmp`, `strchr`, `strcpy`, `malloc`, `calloc`, `realloc`, `free`, `getenv`, and the `printf` subset): public specification ISO/IEC 9899; implemented clean-room by sloppy-coder0821 from function semantics only. Reviewer: pending. Tests: `/bin/hello-c` checks in `tools/acceptance-test.sh`.
- Linux x86-64 syscall numbers used by the wrappers (`read`=0, `write`=1, `close`=3, `mmap`=9, `openat`=257, `exit_group`=231): public Linux man-pages. Implementer: sloppy-coder0821. Reviewer: pending. Tests: C runtime acceptance chain.
- Wrapper constants `AT_FDCWD`=-100, `O_RDONLY`=0, `PROT_READ`=1, `PROT_WRITE`=2, `MAP_PRIVATE`=0x02, `MAP_ANONYMOUS`=0x20: public Linux man-pages. Implementer: sloppy-coder0821. Reviewer: pending. Tests: `openat`/`mmap` checks in `/bin/hello-c`.

Process, pipe and descriptor facts (v0.27):
- Linux x86-64 syscall numbers `pipe`=22, `dup2`=33, `clone`=56, `wait4`=61, `fcntl`=72, `dup3`=292, `pipe2`=293: public Linux man-pages. Implementer: sloppy-coder0821. Reviewer: pending. Tests: `/bin/hello-posix` checks in `tools/acceptance-test.sh`.
- `fork` is modeled as `clone` with `exit_signal` `SIGCHLD`=17 and the remaining arguments zero: public Linux man-pages. Implementer: sloppy-coder0821. Reviewer: pending. Tests: `fork` plus `wait4` round trip in `/bin/hello-posix`.
- Wait status encoding: exited child `WIFEXITED`/`WEXITSTATUS` (`status>>8&0xff`), signal death `WIFSIGNALED`/`WTERMSIG` (`status&0x7f`), `WNOHANG`=1, `WUNTRACED`=2, `ECHILD`=10: public Linux man-pages and `sys/wait.h`. Implementer: sloppy-coder0821. Reviewer: pending. Tests: `/bin/hello-posix` verifies exit status 7 and a user-fault status of 128+11 in earlier milestones.
- Pipe semantics: EOF on `read` when the last write description closes, `EPIPE` when the last read description closes (no `SIGPIPE` yet), shared offsets and pipe ends across `fork`, per-process descriptor tables: public Linux man-pages and pipe(7). Implementer: sloppy-coder0821. Reviewer: pending. Tests: pipe read-to-EOF and content match in `/bin/hello-posix`.
- `pipe2` accepts `O_CLOEXEC`=0x80000 and ignores it (no exec-time close is implemented yet); `fstat` reports pipes as `S_IFIFO`=0x1000: public Linux man-pages. Implementer: sloppy-coder0821. Reviewer: pending. Tests: `/bin/hello-posix` writes through a `dup2`-mapped pipe descriptor.

Signal ABI facts (v0.29):
- Linux x86-64 syscall number `rt_sigpending`=127: public Linux man-pages. Implementer: sloppy-coder0821. Reviewer: pending. Tests: `/bin/hello-signal` checks in `tools/acceptance-test.sh`.
- `struct k_sigaction` field order is handler, flags, mask, restorer, each a machine word: public Linux man-pages for `rt_sigaction` and the published kernel interface description. Implementer: sloppy-coder0821. Reviewer: pending. Tests: the `sigaction` wrapper in `libc/signal.c` and the handler check in `/bin/hello-signal`.
- `SA_NOCLDSTOP`=1, `SA_NOCLDWAIT`=2, `SA_SIGINFO`=4, `SA_RESTORER`=0x04000000, `SA_ONSTACK`=0x08000000, `SA_RESTART`=0x10000000, `SA_NODEFER`=0x40000000, `SA_RESETHAND`=0x80000000 and `SIG_DFL`=0, `SIG_IGN`=1: public Linux man-pages. Implementer: sloppy-coder0821. Reviewer: pending. Tests: the ignored signal and the blocked signal checks in `/bin/hello-signal`.
- A handler installed without `SA_RESTORER` is rejected with `EINVAL`, and the restorer performs the `rt_sigreturn` syscall: public Linux man-pages for `sigaction` and the x86-64 ABI note that a restorer is mandatory. Implementer: sloppy-coder0821. Reviewer: pending. Tests: the handler round trip in `/bin/hello-signal` through `libc/signal.S`.
- The signal frame places the restorer address at the bottom, the `ucontext_t` above it and the `siginfo_t` above that; the handler is entered with the signal number in RDI, the siginfo pointer in RSI and the ucontext pointer in RDX: public Linux man-pages for `sigaction` and the published x86-64 signal frame description. Implementer: sloppy-coder0821. Reviewer: pending. Tests: the siginfo and ucontext checks in `/bin/hello-signal`.
- The `ucontext_t` layout is flags, link, stack, machine context, signal mask and the floating-point area; the machine context is the `sigcontext` with the register order named by the published `REG_*` index list (`REG_R8`=0 through `REG_CR2`=22): public libc interface description of `ucontext_t` and the published `REG_*` index list. Implementer: sloppy-coder0821. Reviewer: pending. Tests: the instruction pointer check in `/bin/hello-signal` reads `uc_mcontext.gregs[REG_RIP]`.
- `siginfo_t` starts with `si_signo`, `si_errno` and `si_code`; a signal sent by a process reports `si_code`=`SI_USER`=0 with the sender pid and uid in the `_kill` member: public Linux man-pages for `sigaction` and `kill`. Implementer: sloppy-coder0821. Reviewer: pending. Tests: the siginfo code and sender checks in `/bin/hello-signal`.
- Default actions: `SIGCHLD` and `SIGCONT` are ignored, `SIGSTOP` and `SIGTSTP` stop the task, every other signal terminates it: public Linux man-pages for `signal`. Implementer: sloppy-coder0821. Reviewer: pending. Tests: the `SIGKILL` and `SIGSTOP`/`SIGCONT` checks in `/bin/hello-signal`.
- A blocking `wait4` or pipe read that is interrupted by a queued signal reports `EINTR`=4: public Linux man-pages for `wait` and `read`. Implementer: sloppy-coder0821. Reviewer: pending. Tests: the parent wait in `/bin/hello-signal` observes the interrupted wait.
- `fork` clears the pending signal set of the child and `execve` clears the pending set and the signal mask: public Linux man-pages for `fork` and `execve`. Implementer: sloppy-coder0821. Reviewer: pending. Tests: the handler count checks in `/bin/hello-signal` run across a fork and an exec chain.
- The x86-64 `SYSRET` instruction loads RFLAGS from RAX and `IRETQ` restores RIP, CS, RFLAGS, RSP and SS while leaving RAX unchanged: AMD64 architecture manual, public processor specification. Implementer: sloppy-coder0821. Reviewer: pending. Tests: the whole acceptance chain returns from every syscall with the result in RAX and the user flags intact.

Before release, legal counsel must approve the clean-room protocol, licenses, trademarks and distribution process.
