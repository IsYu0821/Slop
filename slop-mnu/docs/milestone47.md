# Milestone 47: Per-namespace hostname and seccomp strict mode

Added per-process hostname state reflected in `uname` and `sethostname`, plus
seccomp strict-mode enforcement in the syscall dispatch path.

Acceptance suite remains green.
