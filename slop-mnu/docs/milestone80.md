# Milestone 80: rename, link and symlink on ext2

## Goal

Until now the on-disk filesystem could create, write and delete, but
not rename, hard-link or symlink: `rename`, `link` and `symlink` on
`/mnt` failed with `EROFS`, so `mv`, `ln` and anything resolving a
symlink on disk was impossible. This milestone completes the classic
trio on ext2, byte-identical to Linux.

## Native behavior observed

Probes fixed the contract before any kernel code changed:

- `link` of a directory reports `EPERM`; linking onto an existing
  name reports `EEXIST`; linking through a symlink follows it.
- `rename` onto an existing empty directory replaces it; onto a
  non-empty directory reports `ENOTEMPTY`; file-onto-directory
  reports `EISDIR`; directory-onto-file reports `ENOTDIR`; moving a
  directory into its own subtree reports `EINVAL`; renaming onto
  itself succeeds silently.
- `symlink` stores an arbitrary target verbatim; `readlink` returns
  it byte for byte; `lstat` reports the link, `stat` follows it.

## Implementation

- `kernel/ext2.rs`: `link_path` (nlink++, dirent insert, symlink
  targets resolved first like Linux), `rename_path` (replace files
  and empty dirs, `..` rewrite plus parent link-count fixups on
  directory moves, subtree/self guards), `symlink_path` (fast
  symlinks inline the target in the block area when it fits 60
  bytes, longer targets use data blocks up to 4 KiB),
  `readlink_into` (both layouts).
- `vfs.rs`: `readlink_content` routes symlink reads per inode kind
  (ext2 via the new reader, memory inodes via their pointer);
  `resolve_inner` follows ext2 symlinks through it, so `open`,
  `stat` and friends traverse on-disk links like Linux. `vfs::link`
  on the root filesystem now resolves its final-component symlink
  too (previously it linked the link itself).
- `vfs::rename`/`vfs::link`/`vfs::symlink` dispatch to the new ext2
  paths instead of `EROFS`; tmpfs keeps `EOPNOTSUPP` for links.
- `stat`/`lstat`/`fstat`/`newfstatat`/`statx` now report the real
  ext2 link count (`st_nlink`, `stx_nlink`) instead of a hardcoded 1;
  `ls`, `find` and link-count checks observe the truth. Other
  filesystems keep reporting 1.
- Incidental fix: `unlink` of a symlink failed with `EINVAL`
  because the cleanup called `truncate_file`, which refuses
  non-regular files. Symlink blocks are freed directly now (fast
  symlinks, whose block area holds target text rather than block
  numbers, are detected via `i_blocks == 0` and never passed to the
  block allocator).

## Tests added

- The `DIFF_EXT2_OK` battery is extended in place (net-zero files and
  blocks, so the host platter gates stay exact): hard-link create/
  content/nlink/unlink round trip, symlink create/`readlink`/`lstat`/
  `stat`-through/open-through round trip, a 100-byte slow-symlink
  round trip exercising the block-backed layout, file rename there
  and back, empty-directory rename.
- The `DIFF_AT_OK` battery already covers `linkat`/`symlinkat`/
  `readlinkat`/`renameat`/`renameat2` through dirfds (on the
  fully-capable root side); ext2 adds the on-disk side.

## Verification

- `cargo +nightly build -Zbuild-std=core --release` clean.
- `./tools/acceptance-test.sh` PASS.
- `./tools/differential-test.sh` PASS (guest and native logs contain
  `DIFF_AT_OK` and the extended `DIFF_EXT2_OK`; host `e2fsck` clean,
  free counts exact, guest bytes verified on the platter).
- `./tools/busybox-test.sh` PASS (115 checks match Linux).
- `./tools/heap-test.sh` PASS.
- `./tools/check-source.sh` pass.

## Known limits

- ext2 symlinks cap the target at 4 KiB; longer targets report
  `ENAMETOOLONG`.
- No `mknod` on any filesystem yet (`ENOSYS`).
- Directory `nlink` on ext2 still reports the stored count only
  where the tests observe it; `stat` elsewhere keeps reporting 1.
- Files stay capped at 8 pages (32 KiB) as before.
- External (host-initiated) connections still need `hostfwd` plumbing.

## Next candidates

- ext2 block accounting follow-ups (directory `nlink` truth, free-space
  reporting beyond the current exact-count gate).
- `SCM_RIGHTS` fd passing over `AF_UNIX` (real dbus/systemd IPC).
- `AF_UNIX` `SEQPACKET`.
- `hostfwd` + host-side connector so the host can reach guest servers.
- SMP bring-up.
