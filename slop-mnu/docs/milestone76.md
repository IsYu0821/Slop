# Milestone 76: real network servers in the guest

## Goal

Until now the guest could only *originate* TCP connections (and UDP to the
outside). No program could serve: `listen`/`accept` on `AF_INET` returned
`EOPNOTSUPP`, and `127.0.0.1` was unreachable (`EHOSTUNREACH` from ARP).
This milestone adds TCP passive open plus loopback delivery, so a forked
server and client can rendezvous over `127.0.0.1` entirely inside the
guest, byte-identical to Linux.

## Native behavior observed

Probes fixed the contract before any kernel code changed:

- `listen` on an unbound socket succeeds (auto-bind), `listen(fd, -1)`
  succeeds, `accept` on a non-listening socket reports `EINVAL`,
  duplicate `bind` reports `EADDRINUSE`, refused `connect` reports
  `ECONNREFUSED`, `getpeername` on a listener reports `ENOTCONN`,
  `bind` on a connected socket reports `EINVAL`, and `listen` on
  `SOCK_DGRAM` reports `EOPNOTSUPP` (unchanged behavior).
- Loopback UDP to `127.0.0.1` reports the sender as `127.0.0.1`.

## Implementation

- `TcpState::{Listen, SynRcvd}` plus per-listener accept queues
  (`ACCEPTQ` of 4 with a waiter queue each), kept outside `TcpConn`
  so the table stays `Copy`.
- `tcp_listen` (auto-bind when unbound, idempotent re-listen, `EINVAL`
  otherwise), `tcp_accept` (blocking `acceptq` wait, `EAGAIN` when
  nonblocking, `EINVAL` for non-listeners), `EADDRINUSE` bind conflicts
  with wildcard overlap, `EINVAL` for rebinding, `ENOTCONN` peername on
  listeners.
- Passive open in `tcp_input`: unmatched SYN goes to a matching listener
  (wildcard or exact address) which allocates a child, sends SYN+ACK and
  parks it in `SynRcvd` (retransmitted on timeout, reset on RST);
  the completing ACK establishes it and queues it for `accept`.
  Unmatched SYN without a listener gets RST so `connect` reports
  `ECONNREFUSED`. Retransmitted SYNs resend SYN+ACK; overfull queues
  drop (client RTO retries).
- Closing a listener resets queued children; `tcp_close` of a `SynRcvd`
  slot resets immediately; socket descriptions now free INET sockets on
  last close/exit (previously leaked until reboot).
- Loopback transport: `is_loopback` (127/8 or our own address) redirects
  TCP segments and UDP datagrams into an in-kernel queue drained by
  `poll()` with no locks held, so looped-back input can never deadlock
  on the sender's slot. Source is presented as the destination, keeping
  both directions symmetric; `udp_deliver` matches bound addresses
  against the destination.
- `sys_listen`/`sys_accept`/`accept4` route `AF_INET` to the new paths
  with peer `sockaddr_in` reporting; UDP `listen` stays `EOPNOTSUPP`.
- Cooperative-scheduler fix: `nap()` now yields every 16th round.
  Blocking TCP waits previously spun without ever scheduling, so the
  first true two-task guest rendezvous starved its peer (client recv
  timed out while the server never ran). IRQ-time `poll()` only moves
  frames; progress between tasks needs the yield.

## Tests added

- `DIFF_TCPSRV_OK`: bind, duplicate-bind `EADDRINUSE`, refused-connect
  `ECONNREFUSED`, listener `getpeername` `ENOTCONN`, forked client over
  `127.0.0.1` (peer verification, 8-byte echo each way, orderly EOF),
  `accept` peer `sockaddr_in` verification.
- `DIFF_UDPLOOP_OK`: loopback `sendto`/`recvfrom` with source address,
  connected-datagram peer reporting.

## Verification

- `cargo +nightly build -Zbuild-std=core --release` clean.
- `./tools/acceptance-test.sh` PASS.
- `./tools/differential-test.sh` PASS three consecutive runs, guest and
  native logs contain `DIFF_TCPSRV_OK` and `DIFF_UDPLOOP_OK`.
- `./tools/busybox-test.sh` PASS (115 checks match Linux).
- `./tools/heap-test.sh` PASS.
- `./tools/check-source.sh` pass.

## Known limits

- One fixed accept queue of 4 per listener; excess SYNs drop and rely on
  client retransmission. No `SYN` cookies.
- No window scaling, SACK, or congestion control; single in-flight MSS
  behavior as before. No core dumps / `CLD_DUMPED` as before.
- Files stay capped at 8 pages (32 KiB) as before.
- External (host-initiated) connections still need `hostfwd` plumbing;
  only guest-internal loopback and outbound user-networking are covered.

## Next candidates

- `hostfwd` + host-side connector so the host can reach guest servers
  (real client/server split across the boundary).
- `AF_UNIX` datagram addresses (`/dev/log`-style IPC).
- Writable ext2 behind a test disk image attached to the QEMU command
  lines.
- SMP bring-up.
