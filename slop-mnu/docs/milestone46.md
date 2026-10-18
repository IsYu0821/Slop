# Milestone 46: Mount table and path rewriting

Added a mount table, `mount`/`umount2` handling and mount-aware path resolution.
Absolute paths are rewritten through the longest matching mountpoint before VFS
lookup, enabling bind-style path translation.

Acceptance suite remains green.
