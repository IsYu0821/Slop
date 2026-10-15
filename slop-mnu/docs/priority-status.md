# One-Linux-program priority status

## Implemented in source, pending build/runtime validation

- UEFI application source and `BOOTX64.EFI` build command
- final memory-map acquisition and bounded `ExitBootServices` retry
- `BootInfo` ABI validation and physical allocator startup
- four-level 4 KiB page-table operations: map, unmap, protect, CR3, INVLPG
- GDT selectors for kernel/user code and data
- 64-bit TSS with RSP0 and IST1
- ring-0 and double-fault stacks
- IDT construction
- fatal default, double-fault and page-fault diagnostics

## Blocking the first Linux ELF

1. Produce a single valid PE/COFF image by compiling the Rust kernel for a COFF-compatible freestanding target, or teach the UEFI loader to load the existing ELF kernel.
2. Reserve the loader, kernel and BootInfo ranges before physical allocation.
3. Establish a physical-memory window and implement `PhysAccess`.
4. Build and activate a complete replacement CR3 while preserving every live kernel mapping.
5. Connect ELF mappings and the stack image to real frames and PTEs.
6. Add ring-3 `iretq` entry.
7. Add an assembly `SYSCALL` entry with kernel-stack switching and safe return.
8. Run a static syscall-only ELF that calls `write(1, ...)` and `exit_group(0)`.

No APIC, scheduler, VFS, dynamic linker, C library, network or device driver is required for this first acceptance test.
