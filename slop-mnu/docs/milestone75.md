# Milestone 75: file plumbing for real software

## Goal

`cp`, installers and data pipelines rely on file-content syscalls that
either missed dispatch or returned `ENOSYS`: `fallocate`, `copy_file_range`,
`splice` and `tee`. This milestone wires them up with Linux-compatible
semantics, verified byte-for-byte against native Linux.

## Native behavior observed

Small native probes fixed the semantics before any kernel code changed:

- `copy_file_range`: explicit offsets advance by the moved length; null
  offsets use and advance the file positions; any flag reports `EINVAL`;
  length 0 returns 0; pipes and sockets on either side report `EINVAL`;
  directories report `EISDIR`.
- `splice`: exactly one side must be a pipe, otherwise `EINVAL`; a pipe
  side with a non-null offset reports `ESPIPE` (not `EINVAL`); unknown
  flags report `EINVAL`; length 0 returns 0; file offsets advance.
- `tee`: both sides must be pipes or `EINVAL`; the input stays queued;
  flags are ignored (a flag of 1 still duplicates); length 0 returns 0.
- `fallocate` mode 0 extends with zeroed bytes; `FALLOC_FL_KEEP_SIZE`
  reserves without changing the size; `KEEP_SIZE|PUNCH_HOLE` zeroes the
  covered range; unknown mode bits report `EOPNOTSUPP`; length 0 reports
  `EINVAL` (not success).

## Implementation

- `pipe::peek`: non-consuming read that blocks on empty pipes like `read`
  but neither advances the head nor wakes writers.
- `sys_copy_file_range`: files only (`EISDIR` for directories, `EINVAL`
  for the rest), explicit offsets via `read_at`/`write_at`, null offsets
  via the offset-advancing `read`/`write`, 512-byte chunk loop.
- `sys_splice`: pipe on at least one side, `ESPIPE` for pipe+offset,
  `EINVAL` for two non-pipes, directories, or bad flags; file offsets
  come from the pointer or the current position and are written back or
  advanced accordingly.
- `sys_tee`: pipe-to-pipe only, `peek` plus `write` loop, flags ignored.
- `sys_fallocate`: mode 0 extends via a new `vfs::reserve_ino` (grows
  zeroed capacity up to the 8-page file limit) plus `truncate_ino`;
  `KEEP_SIZE` reserves only; `PUNCH_HOLE` zeroes the live range;
  anything else reports `EOPNOTSUPP`.
- Dispatch cleanup: the live `FALLOCATE`/`SPLICE`/`TEE`/`COPY_FILE_RANGE`
  arms replace the stale `ENOSYS` ones, and provably unreachable duplicate
  arms for already-working calls (`inotify`, `signalfd`, `timerfd`,
  `memfd_create`) are removed.

## Tests added

- `DIFF_CFR_OK`: content and offset advancement for explicit and null
  offsets, `EINVAL` flags, zero length, pipe and directory errors.
- `DIFF_SPLICE_OK`: file-to-pipe and pipe-to-file moves with offsets,
  `ESPIPE`, file-file and bad-flag `EINVAL`, zero length.
- `DIFF_TEE_OK`: duplication without consumption, file-side `EINVAL`,
  zero length, ignored flags.
- `DIFF_FALLOC_OK`: zero-filled extension, `KEEP_SIZE` size stability,
  `PUNCH_HOLE` zeroes, bad-mode `EOPNOTSUPP`, zero-length `EINVAL`.

## Verification

- `cargo +nightly build -Zbuild-std=core --release` clean.
- `./tools/acceptance-test.sh` PASS.
- `./tools/differential-test.sh` PASS three consecutive runs, guest and
  native logs contain all four new markers.
- `./tools/busybox-test.sh` PASS (115 checks match Linux).
- `./tools/heap-test.sh` PASS.
- `./tools/check-source.sh` pass.

## Known limits

- File size stays capped at 8 pages (32 KiB); beyond that `ENOSPC`, where
  native filesystems would succeed. The differential tests stay small.
- Same-file overlapping `copy_file_range` uses a forward chunk copy.
- `sendfile` still ignores the offset pointer (uses the file position).
- No `syncfs`; `fsync`/`fdatasync` are accepted as no-ops on tmpfs.
- `PIDFD_GETFD`, `MKNODAT` and `USERFAULTFD` remain `ENOSYS`.

## Next candidates

- TCP passive open (`listen`/`accept` for `AF_INET`) with `hostfwd` test
  plumbing, so real servers run in the guest.
- `AF_UNIX` datagram addresses (`/dev/log`-style IPC).
- Writable ext2 with a test disk image attached to the QEMU command lines.
- SMP bring-up.
