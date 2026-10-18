# Milestone 45: Process descriptors and memory compatibility

Added `pidfd_open` and `pidfd_send_signal`, made file-backed `mmap` writable
for private mappings, and expanded socket compatibility with `setsockopt`,
`getsockopt`, `sendmsg`, `recvmsg`, `accept4`, `sendmmsg` and `recvmmsg`.

Acceptance suite remains green.
