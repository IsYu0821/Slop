# Milestone 12: user-fault containment and ABI self-test

CPU exceptions now pass vector, error code, RIP, and CS to Rust. Page fault, general protection, and double fault have dedicated assembly stubs because the CPU already pushes an error code for those exceptions. Exceptions originating at CPL3 are recognized through `CS & 3` and terminate only the active Linux process. The kernel first records a signal-like status, switches back to the bootstrap CR3, reclaims the private address space, and halts the acceptance CPU. Ring-0 exceptions remain fatal kernel exceptions.

The static Linux ELF now begins with a negative test: `write(1, 0x1234, 8)` must return raw `-EFAULT` (`-14`). It then performs the valid console write and checks the returned byte count. Any mismatch emits a failure message on fd 2 and exits with status 1. Success emits `hello from a Linux ELF on Slop/MNU; EFAULT passed` and calls `exit_group(0)`.

This improves the meaning of a successful boot. It demonstrates not only ring-3 execution and SYSCALL/SYSRET, but also Linux raw errno behavior, page-table-backed user-pointer validation, a successful user-buffer copy, and process teardown.
