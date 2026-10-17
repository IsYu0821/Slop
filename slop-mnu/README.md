# Slop/MNU

**SLOP** means **Synthetic Low-quality Operating Platform**. **MNU** means **Maybe Not Unix**.

A clean-room, non-Linux, non-GNU x86-64 kernel aimed at Linux **userspace ABI** compatibility and source-level driver portability.

## Scope of this milestone

This archive is the large first engineering increment, not a claim of full Linux compatibility. It establishes:

- freestanding Rust kernel layout (`no_std`, no external crates)
- x86-64 entry code, serial console, panic path and halt loop
- Linux x86-64 syscall dispatcher with initial `write`, `getpid`, `uname`, `exit`, `exit_group`
- kernel-owned errno and UAPI types
- VFS, task, virtual-memory and device-driver contracts
- ELF64 type definitions and loader validation skeleton
- PCI device model and Linux-driver adaptation boundary
- architecture and clean-room licensing documents
- LLVM-only build path; no GCC, binutils, glibc, GNU make or Linux source

## Build prerequisites

- Rust nightly with `rust-src`
- LLVM `ld.lld`
- QEMU for execution (optional)

```sh
rustup component add rust-src
cargo +nightly build -Zbuild-std=core --release
./tools/link.sh
```

`tools/link.sh` emits `build/slop-mnu.elf`. This first milestone is entered by a Multiboot/UEFI-capable loader; replacing that temporary boot seam with the in-tree UEFI loader is milestone 2.

## License policy

Project-authored code is Apache-2.0. No Linux or GNU source is copied. Do not paste GPL code, generated headers derived from GPL headers, or Linux internal structure layouts. Public ABI facts must be recorded in `docs/compat-ledger.md` with provenance.

## Single-image UEFI acceptance build (v0.8)

```sh
rustup component add rust-src
./tools/build-uefi.sh
```

The output is `build/efi/EFI/BOOT/BOOTX64.EFI`. It contains the loader, no-std Rust kernel, privilege-transition assembly and the embedded static Linux acceptance ELF. This path currently assumes identity-accessible physical RAM in the firmware page tables and is intended for the first controlled QEMU/OVMF run.

## QEMU/OVMF acceptance run

```sh
OVMF_CODE=/path/to/OVMF_CODE.fd \
OVMF_VARS_TEMPLATE=/path/to/OVMF_VARS.fd \
./tools/run-qemu.sh
```

The runner uses one emulated CPU and serial output because the current syscall transition state is global rather than per-CPU. A successful run should print the Linux ELF message, report exit status 0, reclaim the private address space, and halt.

## Build preflight

```sh
./tools/check-toolchain.sh
```

This verifies the nightly Rust toolchain, Clang/LLD utilities, LLVM binary tools, Python, and critical source-level ABI connections before the UEFI build starts.

## Automated Linux ELF acceptance gate

```sh
OVMF_CODE=/path/to/OVMF_CODE.fd \
OVMF_VARS_TEMPLATE=/path/to/OVMF_VARS.fd \
./tools/acceptance-test.sh
```

The harness writes the complete guest console to `build/acceptance-serial.log` and prints `acceptance: PASS` only when the Linux ELF validates `-EFAULT`, exits with status 0, and its private address space is reclaimed.

## Read-only initramfs and VFS (v0.21)

The UEFI image embeds `build/initramfs.nsar`. At boot the kernel mounts it, resolves `/init` through VFS, and executes that regular file. The acceptance program also opens and reads `/etc/motd` through Linux `openat`, `read`, and `close` syscalls. NSAR is a temporary allocation-free bootstrap format, not a stable on-disk filesystem ABI.

## Hardware-backed anonymous memory (v0.22)

Anonymous private `mmap` and upward `brk` growth now allocate zeroed physical frames and install user PTEs. `mprotect` updates writable/NX permissions, while exact-range `munmap` removes PTEs and recycles frames. Partial VMA operations, demand paging, file-backed mappings, COW, and brk shrink remain future work.

## Static execve (v0.23)

`/init` now uses Linux `execve` to replace itself with `/bin/hello` from initramfs. Path, argv, and envp are copied from user memory before a new CR3 is built. The old image remains active on load failure; on success SYSRET is redirected to the new ELF entry and stack.

## Static C runtime (v0.26)

`/bin/hello-c` is the first C program built without any host libc. The in-tree `libc/` provides freestanding startup, a Linux syscall shim, string functions, a `printf` subset, a first-fit `malloc` family over anonymous mappings, `getenv` and `openat`/`read`/`close` wrappers. `/bin/hello` now hands the process to `/bin/hello-c` with `execve`, so the acceptance chain covers two address-space replacements and a C `main` returning 0. Build it standalone with `./tools/build-libc-test.sh`.
