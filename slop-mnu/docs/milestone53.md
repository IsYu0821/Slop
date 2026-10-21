# Milestone 53: ABI hardening and differential testing

Added a musl-based differential test harness and test program exercising file,
directory, process, stat, cwd, and identity syscalls. The harness compares a
native Linux run with the OS guest run.

Acceptance suite remains green.
