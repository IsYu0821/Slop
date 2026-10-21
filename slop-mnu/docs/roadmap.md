# Roadmap: large increments

## Increment 1, delivered here
Clean-room policy, freestanding kernel, architecture seams, console, initial syscall ABI, ELF/VFS/VM/task/driver contracts, LLVM build.

## Increment 2, boot and isolation
In-tree UEFI loader, memory map handoff, page-table rebuild, GDT/IDT, APIC/HPET, physical allocator, heap, SMP bring-up, copyin/copyout, KASLR seed, automated QEMU boot tests.

## Increment 3, real Linux process
Demand-paged ELF loader, userspace ring 3, fork/clone/execve/wait, signals, fd table, tmpfs/initramfs, pipes, Unix sockets, futex, epoll. Acceptance: a musl-linked static PID 1 and shell run unmodified.

## Increment 4, storage and hardware
ACPI, PCIe ECAM, MSI-X, IOMMU, virtio-blk/net, NVMe, AHCI, xHCI/HID, block layer, partitioning, ext2 read/write. Acceptance: boot from NVMe image and network via virtio.

## Increment 5, broad Linux ABI
mmap semantics, procfs/sysfs/devtmpfs, netlink, namespaces, cgroups subset, seccomp subset, inotify, timerfd/eventfd/signalfd, io_uring subset. Differential syscall tests run against Linux and Slop/MNU.

## Increment 6, driver portability factory
Machine-readable API inventory; per-kernel-version generated C facade; semantic shims for workqueues, kthreads, sk_buff, NAPI, DRM and ALSA families; license scanner and provenance gate. Start with permissively licensed standalone drivers and clean-room ports.

## Increment 2A, delivered in milestone 2
Versioned firmware-to-kernel BootInfo ABI, bounded physical-frame range allocator, spin lock, canonical userspace-range checks, chunked copyin/copyout use in `write` and `uname`, and x86-64 MSR access primitives. The remaining 2B work is the PE32+ UEFI image, final memory-map retry loop, page-table takeover, descriptors, interrupts and ring-3 transition.

## Increment 3A, delivered in milestone 29
A correct syscall return path (`iretq` instead of `sysretq`), Linux signal delivery with the published `rt_sigframe` layout, `SA_SIGINFO` and the ucontext, per-process pending and blocked masks, `kill` to a process or a process group, `EINTR` for an interrupted blocking wait, and stop/continue for `SIGSTOP` and `SIGCONT`. The remaining 3B work is a real-time signal range, `sigaltstack`, `sigqueue`, `signalfd`, the stopped-child wait status and the floating-point state in the ucontext.
