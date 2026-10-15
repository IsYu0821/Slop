# Milestone 23: transactional static execve

Slop/MNU now dispatches Linux x86-64 syscall 59 and resolves the executable path through the read-only initramfs VFS. The syscall copies the path, argv pointer vector, argument strings, envp pointer vector, and environment strings through page-table-backed usercopy before changing the active address space. The bootstrap limits are eight arguments, eight environment entries, and 127 bytes per string.

The ELF commit path accepts caller-provided argv, envp, and execfn values. It clones the active kernel-visible mappings into a new PML4, clears the executable and user-stack slots, validates and installs the static ELF, creates the Linux initial stack, and returns a complete candidate image. Any error before this point leaves the old process active and returns an errno.

After the candidate is complete, the kernel activates its CR3, updates the task and process-runtime ownership records, resets brk, TLS, clear-child-tid and the VMA ledger, then reclaims the old executable slot, stack slot and PML4. The syscall entry notices an exec-pending flag, replaces the saved SYSRET RIP with the new ELF entry, uses the new user RSP, and clears the flag. A successful exec therefore never resumes at the old syscall instruction.

The initramfs now contains `/init`, `/bin/hello`, and `/etc/motd`. `/init` invokes `execve("/bin/hello", ["hello"], ["SLOP_MNU_EXEC=1"])`. The second ELF validates argc, argv and envp directly on its initial stack, then runs the existing VFS and hardware-backed VM tests before exiting.

This implementation is intentionally static-ELF-only. It does not yet implement close-on-exec descriptors, signal disposition reset, credentials, shebang handling, dynamic interpreters, multithreaded exec, arbitrarily large argument blocks, or complete cleanup of non-image private mappings created before exec.
