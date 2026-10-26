# Known issues registry

Tracked instead of forgotten. Each entry names the interim behavior,
the Linux-correct behavior, how to test it, and where the fix is
planned. Closing an entry requires its test to go green in the
normal gates.

## KI-1: open-but-unlinked files leak their frames

- Interim behavior: `unlink` (and truncate-shrink) skips frame
  recycling while any open file description still references the
  inode, so a use-after-free is impossible but the frames leak.
- Linux behavior: the data stays readable/writable through open
  fds; the last close reclaims everything, including after `fork`
  and `dup` sharing, and on process exit.
- Test: open, unlink, read/write through the old fd, create a new
  file under the same name, `fork`/`dup` sharing, close the last fd
  and verify reclamation (frame counts), plus process-exit cleanup.
- Planned fix: per-open-file lifetime tracking with deferred
  reclamation (proposed with the VFS hardening work, no later
  than the ABI track MS89-MS91).

## KI-2: file timestamps are not truthful

- Interim behavior: `mtime`/`ctime` are stamped on some mutating
  paths, `atime` is never updated, `btime` does not exist.
- Linux behavior: `mtime`/`ctime`/`atime` (`relatime` semantics at
  minimum) and `btime` where supported, consistent across `read`,
  `write`, `truncate`, `chmod`, `rename`, `link`, `unlink`,
  `symlink`, `utimensat`, `stat`/`fstat`/`statx`, `mmap` writes,
  and `make`-style timestamp comparisons.
- Test: a timestamp battery exercising every listed interaction
  natively vs in-guest, including `make` rebuilding exactly once.
- Planned fix: timestamp writes on all mutating paths plus
  `utimensat`/`futimens` truthfulness (proposed MS89 ABI track).

## KI-3: directory nlink on tmpfs/rootfs

- Interim behavior: every inode reports `nlink == 1`, including
  directories that gain and lose subdirectories.
- Linux behavior: directories report 2 + subdirectory count;
  renames and moves keep the counts exact.
- Test: mkdir/rmdir/rename round trips asserting `nlink` on both
  sides, natively vs in-guest.
- Planned fix: maintain link counts in tmpfs and rootfs directory
  operations (proposed MS89 ABI track; ext4 side already counts).

## KI-4: test timeouts under host load

- Interim behavior: the QEMU gates use one fixed timeout; on a
  loaded host a slow run is indistinguishable from a hang, and
  only the tail of the serial log survives.
- Required instrumentation (being added incrementally):
  - boot-completion marker in the guest serial log;
  - per-phase progress markers with host-side timestamps;
  - phase timeouts instead of a single global one where cheap;
  - on timeout, preserve the full serial log plus QEMU status and
    host load average in the failure output;
  - no unbounded timeout extensions: slowness beyond the budget
    stays a failure, but an *explained* one.
- Planned work: harness improvements alongside the SMP track
  (MS82-MS85), where timing-sensitive gates need them most.
- Status: differential/acceptance scripts now log phase starts
  with timestamps and dump the serial tail plus QEMU status on
  failure. Heartbeat markers inside long guest phases are still
  open.
