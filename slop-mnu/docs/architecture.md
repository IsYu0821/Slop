# Architecture

Slop/MNU uses a modular monolithic kernel for the first compatibility target. Rust owns memory-safe policy and object lifetimes; small assembly files own privilege transitions; C is reserved for clean-room source adapters for drivers whose original upstream license permits reuse.

## Compatibility planes

1. **Linux userspace ABI:** x86-64 ELF, syscall numbers, errno, signals, `/proc`, `/sys`, sockets, epoll, futex, ioctls and device nodes.
2. **Driver source portability:** a generated compatibility facade maps a documented subset of Linux driver-facing concepts onto Slop/MNU's native driver API.
3. **Hardware behavior:** PCIe, ACPI, interrupt controllers, DMA/IOMMU, USB, NVMe, networking and graphics are implemented from hardware specifications or permissively licensed references.

Binary `.ko` compatibility is intentionally not the primary design. Linux has no stable in-kernel binary ABI. A version-pinned module capsule may be researched later, but it is a VM-like compatibility product, not the native driver model.

## Kernel subsystems

- `arch`: trap/syscall/context-switch and MMU details
- `vm`: physical allocator, address spaces, copyin/copyout, mmap
- `task`: threads, processes, credentials, namespaces and signals
- `vfs`: inode/dentry/file model and filesystem registration
- `linux_abi`: translation layer, never the kernel's internal API
- `driver`: bus-neutral device graph, resources, DMA and interrupts
