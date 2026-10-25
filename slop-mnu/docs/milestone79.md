# Milestone 79: dirfd-relative *at

## Goal

Until now every `*at` syscall except the bare path forms rejected any
directory fd with `EINVAL` (or `ENOSYS`), so any program that avoids
absolute paths – which is nearly all real software, from coreutils to
CPython – stopped at the first `openat`. This milestone resolves
relative paths against directory fds, like Linux, plus `renameat2`
and `AT_EMPTY_PATH` file-descriptor execution.

## Native behavior observed

Probes fixed the contract before any kernel code changed:

- a non-directory fd as dirfd reports `ENOTDIR`; a bad fd reports
  `EBADF`; an absolute path ignores dirfd.
- `fstatat(AT_FDCWD, "", buf, AT_EMPTY_PATH)` stats the working
  directory itself (it succeeds, it does not report `ENOENT`).
- `renameat2` with `RENAME_NOREPLACE` reports `EEXIST` when the target
  exists; other flag bits report `EINVAL`.
- `statx` flag bits beyond the mask/type set are accepted and ignored.

## Implementation

- `kernel/linux_abi/mod.rs`: `fd_abs_path` / `dirfd_abs_path`
  snapshots (the open fd's absolute path is recorded at open, so a
  later `rename` cannot confuse it), `at_resolve` joins relative
  paths onto the dirfd and reuses the existing
  root/chroot-aware resolver. All `*at` syscalls
  (`openat`, `mkdirat`, `unlinkat`, `renameat`, `linkat`,
  `symlinkat`, `readlinkat`, `newfstatat`, `faccessat`,
  `fchmodat`, `fchownat`, `statx`, `utimensat`, `execveat`) go
  through it with native `AT_FDCWD`/`AT_SYMLINK_NOFOLLOW`/
  `AT_REMOVEDIR`/`AT_EMPTY_PATH` semantics.
- `renameat2` (syscall 316) is newly dispatched with `0` and
  `RENAME_NOREPLACE` supported; exotic flags report `EINVAL`.
- `AT_EMPTY_PATH` with an empty path operates on the fd itself
  (`newfstatat`, `statx`, `fchmodat`, `fchownat`, `utimensat`) or
  executes it (`execveat`); with `AT_FDCWD` it stats the cwd.
- `sys_execve` was split into resolve + `sys_execve_resolved` so
  `execveat` reuses the whole loader/CR3/`argv` path.
- Incidental fix: the in-kernel `statx` layout did not match the
  published UAPI (`ino` belongs at offset 32, not 36; timestamps are
  16 bytes, not 24). The new `*at` battery, which compares `stx_size`
  byte for byte, caught it. Layout, uid/gid and device fields now
  follow the UAPI.

## Tests added

- `DIFF_AT_OK`: dirfd create/read, absolute-path bypass, bad-fd
  `EBADF`, file-fd `ENOTDIR`, `mkdirat` + `EEXIST`, subdir open,
  `..` through a subdir fd, `statx` (+ `AT_EMPTY_PATH`), `fstatat`
  (+ `AT_EMPTY_PATH`, cwd), `faccessat`, `fchmodat`, `fchownat`,
  `utimensat` (+ `NOFOLLOW`), `symlinkat`/`readlinkat`/`lstat`-vs-
  `stat`, `linkat` (+ follow, + `EEXIST`), `renameat`/`renameat2`
  (+ `NOREPLACE` `EEXIST`), `execveat` of the bundled helper
  through a dirfd (exit 17), `unlinkat` (+ `REMOVEDIR`).
- The link/symlink/rename leg runs on the root/tmpfs side because
  ext2 stays read-only for those operations, exactly mirroring how
  real software spans mounts with different capabilities.

## Verification

- `cargo +nightly build -Zbuild-std=core --release` clean.
- `./tools/acceptance-test.sh` PASS.
- `./tools/differential-test.sh` PASS (guest and native logs contain
  `DIFF_AT_OK`; host `e2fsck` clean, free counts exact, guest bytes
  verified on the platter).
- `./tools/busybox-test.sh` PASS (115 checks match Linux).
- `./tools/heap-test.sh` PASS.
- `./tools/check-source.sh` pass.

## Known limits

- `mknodat` stays `ENOSYS` (no device/fifo node creation yet).
- Dirfd paths are snapshots at open; a later rename of the directory
  is not tracked (Linux keeps it live via dentries).
- No `SCM_RIGHTS` ancillary data yet.
- Files stay capped at 8 pages (32 KiB) as before.
- External (host-initiated) connections still need `hostfwd` plumbing.

## Next candidates

- ext2 `symlink`/`link`/`rename` so renames and links work on disk.
- `SCM_RIGHTS` fd passing over `AF_UNIX` (real dbus/systemd IPC).
- `AF_UNIX` `SEQPACKET`.
- `hostfwd` + host-side connector so the host can reach guest servers.
- SMP bring-up.
