# Milestone 25: integrated user task, time, sleep, and random

The boot user ELF is a runnable child task selected by the cooperative scheduler. It owns a kernel stack and CR3, enters ring 3 through a user-task trampoline, and returns to PID 1 as a zombie after exit or a user fault.

Linux syscall paths now include bootstrap implementations of `clock_gettime`, `nanosleep`, and `getrandom`. Time uses Local APIC ticks with a temporary 1 ms conversion, and random output comes from a non-cryptographic xorshift64 generator. These support libc bring-up but are not yet production timekeeping or secure entropy.

The next acceptance gate is a genuinely static C/libc executable reaching `main`, not another hand-written assembly program.
