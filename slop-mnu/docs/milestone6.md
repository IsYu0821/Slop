# Milestone 6: firmware and CPU substrate

This increment follows the shortest path toward one static Linux ELF. It adds the firmware handoff, frame allocator activation, paging primitives, privilege descriptors and fatal exception path.

## Implemented source paths

- `loader/main.c`: obtains and translates the final UEFI memory map, retries `ExitBootServices`, and transfers a versioned `BootInfo`.
- `tools/build-uefi.sh`: emits `build/efi/EFI/BOOT/BOOTX64.EFI` when passed the required COFF kernel objects/archive.
- `paging.rs`: creates four-level tables and changes individual 4 KiB PTEs with USER, WRITABLE, GLOBAL and NX bits.
- `descriptors.rs`: GDT, ring-0/user selectors, 64-bit TSS, RSP0 and IST1.
- `idt.rs` and `exceptions.S`: IDT installation and diagnostic handlers for default exceptions, double fault, and page fault.

## Integration blockers still open

The regular Rust target currently emits an ELF static archive. The UEFI PE/COFF link requires the kernel to be compiled as COFF objects or embedded as a separately loaded ELF image. `build-uefi.sh` intentionally accepts additional link inputs but does not claim that the existing ELF archive can be linked directly.

The page-table API also requires a platform `PhysAccess` implementation. It must expose page-table frames through a mapped physical-memory window. Activating a CR3 before the kernel image, stacks, BootInfo, framebuffer and physical window are all mapped would immediately fault, so activation is not performed during boot yet.

Exception handlers are currently fatal diagnostics. Recoverable usercopy faults require an exception-table mechanism and a distinction between user faults, kernel-copy faults and fatal kernel faults.
