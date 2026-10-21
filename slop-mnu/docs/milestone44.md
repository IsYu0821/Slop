# Milestone 44: Process introspection and namespaces

Added `/proc/self` symlinks for `exe`, `root` and `cwd`, plus a `/proc/self/ns`
directory exposing namespace identifiers. `readlink` now resolves these process
introspection links.

Acceptance suite remains green.
