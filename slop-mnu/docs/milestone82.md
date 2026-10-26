# Milestone 82: CPU enumeration and parked APs

## Goal

Know how many CPUs exist and get the application processors out of
reset without disturbing the running system: walk the ACPI MADT for
all enabled Local APIC IDs, give every AP its own stack and CPU
state, start it through INIT/SIPI into a parked halt loop, and prove
with atomic attestations (not sleeps) that each AP really ran on its
own stack before the boot continues.

## Firmware and protocol contract fixed first

- MADT parsing handles both type 0 (legacy APIC) and type 9 (x2APIC)
  entries, checks lengths before reading, and reports only enabled
  CPUs; `-smp 1/2/4/8` enumerate 1/2/4/8 entries.
- The AP starts with paging OFF, so the SIPI vector page must be
  real physical memory: the trampoline blob hardcodes base 0x7000
  and the vector names that page (0x07). An allocated high frame is
  invisible to the AP.
- Sequence per AP is INIT assert, 10 ms, INIT deassert, 10 ms,
  SIPI, 200 us, SIPI (Linux encoding: IPI 0xC500/0x8500,
  STARTUP 0x0600|vector, physical destination, no shorthand).

## Implementation

- `kernel/smp.rs` (new): MADT-driven `bringup` with per-AP frames,
  per-CPU stacks, GDT/TSS/IDT setup on the AP, LAPIC enable, and an
  atomic state machine (`ABSENT/PRESENT/STARTED/READY`) plus magic,
  stack-probe and single-tick counter per slot. The BSP waits on
  `CPU_STATE` with a TSC deadline; missing APs stay absent and the
  boot continues single-CPU.
- `arch/x86_64/trampoline.S` (new): 16-bit entry, protected-mode
  transition with its own GDT, mailbox load (PML4, entry, LAPIC,
  ID/AP tables), long-mode entry, APIC-ID scan of the BSP-written
  table, per-CPU stack switch, and jump to `ap_main`. Stage markers
  (`0x11111111` real, `0x22222222` prot32, `0x33333333` long,
  `0x44444444` entered) track progress through RAM.
- `install_trampoline` relocates any page-table page living at
  physical 0x7000 to a fresh frame (copy + entry rewrite + CR3
  reload) before zeroing/copying: zeroing it in place unmaps live
  kernel text (first call into the wiped range takes a fetch fault
  through a stale TLB gap). Mailbox and stage slots sit at page
  offset 0x300 (0x7300); writing at offset 0x00 clobbers the AP's
  real-mode entry.
- `kernel/arch/x86_64/apic.rs`: `apic_id`, `send_init`,
  `send_init_deassert`, `send_sipi`, `wait_icr_idle` bounded
  delivery-status wait, `ap_enable` (spurious vector + masked LVTs
  for parked CPUs).
- `kernel/arch/x86_64/idt.rs`: spurious LAPIC vector 0xFF is
  ignored and resumed instead of halting the boot (spurious
  interrupts are expected around INIT/SIPI); the fault code-byte
  dump probes the RIP page first so an unmapped RIP no longer hides
  behind a nested fault.
- `kernel/acpi.rs`: `cpu_list` returning APIC IDs + enabled flags
  for up to `MAX_MADT_CPUS` entries.
- `tools/build-uefi.sh`: assembles and links the COFF trampoline
  object into the EFI binary.

## Tests added

- Boot-log attestation: `smp: cpuN apic=.. stack_top=.. rsp=..
  ticks=1` per READY CPU, `smp: N CPUs online`, and
  `smp: self-test passed (N attested)`. The self-test requires the
  magic, exactly one counter tick, an in-range stack probe on the
  AP's own stack (live RSP on the BSP), a table APIC ID, and
  agreement with the online count; failures halt loudly.
- Manual matrix (TCG): `-smp 1/2/4/8` all boot, enumerate, attest
  and continue into the normal suite with zero faults.

## Verification

- 1/2/4/8 vCPU boots: 1/2/4/8 CPUs online, self-test passed
  (1/2/4/8 attested), per-CPU stacks and APIC IDs correct in the
  report, boot continues into resources/scheduler/vfs self-tests
  and the full acceptance run.
- Quality gates green with the SMP code in the tree:
  `check-source`, `heap`, `acceptance`, `differential` (e2fsck
  clean, free counts exact, platter bytes verified),
  `busybox` 115/115.
- No scheduler changes yet: APs park in `cli; hlt` after
  attestation; parallel execution, IPIs and TLB shootdown belong to
  MS83/MS84.
