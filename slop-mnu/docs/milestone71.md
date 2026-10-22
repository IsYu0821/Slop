# Milestone 71: a clock that agrees with itself

## Goal

The page of code that reads the clock and the kernel that keeps it up to
date have to describe the same time. The differential battery failed now and
then, and the reason was that they did not: the page of code reported a time
far in the future, and a program that measured the clock with it never saw
its own loop end.

## The counter can move backwards

The time stamp counter runs at a constant rate, but its value is not
guaranteed to grow: when the machine takes the processor away and gives it
back, the counter can end up behind the value that was read before. The
difference between the counter now and the value the kernel wrote into the
shared page is then negative, and the page of code used it as it was.

The difference is clamped to zero now, so the time since the last update of
the shared page never goes below zero.

The kernel measures the length of a tick with the counter as well, so the
length of a tick follows the counter instead of standing still, and the
processor time, which is counted in the time the counter has passed, stays
in step with the clock.

## The processor time is counted in nanoseconds

The accounting counted ticks and converted them with the length of a tick
that was measured last. As soon as the length of a tick changed, every tick
that came before was converted with the wrong length, and the processor time
of a program drifted away from the clock. The accounting counts the
nanoseconds now, and the length of a tick is only used where a count of
ticks is what the interface asks for.

## The futex lost a wakeup

A thread that waits for a word and a thread that changes the word race with
each other. The word was read and the waiter was registered in the table in
two steps, and a waker that came between them found no waiter, so the
wakeup was lost and the waiter blocked although the word had changed.

The word is read and the waiter is registered under the same lock the waker
uses to take the waiter out of the table, so the two cannot come between
each other. The timeout of a futex wait is a length of time and not a point
in time, which is how Linux reads it.

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
