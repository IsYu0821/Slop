# Driver compatibility strategy

"Linux drivers" are not a single stable ABI target. Slop/MNU splits the problem:

- userspace-visible device ABI compatibility: device nodes, ioctl payloads, sysfs/procfs/netlink behavior;
- source portability: recompile a legally compatible driver against a versioned Slop/MNU facade;
- hardware enablement: native Rust drivers written from vendor specifications;
- optional legacy capsule: execute a narrowly pinned driver in an isolated compatibility domain.

The facade must never expose Slop/MNU internals. It uses opaque handles, generated layout assertions and explicit lifecycle rules. Each supported Linux API profile is versioned and tested independently.
