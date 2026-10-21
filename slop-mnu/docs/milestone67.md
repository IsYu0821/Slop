# Milestone 67: the virtio drivers speak the modern interface

## Goal

Make the hardware drivers work with the device models a Linux user expects.
Until now the virtio drivers only spoke the legacy port I/O interface
(version 0.9.5), so every device had to be started with a compatibility
switch (`disable-modern=on`). A plain `virtio-blk-pci` or `virtio-net-pci`
device model, the default in every current emulator and hypervisor, was not
usable at all.

## The transport

`kernel/driver/virtio.rs` holds the transport that both drivers use:

- The legacy interface keeps its port I/O register set: the host and guest
  feature words, the queue address, the queue size, the queue select, the
  notification register, the status and the device configuration.
- The modern interface (virtio 1.0) describes itself through vendor specific
  PCI capabilities. The transport walks the capability list, finds the common
  configuration, the notification area and the device configuration, and
  accesses them through the memory mapped bar.
- The modern interface is negotiated with the `VIRTIO_F_VERSION_1` feature
  bit, and the device confirms the result through the `FEATURES_OK` status
  bit before the queues are installed.
- A modern device lets the driver choose the queue size, the legacy device
  offers one. Both paths end in the same queue description.

The split virtqueue layout is shared: the descriptor table, the available ring
and the used ring. The legacy device keeps the three parts in one buffer at
fixed offsets, the modern device names three separate addresses, and the
transport installs them in the way the interface expects.

## The drivers

- `kernel/driver/virtio_blk.rs` reads the capacity from the device
  configuration of a modern device and from the legacy configuration space
  otherwise, and serves the block requests over either interface.
- `kernel/driver/virtio_net.rs` reads the hardware address from the device
  configuration, posts the receive buffers and transmits over either
  interface.

The test harness no longer asks the emulator for a legacy device, so the
acceptance, differential and BusyBox batteries all run against the modern
device models.

## Acceptance criteria

1. The kernel prints `blk: virtio-blk up capacity=16384 sectors` and
   `net: virtio-net up mac=...` with a default `virtio-blk-pci` and
   `virtio-net-pci` device model.
2. `./tools/acceptance-test.sh` prints `acceptance: PASS`, including the
   UDP and TCP echo test that moves real packets through the network driver.
3. `./tools/busybox-test.sh` prints `busybox compat: PASS (115 checks match
   Linux)`.
4. `cargo +nightly build -Zbuild-std=core --release` is clean.
