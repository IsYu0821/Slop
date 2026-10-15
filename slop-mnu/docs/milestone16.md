# Milestone 16: CPU capability gate and write protection

The bootstrap now validates architectural prerequisites instead of assuming every x86-64 environment supports the selected entry path. Extended CPUID checks require SYSCALL/SYSRET and NX before their MSRs or PTE bits are used. Optional 1 GiB page, SMEP, and SMAP capabilities are reported for later stages.

CR0.WP is enabled during CPU initialization. With WP set, ring-0 writes honor read-only page-table entries. This is required before W^X and immutable kernel mappings can be treated as real protection rather than user-only protection.

SMEP and SMAP are detected but deliberately not enabled yet. SMEP would require every kernel instruction path to be mapped supervisor-only, while the current process CR3 inherits firmware mappings. SMAP requires explicit STAC/CLAC handling around user memory access. Both will be enabled after the kernel-owned mapping set and direct-map replace firmware inheritance.
