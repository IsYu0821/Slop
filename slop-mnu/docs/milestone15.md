# Milestone 15: reliable serial acceptance output

The automated gate depends on the final process-reclamation marker being visible before QEMU exits. The earlier serial writer sent bytes directly to COM1 and could overrun the UART FIFO or terminate the VM before the last bytes left the transmitter.

The console now polls the line-status register before every byte, normalizes newline to CRLF, and exposes `console::flush`. Polling loops are bounded so a missing UART cannot hang the kernel forever. The QEMU debug-exit path drains the transmitter before writing to port `0xf4`.

This change does not implement interrupt-driven serial I/O. Polling is appropriate for the single-core bootstrap and deterministic acceptance log; a queued IRQ-driven console belongs after APIC/IOAPIC initialization.
