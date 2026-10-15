# Milestone 17: interrupt-controller and timer foundation

The UEFI loader now locates ACPI 1.0/2.0 RSDP entries in the system configuration table, preferring ACPI 2.0, and passes the pointer through BootInfo. The kernel validates RSDP, root table, and MADT checksums before consuming controller addresses. MADT parsing records the Local APIC address, first IOAPIC, ISA interrupt-source overrides, and Local APIC address override.

The bootstrap masks both 8259 PICs, enables xAPIC through IA32_APIC_BASE, enables the Local APIC software bit, masks LINT0/LINT1, and masks every IOAPIC redirection entry. External device IRQs therefore cannot enter until an explicit route is installed. The Local APIC timer runs periodically on vector 0x40. Its assembly entry preserves general-purpose registers, calls a Rust tick handler, writes EOI, restores registers, and returns with IRETQ.

Initial user RFLAGS now includes IF (`0x202`). The acceptance ELF can therefore be interrupted while running in ring 3, with hardware stack switching through TSS.RSP0. Kernel entry through SYSCALL still masks IF using IA32_FMASK, so the current global syscall stack cannot be recursively used by timer preemption.

The timer count is intentionally uncalibrated. It proves interrupt delivery but is not yet a time source. HPET or PIT calibration is required before ticks can represent nanoseconds or drive a fair scheduler quantum.

`IrqSpinLock` saves IF, disables local interrupts while held, and restores the previous IF state on release. This is the required locking primitive for run queues and timer queues shared with interrupt context.

Before entering user mode, the kernel performs a bounded Local APIC timer self-test. It enables IF, executes HLT, receives periodic timer interrupts, disables IF again, and requires the tick counter to advance. The acceptance harness now requires the marker `local APIC timer and EOI passed`, making timer delivery part of the automated gate.

`route_isa` converts an ISA IRQ through MADT interrupt-source overrides, applies active-low and level-trigger flags, selects the bootstrap Local APIC as destination, and programs the matching IOAPIC redirection entry. All routes remain masked unless a driver explicitly requests otherwise.

The timer gate uses TSS IST2 and a dedicated 64 KiB IRQ stack. This avoids corrupting the global SYSCALL kernel stack when a timer interrupt arrives while Rust syscall code is running. The assembly entry also adds an eight-byte alignment slot before calling Rust, because the CPU interrupt frame plus fifteen saved registers otherwise leaves the SysV call site misaligned.
