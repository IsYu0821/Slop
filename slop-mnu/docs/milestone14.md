# Milestone 14: deterministic acceptance result

A halted VM is ambiguous: it may mean a successful `exit_group`, a kernel deadlock, or a fatal exception. The bootstrap test now writes a result byte to QEMU's `isa-debug-exit` device after process teardown. Guest value `0x10` denotes success, `0x11` denotes a nonzero Linux exit status, and `0x12` denotes a contained user CPU fault.

`tools/acceptance-test.sh` runs QEMU under a timeout, saves serial output to `build/acceptance-serial.log`, and requires all of the following:

- QEMU host status 33, corresponding to guest value `0x10`.
- The Linux ELF success marker containing `EFAULT passed`.
- A zero task exit status.
- Confirmation that the process address space was reclaimed with vector zero.
- Absence of the ABI failure marker and kernel-exception marker.

This turns the first Linux-program goal into a machine-checkable gate rather than a visual inspection of serial output. The actual guest execution remains to be performed in the user's build environment.
