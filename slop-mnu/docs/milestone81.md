# Milestone 81: files bigger than 32 KiB

## Goal

Every page-based file (tmpfs and the initramfs root) stopped at 8
pages: anything past 32 KiB failed with `ENOSPC`, which rules out
practically every real program (compilers, archives, package
payloads). This milestone replaces the fixed 8-page array with
direct + single-indirect + double-indirect block addressing, up to
262152 pages (~1 GiB), with holes reading back as zeros like Linux.

## Native behavior observed

Probes fixed the contract before any kernel code changed:

- writes past the old cap succeed and read back byte-identical;
  `ftruncate` up extends without allocating, down keeps the size;
  holes read as zeros; shrinking then rewriting works;
- `fstat`/`statx` report the true size; `ENOSPC` appears only when
  the frame allocator is actually exhausted; absurd sizes report
  `EFBIG`.

## Implementation

- `kernel/vfs.rs`: `Inode` gains `indirect`/`dindirect` table
  addresses and a `u32` page count. `page_frame_of` resolves a file
  block with no allocation (holes stay `None`), `ensure_page_of`
  allocates the data frame plus any missing tables on the write
  path, zero-filling everything. Reads zero-fill holes within the
  file size; `write`/`ftruncate`-up/`fallocate` no longer stop at 8
  pages (`EFBIG` past ~1 GiB).
- Lifetime: `free_beyond` recycles data pages and empties tables on
  truncate-shrink and at unlink (skipped while open descriptions
  still reference the inode, which keeps working on the retained
  frames like Linux anonymous inodes). A new hard link bumps every
  frame through the page refcounts instead of duplicating them.
- Initramfs entries, static buffers and proc/sys nodes own no frames
  and are never freed; the single-page `data` fast path is kept and
  kept consistent with the page tables.
- `kernel/fd.rs`: `any_open_ino` backs the do-not-free-while-open
  rule.

## Tests added

- `DIFF_BIGFILE_OK`: a 3 MiB round trip exercising all three levels
  (direct, single- and double-indirect), full content verification,
  `stat` size, truncate to 100, a sparse rewrite at 2 MiB with hole
  zero-fill and tail verification, then unlink and `rmdir`.

## Verification

- `cargo +nightly build -Zbuild-std=core --release` clean.
- `./tools/acceptance-test.sh` PASS.
- `./tools/differential-test.sh` PASS (guest and native logs contain
  `DIFF_BIGFILE_OK`; host `e2fsck` clean, free counts exact, guest
  bytes verified on the platter).
- `./tools/busybox-test.sh` PASS (115 checks match Linux).
- `./tools/heap-test.sh` PASS.
- `./tools/check-source.sh` pass.

## Known limits

- Truncate-shrink frees now; truncate-grow leaves holes (no
  preallocation), like Linux.
- Open-but-unlinked files pin their frames until close (no
  use-after-free, but no reclamation either yet).
- Symlinks, ext2 and proc/sys paths are unchanged by this work.

## Next candidates

- ext4 read-only mount (superblock/inode/dir parsing behind feature
  checks), then staged read-write and journaling.
- `SCM_RIGHTS` fd passing over `AF_UNIX`.
- `hostfwd` + host-side connector so the host can reach guest servers.
- SMP bring-up: AP startup, per-CPU state, scheduler, IPI, TLB
  shootdown.
