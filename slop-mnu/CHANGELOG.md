# Changelog

## 0.2.0 - 2026-10-12
- Added versioned loader/kernel BootInfo ABI and memory-map records.
- Added bounded physical frame allocator with overflow checks.
- Added dependency-free spin lock.
- Added x86-64 MSR read/write primitives and syscall MSR constants.
- Replaced direct syscall pointer dereferences with chunked usercopy boundaries.
- Added early security invariants and milestone 2A roadmap entry.

## 0.3.0 - 2026-10-12
- Centralized Linux x86-64 syscall numbers and UAPI structure layouts.
- Added identity, PID/TID, close, fstat, brk, arch_prctl, set_tid_address, robust-list and prlimit bootstrap behavior.
- Added FS-base process state needed by libc thread-local storage setup.
- Added System V AMD64 auxiliary-vector builder for future ELF exec.
- Kept unsupported memory, signal, futex, filesystem and clock calls explicit as ENOSYS rather than faking success.

## 0.4.0 - 2026-10-12
- Replaced alignment-sensitive ELF casting with a bounds-checked byte parser.
- Added strict ELF64 PT_LOAD planning, overflow checks, interpreter detection and entry validation.
- Added a fixed-capacity userspace VMA ledger with placement, overlap checks, protection and removal.
- Added anonymous mmap, mprotect and munmap ABI paths without pretending hardware PTEs already exist.
- Added ENODEV and EEXIST errno values.

## 0.5.0 - 2026-10-12
- Added a complete allocation-free AMD64/Linux initial stack builder for argc, argv, envp and auxv.
- Added AT_RANDOM, AT_EXECFN and ELF metadata placement with bounds and NUL checks.
- Added a transaction-friendly ELF execution planner with page-aligned PT_LOAD mappings and a guarded user stack reservation.
- Added E2BIG and explicit limits for bootstrap argument and environment vectors.

## 0.6.0 - 2026-10-12
- Added a freestanding C/assembly UEFI application entry and BOOTX64.EFI build path.
- Added UEFI GetMemoryMap allocation, descriptor translation and bounded ExitBootServices retry loop.
- Connected validated BootInfo to the physical allocator and CPU descriptor initialization path.
- Added x86-64 four-level page-table map, unmap, protect, CR3 activation and INVLPG primitives.
- Added GDT, long-mode TSS, ring-0 stack, IST stack and IDT construction.
- Added fatal default, double-fault and page-fault assembly entry paths with CR2 diagnostics.

## 0.7.0 - 2026-10-12
- Added IA32_EFER/STAR/LSTAR/FMASK programming for 64-bit SYSCALL.
- Added assembly syscall entry, user-RSP capture, kernel-stack switch, register preservation and Rust dispatch.
- Added lower-canonical SYSRET RIP/RSP validation and fatal fallback.
- Added an iretq ring-3 entry trampoline with user selector and context validation.
- Added a no-libc static Linux acceptance ELF using only write and exit_group.

## 0.8.0 - 2026-10-12
- Added a COFF Rust-kernel build path and single-image BOOTX64.EFI link pipeline.
- Embedded the static Linux acceptance ELF into the UEFI image and exported it through BootInfo initrd fields.
- Added current-CR3 translation and bootstrap identity physical access.
- Added actual PT_LOAD frame allocation, user PTE installation, segment copying and BSS zeroing.
- Added a mapped one-page user stack, Linux initial stack copy, and automatic iretq entry.
- Made assembly/Rust boundaries explicitly SysV64 so the same kernel can be linked into a Microsoft-ABI UEFI image.

## 0.9.0 - 2026-10-12
- Replaced numeric-only user pointer checks with page-table-backed usercopy validation.
- Added effective USER/WRITABLE/PRESENT permission checks and page-boundary chunking.
- Added bounded physical-frame recycling with duplicate-free detection.
- Added transactional ELF page installation and rollback of PTEs and frames on failure.
- Removed unchecked byte writes from the ELF copy path.

## 0.10.0 - 2026-10-12
- Added a per-process bootstrap CR3 cloned from the live firmware root.
- Cleared dedicated PML4 slots before installing the Linux image and user stack.
- Moved the acceptance ELF to a separate 4 TiB PML4 slot.
- Activated the process CR3 immediately before the ring-3 IRET transition.
- Rejected ELF layouts that share the identity slot or cross the private image slot.

## 0.11.0 - 2026-10-12
- Added process address-space lifetime tracking across the CR3 switch.
- Added recursive teardown for private PML4 slots, including user frames and intermediate page tables.
- Restored the bootstrap CR3 before reclaiming the process root.
- Connected exit_group to address-space reclamation before CPU halt.
- Added a single-core QEMU/OVMF serial acceptance runner.

## 0.12.0 - 2026-10-12
- Added containment for user-mode CPU faults with process-only termination and address-space reclamation.
- Added explicit general-protection, page-fault and double-fault entry layouts.
- Recorded process exit status and terminating exception vector.
- Strengthened the Linux acceptance ELF to verify invalid write pointers return raw `-EFAULT`.
- Added distinct success and failure messages and nonzero failure exit status.

## 0.13.0 - 2026-10-12
- Corrected the Microsoft-x64 to SysV64 loader transition stack alignment.
- Added dedicated error-code exception stubs for invalid TSS, segment-not-present, stack-segment and alignment-check faults.
- Cleared DF before entering Rust exception handling.
- Added source consistency and host toolchain preflight scripts.
- Included compiler-builtins memory intrinsics in the cross-target Rust build.

