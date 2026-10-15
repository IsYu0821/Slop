# Milestone 13: ABI hardening and build preflight

The UEFI loader is compiled with the Microsoft x64 ABI while the kernel entry is explicitly SysV64. The transition stub now subtracts eight bytes before `call`, so the SysV callee observes the required stack alignment. BootInfo remains passed in RDI.

Exception stubs now distinguish additional CPU-error-code vectors: invalid TSS (10), segment not present (11), stack-segment fault (12), general protection (13), page fault (14), alignment check (17), and double fault (8). Treating these as no-error-code exceptions would shift RIP and CS decoding and could misclassify a kernel fault as a user fault. DF is cleared before calling Rust.

`tools/check-source.sh` checks critical ABI and ownership connections without compiling. `tools/check-toolchain.sh` verifies all required host commands and runs the source checks. The UEFI build invokes this preflight before producing artifacts.

The Rust cross-target build now includes `compiler_builtins` with memory intrinsics through `-Zbuild-std`. This reduces dependence on host CRT implementations when linking the freestanding PE/COFF image, while the loader still supplies the basic C memory symbols needed by Clang-generated code.
