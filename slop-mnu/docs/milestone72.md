# Milestone 72: the real-time signals and the alternate stack

## Goal

Linux has two signal facilities that the kernel did not provide: a range of
signals that carry a value and are queued, and a stack a handler can run on
of its own. The differential battery now covers both, and the guest matches
Linux on both.

## The real-time range

The signals from 32 to 64 are queued. A real-time signal that is sent
carries a value, and more than one of them can wait at the same time, so a
handler that runs once per signal can read them all.

The kernel keeps a queue for each signal number, up to a small depth, and
the value of the front of the queue travels in the `siginfo` the kernel
builds, at the offset the C library reads as `si_value`. A signal outside
the range is refused with `EINVAL`, which is what Linux does.

The C library sends the signal with `sigqueue`, which on Linux calls
`rt_sigqueueinfo` with the published information, so the kernel reads the
value from the information a program passes and ignores the other fields.

## The alternate stack

A program asks for a stack with `sigaltstack`, and a handler that asked for
one with `SA_ONSTACK` runs on it. The kernel builds the frame on the
alternate stack, marks it as in use while the handler runs, and frees it
when the handler returns. The program sees the address of the handler on the
stack it asked for, and the kernel reports the size it asked for.

A stack that is too small is refused with `ENOMEM`, which is what Linux
does.

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
- `./tools/differential-test.sh` PASS, twenty four runs in a row.
- `./tools/busybox-test.sh` PASS, 115 checks match Linux.
- `./tools/heap-test.sh` PASS.
- `./tools/check-source.sh` pass.

The differential battery now ends with `DIFF_DONE` and every marker is
present, for the first time since the battery was introduced.
