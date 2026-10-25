# Milestone 73: stopped children and the floating-point ucontext

## Goal

Linux has two process facilities the kernel did not provide: a stopped
state a parent can observe, and the floating-point registers in the signal
context. The differential battery now covers both, and the guest matches
Linux on both.

## The stopped state

A task stopped by `SIGSTOP`, `SIGTSTP`, `SIGTTIN` or `SIGTTOU` with the
default disposition parks in a new `Stopped` scheduler state instead of
running. The parent learns about it through `waitpid` with `WUNTRACED`
(`WIFSTOPPED` with `WSTOPSIG`) and about the resume through `waitpid`
with `WCONTINUED` (`WIFCONTINUED`, status `0xffff`). `waitid` reports
`CLD_STOPPED` and `CLD_CONTINUED` with the same selection flags. The
parent receives `SIGCHLD` on stop and continue unless it asked for
`SA_NOCLDSTOP`.

`SIGSTOP` cannot be blocked, handled or ignored and always stops; `SIGCONT`
resumes a stopped task and is consumed with the default disposition, or
queued for a handler. `SIGTSTP`/`SIGTTIN`/`SIGTTOU` stop only with the
default disposition and when unblocked. A blocked stop stays pending.

The fix also corrects `pending()`: it masked `SIGKILL` and `SIGSTOP` out
of the deliverable set, so neither was ever delivered through the normal
path. The mask is now `sigmask & !unblockable`, and `has_deliverable_signal`
treats a stop disposition as deliverable so a blocked waiter parks
correctly.

## The floating-point ucontext

The x86-64 `ucontext_t` carries a 512-byte FXSAVE area. The kernel keeps
only the sixteen SSE registers; they are published at FXSAVE offset 160
like Linux, with `MXCSR` set to its reset value `0x1f80` at offset 24 and
`fpregs` pointing at the area. A handler that writes the area sees its
registers restored through `rt_sigreturn`, so an SSE pattern written in
the handler survives the return.

User SSE registers now survive system calls: the syscall entry parks them
in the running task's per-task FPU area (`CURRENT_FPU`) before the kernel
uses XMM registers, and restores them before `iretq`. The same area feeds
the ucontext on delivery and receives the handler-approved registers on
return; context switches keep using it, so no `FXSAVE` trap is needed on
TCG builds.

## Acceptance criteria

1. `./tools/acceptance-test.sh` prints `acceptance: PASS`.
2. `./tools/differential-test.sh` prints `differential test: PASS`.
3. `./tools/busybox-test.sh` prints `busybox compat: PASS (115 checks match
   Linux)`.
4. `./tools/heap-test.sh` prints `heap: PASS`.
5. `./tools/check-source.sh` passes.
6. `cargo +nightly build -Zbuild-std=core --release` is clean.

## Verification

- `cargo +nightly build -Zbuild-std=core --release` clean.
- `./tools/acceptance-test.sh` PASS.
- `./tools/differential-test.sh` PASS, three runs in a row, with new
  `DIFF_STOP_OK` and `DIFF_FP_OK` markers on native Linux and in the guest.
- `./tools/busybox-test.sh` PASS, 115 checks match Linux.
- `./tools/heap-test.sh` PASS.
- `./tools/check-source.sh` pass.
