# Milestone 70: the vector system call entry point

## Goal

Linux gives every program a page of code that it can call without entering
the kernel. The C library looks the entry points up in the dynamic symbol
table of that page and uses them for the clock, so a program that reads the
clock does not pay for a system call, and the time it spends is charged to
the program instead of the kernel.

Without that page every clock reading of a program is a system call, and a
program that measures the clock reports almost no user time. The
differential battery compared the processor time of a program with the
value Linux reports, and the two were far apart.

## The page

`vdso/vdso.S` holds the entry points, with the names Linux uses:
`__vdso_clock_gettime`, `__vdso_gettimeofday`, `__vdso_time` and
`__vdso_getcpu`. The page is a shared object with a dynamic symbol table,
so the C library finds the entry points the same way it finds them on
Linux.

`tools/build-vdso.sh` builds it and checks two things that the C library
needs: the dynamic hash table has to be beside the symbol table, and the
file offset and the address of every loadable segment have to be the same.
The C library computes the load address of the page from the last loadable
segment, so the two have to agree with the addresses the kernel maps the
segments to.

The page is mapped at the same address in every process, alone in a page
table slot of its own, and the auxiliary vector points at it.

## The time

The page cannot ask the kernel for the time without defeating its purpose,
so the kernel keeps a shared page up to date:

- the rate of the time stamp counter, in nanoseconds per cycle with 32
  fractional bits,
- the value of the counter at the last update,
- the monotonic and the wall clock in nanoseconds at that moment.

The entry point reads the page under a sequence lock, adds the time that
passed since the last update, measured with the counter, and splits the
result into seconds and nanoseconds. The clocks that count the time a
process spends on the processor are not known in the page, so they report
that they are not available and the caller falls back to the system call,
like Linux.

The shared page is the same physical frame in every process, so the kernel
writes it through the direct map without knowing which program runs. The
frame is counted, so the last program that goes away does not take the page
away from the kernel.

## The interrupt path read the wrong place

With the page in place, a program that reads the clock spent its time in
user mode, but the processor time of the program stayed small: nearly every
timer tick was still charged to the kernel.

The interrupt path read the code segment of the interrupted context from
the wrong place. The vector register save area and the general registers
lie between the stack pointer and the frame of the processor, so the code
segment is 384 bytes above the stack pointer and not 128 bytes above it.
The code read 128 bytes, which is part of the vector register save area, and
the segment it found there was almost always the kernel one. Every tick was
therefore charged to the kernel, and the processor time of a program that
reads the clock never grew.

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
- `./tools/differential-test.sh` PASS.
- `./tools/busybox-test.sh` PASS, 115 checks match Linux.
- `./tools/heap-test.sh` PASS.
- `./tools/check-source.sh` pass.

The differential battery now runs to the end for the first time. It used to
stop at the pending signal check, and before that at the processor time
accounting, because the clock could not be read without entering the
kernel.

## Open items

- The threaded part of the differential battery fails now and then. The
  program hangs with three of four threads finished, so a wakeup of a
  waiting thread is lost. The analysis is recorded in
  `docs/priority-status.md`.
