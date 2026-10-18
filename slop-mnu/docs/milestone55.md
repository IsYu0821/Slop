# Milestone 55: Resource accounting and VFS mount namespaces

Moved mount routing from the Linux syscall layer into the VFS. Mount selection
uses the longest matching mountpoint in the caller's mount namespace. Added
fixed-capacity tmpfs stores, tmpfs file/directory operations, ext2 mount routing,
and mount-table cloning for `CLONE_NEWNS`, `fork`, and `unshare`. `mount` and
`umount2` now use the VFS registry and require `CAP_SYS_ADMIN` for the current
mount namespace. Unmounting a
mount with nested mounts returns `EBUSY`.

Added descriptor/open-file counts, VM area/byte/page statistics, and scheduler
state counts. Process exit reports resources before and after teardown, and the
acceptance test rejects incomplete address-space cleanup. The physical-frame
recycle pool now holds 65,536 pages, covering the configured 256 MiB guest; this
removes the previous 256-page teardown bottleneck.

Implemented reachable `capget`/`capset` handlers, capability checks for identity
changes and privileged namespace operations, and `PR_SET/GET_NO_NEW_PRIVS`.
Seccomp now validates supported classic-BPF instructions, rejects unsupported
flags and replacement filters, shares filter lifetime safely across forked
tasks, and returns errno/trap/kill/allow actions without treating trace and user
notification as allow.

Inotify now matches events to the correct watch descriptor, queues complete
records, reports overflow and ignored-watch events, removes state on final fd
close, and delivers create/modify events. The io_uring subset validates setup
parameters and ring indices, uses kernel-maintained SQ/CQ cursors, resolves SQE
file descriptors through the process fd table, checks user buffers, and tracks
shared ring mappings. The differential test submits a NOP and checks its CQE.

The differential test checks representative `ENOENT`, `EBADF`, and `EINVAL`
results, reads `/proc/self/status`, exercises capget/capset, seccomp, inotify and
io_uring, verifies QEMU's success exit status, and restores temporarily modified
inputs even when the run fails.

## Limits

- Mount and tmpfs tables are fixed-capacity; tmpfs file storage is limited to
  eight pages per file. Tmpfs hard links are not implemented.
- Tmpfs backing stores are retained after unmount so open inode handles remain
  valid; reclaiming an unreferenced store is future work.
- Mount flags and `mount(2)` data are not interpreted. Supported filesystem
  types are tmpfs and ext2; ext2 remains read-only.
- Seccomp supports a validated classic-BPF subset and one filter per process;
  unsupported filter actions and synchronization flags are rejected or fail
  closed. Inotify implements a subset of event sources and does not block on
  empty reads.
- io_uring supports NOP, READ and WRITE with rings up to 64 entries; registration,
  nonzero enter flags, and minimum-completion waiting are unsupported.
- Resource diagnostics cover process descriptors, VM ledger entries, and task
  slots; they are not a complete global allocator/leak detector.

## Verification

- `./tools/check-source.sh` — passed.
- `./tools/differential-test.sh` — passed for native and guest runs.
- `OVMF_CODE=/usr/share/OVMF/OVMF_CODE_4M.fd OVMF_VARS_TEMPLATE=/usr/share/OVMF/OVMF_VARS_4M.fd TIMEOUT=40 ./tools/acceptance-test.sh` — passed with no incomplete-cleanup markers.
