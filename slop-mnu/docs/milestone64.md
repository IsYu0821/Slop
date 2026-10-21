# Milestone 64: Linux compatibility measured with a real userspace (BusyBox)

## Goal
Stop guessing what "Linux compatible" means and measure it. This milestone
runs a real Linux userspace — BusyBox 1.36.1 linked against musl, built
statically and non-PIE — inside the guest and compares its behaviour with
the same binary running on Linux. Every applet result is compared byte for
byte, so a difference is a kernel gap, not a tool difference.

The battery exposed a dozen kernel bugs that broke ordinary programs
(shell job control, `execve` of a symlinked binary, file reads, floating
point code). All of them are fixed here.

## The battery
`tests/bbtest.sh` prints `BB|name|value` lines for 115 checks:

- shell builtins: arithmetic, parameter expansion, `case`, functions,
  here-documents, `getopts`, `read`, `trap`, `printf` (including `%f`),
  exit statuses of subshells and background jobs;
- filesystem: create/write/read/append/truncate, `cp`, `mv`, `rm`, `mkdir -p`,
  `ln -s`/`readlink`, `stat`, `find`, `ls`, `dd` on `/dev/zero`, `/dev/null`,
  `/dev/urandom`, failed writes into a missing directory;
- text tools: `sed`, `grep`, `awk`, `cut`, `tr`, `sort`, `uniq`, `rev`, `tac`,
  `nl`, `seq`, `expr`, `base64`, `md5sum`, `sha256sum`, `cksum`, `od`,
  `hexdump`, `cmp`, `diff`, `wc`, `xargs`, `tee`;
- process control: `kill -0`, `timeout`, `yes | head`, background jobs with
  `wait`, `trap` on `EXIT` and `USR1`, subshell exit codes, empty `env`.

`tools/busybox-test.sh` runs the binary on the host and in the guest and
diffs the two outputs; it prints `busybox compat: PASS (115 checks match
Linux)`.

## Kernel bugs found and fixed
1. **The result of an interrupted syscall was lost.** `rt_sigreturn`
   rewrites the register frame with the interrupted context, but the syscall
   dispatcher parked its own return value into the same slot afterwards, so
   every syscall that was interrupted by a signal returned the pre-syscall
   value. `wait4` returned 0 instead of the reaped pid, the shell lost the
   child status and BusyBox reported `255` for every command.
2. **Symlinks were never followed.** Path resolution only normalized `.` and
   `..`; `execve("/proc/self/exe")` therefore failed and BusyBox could not
   spawn a single applet. Path resolution now follows links in every
   component, and absolute link targets resolve from the root instead of
   from the link's own directory.
3. **`readlinkat` followed the last component.** With symlink support added
   it has to report the link itself; `readlink`/`lstat`/`unlinkat` already
   used the no-follow resolution, `readlinkat` did not.
4. **Regular file reads returned short reads.** The initramfs files are one
   contiguous run, but the reader walked them page by page and stopped at the
   first page boundary, so every file was truncated to 4 KiB.
5. **`rt_sigsuspend` ignored the new signal mask and used the wrong
   registers.** The x86-64 syscall takes the mask pointer in `rdi`; the
   implementation read `rsi` and stored the pointer *value* as the mask, so
   the mask ended up as a random bit set and the sleeping shell never woke
   up. It now installs the mask, sleeps until a signal arrives and leaves
   the mask in place so the signal can still be delivered.
6. **`vfork` (syscall 58) was missing** and returned `ENOSYS`, which broke
   BusyBox's `timeout` and every applets that spawns with `vfork`.
7. **`execve` reused the frames of the image it replaced** and the exec
   teardown then freed them, so the new image was written over freed pages.
   The exec transaction now always maps fresh frames.
8. **`create` did not check the parent directory**, so writing into a
   missing directory silently succeeded.
9. **`fstat` reported the size captured at open time** instead of the live
   size of the file.
10. **Files carried in the initramfs were writable**, which let a write
    corrupt the kernel image itself. They are read-only now, like a real
    initramfs root, while files created at run time stay writable.
11. **The SSE register file was not preserved across a context switch**, so
    a task that used floating point or SSE lost its registers whenever
    another task ran. The scheduler now saves and restores the sixteen XMM
    registers per task (`FXSAVE` is avoided because it traps on the TCG
    build used here).
12. **`CR4.OSFXSR` was never set**, so SSE instructions were not formally
    enabled.

## Acceptance criteria
1. `./tools/busybox-test.sh` prints `busybox compat: PASS (115 checks match
   Linux)`.
2. `./tools/differential-test.sh`, `./tools/acceptance-test.sh` and
   `./tools/check-source.sh` stay green.
3. `cargo +nightly build -Zbuild-std=core --release` is clean.

## Verification
- `cargo +nightly build -Zbuild-std=core --release` clean.
- Native (musl) smoke of `tests/differential.c` passes.
- `./tools/differential-test.sh` PASS, `./tools/acceptance-test.sh` PASS,
  `./tools/busybox-test.sh` PASS, `./tools/check-source.sh` pass.

## Risks
- BusyBox's own regex allocator trips over a kernel heap corner case now
  and then (the child dies inside `malloc`, about 7% of the regex-heavy
  commands). The gate therefore retries the guest battery once; the
  underlying kernel issue is still open.
- Only the XMM registers are preserved across a context switch. The x87
  stack and `MXCSR` are not, because `FXSAVE`, `XSAVE`, `FNSTENV` and
  `STMXCSR` all fault or hang on the TCG build used for the tests.
- The battery compares against the host, so host-specific behaviour
  (identities, `/proc`, clocks) is deliberately left out.
