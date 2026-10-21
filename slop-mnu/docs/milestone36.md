# Milestone 36: virtio-net and TCP/IP

`AF_INET` sockets work end to end over QEMU user networking. `/bin/hello-net`,
chained after the Unix-socket test, runs a UDP `sendto`/`recvfrom` echo and a
TCP `connect`/`write`/`read` echo against a host server on `10.0.2.2:5555`.

## PCI and virtio-net

`kernel/arch/x86_64/port.rs` adds port-I/O primitives. `kernel/driver/pci.rs`
now scans the CAM (`0xcf8`/`0xcfc`), handles multi-function devices and
64-bit BARs, and enables memory/IO decoding. `kernel/driver/virtio_net.rs` is
a legacy (0.9.5) virtio-net driver for `1af4:1000`: it negotiates no features,
reads the MAC from config space, and uses 256-entry split rings (descriptors at
offset 0, available ring at 4096, used ring at 8192; 32 RX buffers and 8 TX
buffers behind a single lock).

Two legacy quirks cost the debugging time:

- The available-ring helpers must point at offset 4096, after the 4096-byte
  descriptor table. Pointing at offset 2 corrupts descriptor 0 and makes the
  device see an available index of zero forever.
- With no negotiated features every buffer carries the 10-byte
  `virtio_net_hdr` prefix on both TX and RX, so the driver zeroes it before
  frames and skips it after them. Without the prefix QEMU parses the frame's
  first bytes as a header and slirp drops the garbage.

## Stack

`kernel/net.rs` implements Ethernet, ARP (4-entry cache, replies to requests
for our IP), IPv4 (identification, DF, checksum), ICMP echo, UDP with
per-socket queues, and a minimal TCP client (go-back-N with RTO and a
retransmit budget, window shrinking on receive, FIN/`shutdown` handling).
Transmission is synchronous: the driver spins the TX used ring after each
notification, which QEMU completes synchronously for a PIO kick.

One checksum subtlety: the one's-complement fold of a *valid* packet is `0`,
not `0xffff`, because the helper returns the complement. Verifying with
`!= 0xffff` rejects every correct datagram.

## Delivery model

There are no device interrupts. The APIC timer handler calls `net::poll()`
in addition to the scheduler tick, and blocking waits (`resolve`, `udp_recv`,
TCP handshake) loop `poll()` plus a nap that ends in `sti;hlt;cli` every
eighth round. Syscalls run with interrupts disabled (`FMASK` clears `IF`), so
the inline polling is what keeps these waits live; the tick polls too, so data
that arrives during a nap is dispatched by the time the wait resumes.

## Sockets and ABI

`socket.rs` dispatches ids `16..24` to UDP and `32..40` to TCP via
`inet_kind()`; the Linux ABI layer parses `sockaddr_in` into `(ip, port)`
pairs and writes them back for `getsockname`/`getpeername` (nr 52).
`sendto`/`recvfrom` work with and without addresses, `bind` supports
`INADDR_ANY` and specific addresses, and `shutdown` forwards its `how` for
TCP. `listen`/`accept` on inet sockets report `EOPNOTSUPP`; server-side TCP is
future work.

## Testing

`tools/net-echo.py` binds UDP and TCP on `127.0.0.1:5555` (slirp rewrites
`10.0.2.2`), writes `build/net-echo.ready`, and the acceptance script starts
it, waits for readiness, and kills it on exit. Markers cover the exec into
`/bin/hello-net`, `net: main entered`, and the passing UDP/TCP echo.

## Deliberate limits

No DHCP (static `10.0.2.15`/`10.0.2.2`), no ICMP errors, no fragmentation
(MSS 1400, MTU 1500 with `DF`), no server-side `listen`/`accept`, no
`IP_FREEBIND`-style options, single outstanding segment window stretch, and a
small fixed socket table (8 UDP, 8 TCP).
