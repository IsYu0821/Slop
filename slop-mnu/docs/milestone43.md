# Milestone 43: Socket API completion

Added `setsockopt`, `getsockopt`, `sendmsg`, `recvmsg` and `accept4` support.
`sendmsg`/`recvmsg` aggregate iovec buffers; socket options are accepted with
zero values; `accept4` honors the non-blocking/cloexec flag mask.

Acceptance suite remains green.
