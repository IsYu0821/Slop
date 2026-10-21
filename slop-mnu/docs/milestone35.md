# Milestone 35: Unix domain sockets

`AF_UNIX` stream sockets work end to end. `/bin/hello-socket` is a musl-linked
program that runs a `socketpair` round trip and a fork-based
`bind`/`listen`/`connect`/`accept` session.

## Socket layer

`kernel/socket.rs` owns a fixed 16-socket table. Each endpoint has a kind, a
state (`Unbound`/`Listening`/`Connected`), an optional bound path, a peer
index and a 4 KiB ring buffer with `readq`/`writeq` wait queues, mirroring the
pipe implementation. `Socket` was added to `OpenKind`, so `read`/`write`,
`pread64`/`pwrite64`, `close` and `fstat` route through it; `seek` reports
`ESPIPE`.

`connect` now establishes the pair immediately: it allocates the server
endpoint, links both sides, and parks `(client, server)` in the listener's
pending queue. `accept` takes the pair and hands the server descriptor over,
so a client can send before the server accepts — matching Linux stream
semantics. Previously the pair was only linked inside `accept`, which made the
client's first `write` fail with `ENOTCONN`.

## Syscalls

`socket`, `bind`, `listen`, `accept`, `connect`, `sendto`, `recvfrom`,
`socketpair`, `shutdown` and `getsockname` are implemented, with
`sockaddr_un` family validation. `read` on a socket returns one `recv`'s
worth instead of looping to the requested length; looping blocked waiting for
bytes the peer never promised.

## fork

musl's `fork` calls `SYS_fork` (57), not `clone`; the dispatcher previously
only handled `clone`, so `fork` returned `ENOSYS` and every musl program that
forked failed. `sys_fork` now shares the `fork_process` path.

## Deliberate limits

`SOCK_DGRAM` is accepted but behaves like a stream, `SO_PEERCRED` and
`SO_SNDBUF` options are unimplemented, there is no `sendmsg`/`recvmsg` scatter
support, socket files are not visible in the VFS, and `accept` always blocks.