## 0.14.0 - 2026-10-12
- Added QEMU isa-debug-exit signaling for deterministic guest success, ABI failure and user-fault outcomes.
- Added a timeout-bounded acceptance harness that captures and validates serial output.
- Added automated checks for EFAULT success, zero exit status and full address-space reclamation.
- Updated the QEMU runner with the debug-exit device.

## 0.15.0 - 2026-10-12
- Added UART transmitter-ready polling instead of writing bytes blindly.
- Added CRLF normalization for serial terminals.
- Added bounded serial drain before QEMU debug-exit.
- Protected acceptance markers from being lost when the VM exits immediately after process teardown.

## 0.16.0 - 2026-10-12
- Added CPUID detection for SYSCALL/SYSRET, NX, 1 GiB pages, SMEP and SMAP.
- Rejected CPUs that cannot support the required Linux x86-64 entry and NX model.
- Enabled CR0.WP so supervisor writes respect read-only PTEs.
- Added boot diagnostics for detected CPU protections.

## 0.17.0 - 2026-10-12
- Located ACPI RSDP through UEFI configuration tables and passed it through BootInfo.
- Added checksum-validated XSDT/RSDT and MADT discovery.
- Parsed Local APIC, IOAPIC, interrupt-source overrides and LAPIC address overrides.
- Initialized xAPIC, masked the legacy PIC and all IOAPIC redirection entries.
- Added a periodic Local APIC timer vector, full register-preserving IRQ entry and EOI processing.
- Enabled IF in the initial ring-3 RFLAGS.
- Added an interrupt-disabling spin lock for state shared with IRQ handlers.

## 0.18.0 - 2026-10-12
- Added fixed-capacity task descriptors with dynamic PID allocation and task states.
- Added per-task 32 KiB kernel stacks and a SysV64 callee-state context switch.
- Added a round-robin runnable queue, cooperative yield, sleeping tasks and timer wakeups.
- Added timer-driven `need_resched` bookkeeping without unsafe switching inside interrupt context.
- Added kernel-thread creation and a two-thread context-switch self-test.

## 0.19.0 - 2026-10-12
- Added per-task CR3, FS base, TSS.RSP0 and SYSCALL kernel-stack state.
- Applied architecture state before every cooperative context switch.
- Connected `arch_prctl(ARCH_SET_FS)` to the real IA32_FS_BASE MSR.
- Corrected the bootstrap task to use the actual 64 KiB ring-0 stack rather than an unused scheduler stack.
- Extended the scheduler test to verify FS-base isolation across two kernel threads.

## 0.20.0 - 2026-10-12
- Added parent PID tracking and a real task-backed `getppid` result.
- Added fixed-capacity wait queues with block, wake-one and wake-all operations.
- Added scheduler APIs for blocking and waking tasks by PID.
- Added child zombie lookup, exit-status collection and task-slot reuse.
- Extended the scheduler self-test to reap both child kernel threads.

## 0.21.0 - 2026-10-12
- Replaced direct embedded-ELF boot with a read-only initramfs containing `/init` and `/etc/motd`.
- Added checksum-independent, bounds-checked NSAR initramfs parsing and absolute path lookup.
- Added read-only inode metadata and offset-based file reads.
- Added a 32-entry bootstrap FD table with console descriptors and independent file offsets.
- Implemented `openat`, regular-file `read`, `close`, `lseek`, `fstat`, and `newfstatat` paths.
- Extended the Linux acceptance ELF to open and print `/etc/motd` before exiting.

## 0.22.0 - 2026-10-12
- Connected anonymous private `mmap` to physical-frame allocation and user PTE creation.
- Zero-filled every newly allocated anonymous and brk page before making it accessible.
- Connected `mprotect` to hardware PTE writable/NX permissions and TLB invalidation.
- Connected `munmap` to PTE removal and physical-frame recycling.
- Connected upward `brk` growth to mapped read/write heap pages.
- Added rollback when multi-page anonymous mapping fails partway through allocation.
- Extended the ring-3 acceptance program to read/write mmap and brk memory.

## 0.23.0 - 2026-10-13
- Added Linux x86-64 `execve` dispatch for static initramfs ELF files.
- Added bounded, usercopy-backed path, argv, and envp capture.
- Generalized the transactional ELF loader to construct a caller-supplied initial stack.
- Added atomic address-space replacement after the new ELF and stack are fully built.
- Added syscall-return redirection so successful exec resumes at the new entry/RSP and never returns to the old image.
- Added `/bin/hello`; `/init` now execs it with one argument and one environment variable.
- Added ring-3 validation of argc, argv, and envp after exec.

## 0.24.0 - 2026-10-13
- Integrated boot user ELF as a normal runnable child task with its own kernel stack and CR3.
- Returned user exit and faults to the scheduler as zombie tasks, then reaped them from PID 1.

## 0.25.0 - 2026-10-13
- Added bootstrap clock_gettime, nanosleep, and getrandom syscall paths.
- Extended the ring-3 acceptance program with time, sleep, and random checks.

## 0.25.2 - 2026-10-15
- Renamed the project to Slop/MNU: Synthetic Low-quality Operating Platform / Maybe Not Unix.
- Renamed package, target, symbols, artifacts, boot ABI, uname identity, initramfs messages, tests, and documentation.
