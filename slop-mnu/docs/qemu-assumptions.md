# QEMU-specific assumptions inventory (Track 2)

Enumerated from the tree; each item classified:
1. test-only, production path unaffected
2. replaceable with real-machine detection
3. needs a platform abstraction
4. dangerous on real hardware
5. removable

QEMU regression tests stay; the production path must not depend on them.

## Devices that exist only under QEMU (class 1)

- EDU (`1234:11e8`, `kernel/driver/pci.rs` + `tests/kmod/*`): real-DMA/IRQ
  test device. Test-only by construction; never probed except by the
  `slotest` test module. No production dependency.
- `isa-debug-exit` port `0xf4` (all `tools/*-test.sh`, `run-qemu.sh`):
  guest exit-code channel. Test harness only. Real-machine bring-up must
  use reset/shutdown (ACPI FADT reset register) instead.
- Fixed PCI IDs in selftests (`8086:1237` host bridge, `1234:ABCD`,
  `8086:29C0` in `linux_compat.rs` selftests): prove the match path, no
  driver binds them in production. Class 1.
- `-cpu max`, `-machine q35` flags in test scripts: harness only.

## virtio stack (class 2/3)

- `kernel/driver/virtio{,_blk,_net}.rs`: virtio-blk carries the ext2/ext4
  test images; virtio-net carries the test network. The drivers bind by
  PCI class/vendor (`VIRTIO_VENDOR=0x1af4`), which is transport-correct
  (virtio-pci exists on real hypervisors too), but bare metal has no
  virtio. Plan: keep for QEMU regression; add NVMe (block) and a real
  Ethernet driver (net) behind the common block/net core. No virtio
  fallback logic may gate real-device probing.
- Legacy `/mnt` virtio-blk auto-route (`vfs.rs`): assumes the first
  virtio-blk device holds the test filesystem. Real machines need an
  explicit mount (already supported) with the legacy route kept for
  QEMU tests only. The read-only ext4 contract stays valid on both.

## User networking constants (class 2)

- `OUR_IP=10.0.2.15`, gateway `10.0.2.2`, DNS `10.0.2.3`
  (`kernel/net.rs`, Slirp defaults). These are defaults, not detection:
  DHCP already overrides the leased IP where offered. Real-machine work:
  honor DHCP-provided gateway/DNS everywhere (audit the static fallbacks),
  never assume `10.0.2.x` reachability.

## Serial console (class 2/3)

- COM1 `0x3f8` polling console (`kernel/console.rs`): fine on most PCs
  but not universal (no SuperIO, MMIO UARTs, USB debug). Real-machine
  bring-up needs UEFI ConOut + framebuffer logging alongside serial,
  selected by what firmware/ACPI actually describes. Keep 0x3f8 as a
  probe, not an assumption.

## Firmware/tables (already generic — keep)

- ACPI discovery walks RSDP→XSDT/RSDT→MADT from firmware tables
  (`kernel/acpi.rs`); MADT/ISO/IOAPIC/LAPIC parsing is table-driven.
  No QEMU-specific table assumption found. MCFG/ECAM, HPET, FADT reset,
  SRAT are parsed on demand — extend, don't fork, for real machines.
- UEFI memory map + ExitBootServices + GOP framebuffer via BootInfo
  (`loader/main.c`, `kernel/boot.rs`): generic UEFI, portable as-is.
- Page tables: direct map requires 1 GiB pages; fail-fast otherwise.
  Real firmware + CPUID check stays; no QEMU identity-map assumption
  in the kernel path (firmware low mappings are dropped at
  `kmain_high_continue`).

## SMP startup (class 2, low risk)

- AP trampoline at `0x7000` (`kernel/smp.rs`): the conventional SIPI
  vector page, also valid on real hardware. Keep, but verify the target
  RAM is usable from the UEFI memory map instead of assuming it (a
  reserved region there would be class 4).

## Timing (class 2)

- Per-tick TSC recalibration was removed (Track 0): the rate is now the
  boot-measured value only. QEMU TCG jitter no longer leaks into
  ns<->tick conversions; real invariant-TSC hardware is unaffected.
- QEMU EDU DMA completion latency (~100 ms timer) is a test-timing
  fact, not a kernel assumption (all device waits are bounded polls).

## Interrupts (class 1/2)

- IOAPIC GSI swizzle assumes q35 root-port layout
  (`arch/x86_64/apic.rs` `route_gsi_low` + q35 comment): correct for
  QEMU q35; real machines need swizzle from the ACPI _PRT (not yet
  parsed — Track 2 follow-up). MSI/MSI-X programming is generic PCI.
- QEMU EDU MSI is programmed but never delivered by the emulator
  (documented; INTx used). No production reliance.

## DMA/IOMMU (class 4 if ignored)

- The DMA core programs bus addresses directly (no IOMMU). Safe under
  QEMU (no IOMMU). Real machines with an enabled IOMMU (Intel VT-d
  DMAR / AMD-Vi IVRS tables) MUST either program remapping or verify
  pass-through; enabling bus mastering on unknown devices stays
  forbidden until the IOMMU tables are parsed. Read-only NVMe bring-up
  must confirm the translation regime first.

## What must never reach production (class 4/5)

- QEMU-only PCI IDs must not appear in any production match table.
- The `0xf4` exit port must not appear in any non-test code path
  (it is already test-scripts-only).
- `expect("already tested")`-style shortcuts: every real-machine claim
  below needs its own on-hardware log; QEMU evidence does not transfer.
