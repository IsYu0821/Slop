# Milestone 78: AF_UNIX datagrams carry addresses

## Goal

Until now every `AF_UNIX` datagram was a dead end: `bind` accepted the
address but `sendto` ignored it, `recvfrom` never reported a source, and
`connect`/`getpeername` only understood streams. Any program that talks
to `syslog`, `dbus` or another local datagram peer failed. This
milestone makes `SOCK_DGRAM` over `AF_UNIX` byte-identical to Linux for
the connected and unconnected cases, including abstract names.

## Native behavior observed

Probes fixed the contract before any kernel code changed:

- `bind` of a second socket to the same path reports `EADDRINUSE`;
  rebinding the same socket reports `EINVAL`.
- `send` on an unconnected datagram socket reports `ENOTCONN`;
  `sendto` to an unknown path reports `ENOENT`.
- `recvfrom` reports the sender's bound path; an unbound sender is
  reported as the empty (family-only) address.
- `connect` records the peer; `getpeername` then reports it and
  `send`/`recv` work without addresses.
- Datagram boundaries are preserved: two sends arrive as two reads.
- Oversize datagrams report `EMSGSIZE` (Linux allows ~212 KiB; the
  guest caps at 4 KiB, so the probe uses 300 KiB where both agree).
- Abstract names (leading NUL) rendezvous without the filesystem.
- `socketpair(AF_UNIX, SOCK_DGRAM)` behaves as a connected pair.
- `listen`/`accept` on a datagram socket report `EOPNOTSUPP`.

## Implementation

- `kernel/socket.rs`: per-socket datagram inbox (`8 x 4 KiB` messages,
  each with its sender's raw path). Paths are raw bytes so filesystem
  and abstract names never collide. New `bind_bytes`/`connect_bytes`
  entry points keep the `&str` wrappers for streams.
- `dgram_connect` checks pathname targets (`ENOENT` when missing);
  abstract targets may be created later.
- `dgram_sendto`/`dgram_recvfrom` block on the existing wait queues
  with `EAGAIN` for nonblocking sockets; `socketpair_kind` takes the
  socket kind so `SOCK_DGRAM` pairs connect peer-to-peer.
- `linux_abi`: `sockaddr_path` keeps the leading NUL for abstract
  names; new `write_sockaddr_un` reports bound paths; `sys_sendto`
  and `sys_recvfrom` route datagrams per-message; `sys_connect`,
  `sys_getpeername` (datagram peer, stream peer path) and
  `sys_getsockname` (unchanged) match Linux.
- `poll`/`select`/`epoll` already report sockets readable/writable,
  so datagram readiness needs no extra plumbing.
- Small ABI corrections in the same increment: `NICE` is `134`, not
  `34` (which is `PAUSE`), and the existing `sys_times` is now
  dispatched as `TIMES=100` instead of falling through to `ENOSYS`.

## Tests added

- `DIFF_UNIXDG_OK`: duplicate-bind `EADDRINUSE`, `getsockname`,
  unconnected-`send` `ENOTCONN`, `sendto`/`recvfrom` with source
  verification, connected `send`/`recv` with `getpeername`,
  boundary preservation, anonymous sender, unknown-path `ENOENT`,
  `EMSGSIZE`, abstract rendezvous and `socketpair` both ways.

## Verification

- `cargo +nightly build -Zbuild-std=core --release` clean.
- `./tools/acceptance-test.sh` PASS.
- `./tools/differential-test.sh` PASS (guest and native logs contain
  `DIFF_UNIXDG_OK`; host `e2fsck` clean, free counts exact, guest bytes
  verified on the platter).
- `./tools/busybox-test.sh` PASS (115 checks match Linux).
- `./tools/heap-test.sh` PASS.
- `./tools/check-source.sh` pass.

## Incidental fix: ext2 mkdir cleanup

Verifying this milestone exposed a real pre-existing leak: `mkdir_path`
cleaned a failed `dir_insert` (e.g. the test's deliberate duplicate
`mkdir`, expecting `EEXIST`) with `truncate_file`, which refuses
directories, so the inode was freed but its data block was not. Every
run leaked exactly one block (`e2fsck` flagged `-8190`, free counts off
by one). The cleanup now frees directory blocks directly with
`free_from`, mirroring `rmdir_path`. No new test was needed: the
existing `DIFF_EXT2_OK` plus the host `e2fsck`/count/byte gates fail
before the fix and pass after it.

## Known limits

- Datagram payloads cap at 4 KiB per message (8 queued per socket);
  larger Linux datagrams report `EMSGSIZE` earlier than on Linux.
- No `SCM_RIGHTS`/`SCM_CREDENTIALS` ancillary data yet.
- Files stay capped at 8 pages (32 KiB) as before.
- External (host-initiated) connections still need `hostfwd` plumbing;
  only guest-internal loopback and outbound user-networking are covered.

## Next candidates

- `SCM_RIGHTS` fd passing over `AF_UNIX` (real dbus/systemd IPC).
- Writable ext2 `rmdir` block accounting fix (one leaked block).
- `AF_UNIX` `SEQPACKET`.
- `hostfwd` + host-side connector so the host can reach guest servers.
- Writable ext2 behind a test disk image attached to the QEMU command
  lines.
- SMP bring-up.
