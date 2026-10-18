# Milestone 48: seccomp BPF filters

Added classic BPF evaluation for `SECCOMP_SET_MODE_FILTER`, including loads,
stores, ALU, jumps, returns and scratch memory. Filters can allow, errno, trap
or kill syscalls.

Acceptance suite remains green.
