# Milestone 39: Symlinks, procfs and timerfd

Added VFS symlink and hard-link support with symlink-aware path resolution,
`/proc` virtual files (`self`, `status`, `stat`, `cmdline`, `environ`, `maps`,
`cpuinfo`, `meminfo`, `uptime`, `version`, `loadavg`, `fd`), and `timerfd`
support. `readlink`, `symlink`, and `link` syscalls are implemented; `lstat`
does not follow the final component.

Acceptance suite remains green.
