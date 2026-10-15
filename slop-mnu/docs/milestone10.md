# Milestone 10: first process-owned CR3

The acceptance process no longer inserts user PTEs directly into the firmware root. A new PML4 frame is allocated, the live root is copied to preserve the executing PE image, kernel stacks, serial I/O path, BootInfo and identity-accessible RAM, and dedicated user slots are cleared before mapping.

The test ELF is linked at 4 TiB (`0x0000040000000000`), giving it PML4 slot 8. The stack remains near the top of the lower canonical half in slot 255. Both slots are private in the new root. Layouts that use slot 0 or cross the image slot are rejected.

Immediately before IRETQ, CR3 is loaded with the process root. The syscall kernel path remains mapped because the rest of the firmware root was copied. This is an isolation step, not a final kernel address-space design: low firmware mappings and most other firmware PML4 entries are still inherited. The next stage is a deliberate kernel-only mapping set and a physical direct map rather than wholesale firmware inheritance.
