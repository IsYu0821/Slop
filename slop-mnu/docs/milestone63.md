# Milestone 63: High-half kernel and low-address user binaries

## Goal
Stop squatting on UEFI's identity map. The kernel now relocates itself to a
high-half image base at boot, owns a page table that keeps only a direct
physical map plus the kernel image, and leaves the entire low half to user
processes. That makes standard non-PIE static binaries linked at `0x400000`
work unmodified, and removes the low-address carve-outs the kernel used to
reserve (`EXE_BASE` slot rejection in `exec`).

## Scope
- Boot sequence split into `kmain_bootinfo` (before relocation) and
  `kmain_high_continue` (after). The first part runs on firmware page
  tables: it brings up the console, memory allocator, CPU features and ACPI,
  and stashes the memory map, interrupt info and initrd location in
  statics.
- The firmware's own PML4 is mapped read-only by OVMF, so writing high-half
  entries into it faults (`#PF` W=1 P=1). The kernel clones the PML4 into a
  fresh frame instead (`clone_bootstrap`), installs the direct map there,
  and then activates it.
- Direct map: `PHYS_BASE = 0xffff_8000_0000_0000`, 1 GiB pages, PML4 slots
  256–257 (first 1 TiB of RAM). Kernel image: `KIMAGE_BASE =
  0xffff_9000_0000_0000`, PML4 slot 288, mapped with 4 KiB pages.
- OVMF relocates the PE image at load time (observed base `0xDABE000`,
  while lld links at `0x140000000`), so absolute pointers inside the image
  point at the firmware-chosen base. `apply_relocations` parses the PE32+
  `.reloc` section at the copied image and applies the `DIR64` (type 10)
  entries (409 of them) for the new base; without this the first `println!`
  jumped into unmapped low memory.
- `enter_high` is a naked function that swaps `rsp` to the relocated
  ring-0 stack and jumps to the relocated entry via `naked_asm!`. On this
  target inline assembly is Intel syntax.
- After the jump, `keep_slots(&[256, 257, 288])` zeroes every other PML4
  entry (firmware identity map, framebuffer leftovers) and reloads `CR3`.
  The CPU, descriptors, IDT, APIC and syscall MSRs are re-initialized from
  the high addresses; the previously stashed boot data is consumed through
  the statics.
- `loader/main.c` now returns `image_base`/`image_size` in `BootInfo`
  (queried through `EFI_LOADED_IMAGE_PROTOCOL`); `BOOT_ABI_VERSION` is 2.
- Physical-access abstraction: `EarlyPhys` (identity, used while building
  the new tables), `Identity` (direct-map based, for DMA and page-table
  walking), `phys_to_virt`, and `virt_to_phys` implemented as a page-table
  walk so it covers both the direct map and the relocated image.
- DMA users (virtio-net, virtio-blk) translate every ring/buffer address
  with `virt_to_phys`. `virt_to_phys` must preserve the in-page offset;
  masking to the page base made TX buffers alias the first buffer of their
  page and broke ARP/network traffic (the first symptom was
  `EHOSTUNREACH` with ARP ringing out).
- PA-as-pointer audit after the move to high addresses: tmpfs page arrays
  now store `phys_to_virt(frame)`, the file-backed fault path in
  `process.rs` copies through the direct map, and the seccomp install/eval
  paths translate the stored frame only at dereference time.
- `exec` no longer rejects slot 0; user CR3 clones keep the direct map and
  kernel image as supervisor mappings while zeroing the image, stack and
  mmap slots.
- Tests: `tests/minimal-linux.ld` now links at `0x400000`, so every
  acceptance stub and the differential `/init` are non-PIE low-address
  `ET_EXEC` binaries. Added `tests/nopie-helper.c` (musl `-static -no-pie`)
  and the `DIFF_NOPIE_OK` differential case.

Out of scope: KASLR, per-process kernel address-space isolation beyond the
current CR3 clone, and non-PIE dynamic executables.

## Data model
- The kernel image is mapped at `KIMAGE_BASE + (va - image_base)`; all
  function pointers (IDT, syscall MSRs, GDT/TSS) are rebuilt high after the
  jump.
- `PageTableRoot::current()` is the live kernel CR3; `clone_bootstrap`
  produces per-task roots carrying the supervisor mappings.
- `phys::virt_to_phys(va)` walks the current tables and ORs back the low 12
  bits of `va`; callers pass buffers in the direct map or the kernel image.

## Acceptance criteria
1. `./tools/acceptance-test.sh` prints `acceptance: PASS` with `/init`
   linked at `0x400000` and no low-half reservations left in `exec`.
2. `./tools/differential-test.sh` prints `differential test: PASS`,
   including `DIFF_NOPIE_OK`: fork+exec of a non-PIE musl helper reports
   `rip=0x40105a` and exit status 17.
3. `./tools/check-source.sh` passes with tokens for the relocation path,
   the physical-access helpers and the non-PIE test.

## Verification
- `cargo +nightly build -Zbuild-std=core --release` clean.
- Native (musl) smoke of `tests/differential.c` passes.
- `./tools/differential-test.sh` PASS, `./tools/acceptance-test.sh` PASS,
  `./tools/check-source.sh` pass.
- Serial log shows `Slop/MNU 0.27.0: high-half kernel, BootInfo accepted`
  followed by the full acceptance suite; `/bin/hello-net` runs at
  `0x4000000657a7` (PIE) while the low stubs run at `0x4000e8`.

## Risks
- The boot path depends on the loader reporting the real image base; a
  future loader change that drops `EFI_LOADED_IMAGE_PROTOCOL` support
  breaks relocation.
- Only `DIR64` relocations are applied (all lld emits for x86-64 PE).
- The direct map covers the first 1 TiB; more RAM would need another PML4
  slot (or 4 KiB pages above the 1 GiB boundaries).
- The kernel image is unmapped from the low half, so any remaining
  identity-map assumption in firmware-facing code (e.g. new drivers poking
  physical addresses) must go through `phys_to_virt`/`Identity`.
