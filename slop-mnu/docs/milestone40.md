# Milestone 40: Credentials, namespaces, seccomp, netlink, sysfs, inotify and signalfd

Added a broad Linux compatibility layer:
- per-process credentials, groups, capabilities, umask and namespace IDs
- `setuid`, `setgid`, `setreuid`, `setregid`, `setresuid`, `setresgid`, `getgroups`, `setgroups`, `capget`, `capset`, `unshare`, `setns`, `seccomp`
- netlink sockets (`AF_NETLINK`) with bind/sendto/recvfrom/getsockname
- inotify and signalfd file descriptors
- `/sys` virtual filesystem with common kernel and device entries
- namespace propagation for `clone`/`fork`

Acceptance suite remains green.
