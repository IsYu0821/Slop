# Milestone 11: process exit and address-space reclamation

The first Linux process now has an explicit lifetime. Before ring-3 entry, the kernel records the bootstrap CR3, process CR3, ELF PML4 slot, and stack PML4 slot. `exit_group` reaches `task::exit`, which switches back to the bootstrap CR3 before touching page-table frames owned by the process.

Each private slot is then walked through PDPT, PD, PT, and leaf PTE levels. User frames are returned to the physical allocator, empty PT/PD/PDPT frames are returned, the PML4 entry is cleared, and finally the process PML4 frame itself is recycled. Shared firmware/kernel entries are never traversed or released because only slots cleared before process construction are registered as private.

The teardown is intentionally limited to 4 KiB mappings. A large-page entry in a private slot is rejected. When huge pages are introduced, teardown must handle their ownership explicitly.

`tools/run-qemu.sh` provides the first reproducible acceptance invocation for QEMU with OVMF, one CPU, serial output, and the generated FAT directory. Paths to OVMF firmware can be overridden with `OVMF_CODE` and `OVMF_VARS_TEMPLATE`.
