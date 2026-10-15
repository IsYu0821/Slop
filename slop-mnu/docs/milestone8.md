# Milestone 8: first end-to-end Linux ELF path

This increment connects the previously isolated parts into one acceptance path:

1. Build a static syscall-only Linux ELF.
2. Convert that binary into a COFF object.
3. Compile the no-std Rust kernel as a COFF static library.
4. Link loader, kernel, assembly transitions and embedded ELF into `BOOTX64.EFI`.
5. Pass the embedded ELF as the BootInfo initrd.
6. Reuse the firmware's active CR3 for the initial single-process experiment.
7. Allocate zeroed frames, install user PTEs, copy PT_LOAD bytes, and zero BSS.
8. Map a writable, non-executable user stack and build the Linux initial stack within it.
9. Enter ring 3 through IRETQ.
10. Receive `write` and `exit_group` through the SYSCALL path.

## Deliberate bootstrap assumptions

The `Identity` physical accessor assumes that page-table frames and newly allocated conventional RAM remain identity-addressable in the firmware page tables. This is suitable only for the first controlled UEFI/QEMU acceptance run. It is not a portable UEFI guarantee and must be replaced by a kernel-owned direct map.

The active firmware CR3 is modified instead of installing a process-owned root. This avoids losing live PE image mappings before a direct map and kernel mapping copier exist, but it provides no process isolation.

Only a single static ET_EXEC program is accepted. Dynamic ELF, PIE relocation, file-backed mmap, multiple tasks, signals, scheduler preemption and resource reclamation remain outside this acceptance gate.

The first user entry clears IF because no IRQ routing or timer acknowledgement exists yet. Enabling user interrupts before PIC/APIC handling could route an interrupt into the fatal default exception gate. IF will be enabled after the minimal Linux ELF acceptance test and interrupt-controller initialization are both stable.

The acceptance ELF is linked at `0x0000004000000000`, outside the low physical identity range normally populated by firmware. This avoids colliding with firmware large-page entries while reusing the current CR3. The page-table walker rejects a large-page entry instead of treating it as a next-level table. Newly created ancestor entries propagate the USER bit, and EFER.NXE is enabled before any NX PTE is installed.
