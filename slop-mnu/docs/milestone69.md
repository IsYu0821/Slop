# Milestone 69: a clock that counts in real units

## Goal

The kernel counted one timer tick as a millisecond. The local APIC timer of
a machine does not run at a rate the processor reports, so every time the
kernel handed to a program was scaled by a wrong unit: the monotonic clock,
the wall clock, the processor time, the length of a sleep and the interval
timers.

The milestone measures the clock against a source whose rate the hardware
fixes, and every conversion in the kernel follows the measurement.

## The rate of the time stamp counter

The time stamp counter runs at a constant rate that the processor does not
state, so it has to be measured. The programmable interval timer of the
motherboard counts at a rate the hardware fixes: 1 193 182 counts per
second. Channel 2 of that timer is wired to the status port of the sound
card, so the line it drives can be watched while it counts down. That gives
a window of exactly known length, and the number of counter cycles inside
the window is the rate of the counter.

The measurement on the test machine gives 2.92 GHz. The same measurement
through the local APIC timer agrees with it to a fraction of a percent, so
the two measurements check each other.

## The length of a tick

The local APIC timer counts bus cycles, and the rate of the bus is not known
either, so the length of a tick is measured against the counter. The
measurement happens inside the interrupt, where the counter is read a few
cycles after the interrupt arrives: the span of a run of ticks is the sum of
the gaps between them, so the delay of the interrupt cancels out.

The timer is then programmed so that a tick lasts a quarter of a millisecond.
The count of the timer is scaled with the length that was measured, and the
tick has the same length on every machine. The rate a Linux kernel with a
250 hertz timer uses is the same.

The kernel measured 1.6 milliseconds for a tick before this milestone, and
it assumed one millisecond.

## The interval timers

The interval timers turned the tick into a millisecond, so an alarm of five
seconds asked for five thousand ticks and the time that was left was
reported wrong. They use the measured length of a tick now.

The wall clock timer rounds its deadline down, so the time that is left is
never longer than the time that was asked for. That is what Linux reports
when a timer is cancelled: cancelling an alarm of five seconds reports four
seconds and 999960 microseconds, and the C library rounds that up to five
seconds. Rounding the deadline up instead made the report five seconds and
a few milliseconds, which the C library rounds up to six.

The processor time accounting reports real nanoseconds for the same reason.

## Acceptance criteria

1. `./tools/acceptance-test.sh` prints `acceptance: PASS`.
2. `./tools/busybox-test.sh` prints `busybox compat: PASS (115 checks match
   Linux)`.
3. `./tools/heap-test.sh` prints `heap: PASS`.
4. `./tools/check-source.sh` passes.
5. `cargo +nightly build -Zbuild-std=core --release` is clean.

## Verification

- `cargo +nightly build -Zbuild-std=core --release` clean.
- `./tools/acceptance-test.sh` PASS.
- `./tools/busybox-test.sh` PASS, 115 checks match Linux.
- `./tools/heap-test.sh` PASS.
- `./tools/check-source.sh` pass.
- The differential battery reaches the processor time accounting of a child,
  which needs a clock that a program can read without entering the kernel.

## Open items

- The differential battery stops at the processor time of a child. The
  reason is the missing vector system call entry point: on Linux a program
  that reads the clock does not enter the kernel, so the loop that measures
  the clock spends its time in user mode and the user time of the child
  grows. This kernel has no such entry point, so every clock reading is a
  system call and the time is charged to the kernel.
- The rate of the counter is measured once at boot. A machine whose counter
  is not invariant would need the measurement repeated.
