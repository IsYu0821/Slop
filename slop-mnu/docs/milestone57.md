# Milestone 57: procfs truth and process introspection

## Goal
Replace the placeholder `/proc` with real per-process and system data so that
process-inspection tools and musl programs see truthful state: actual command
lines, environment, memory maps, residency, scheduler identity, and descriptor
link targets. Target: `/proc/self/*`, `/proc/<pid>/*` and the system files parse
identically to Linux for the fields Slop/MNU implements.

## Scope
- `/proc/self/{cmdline,environ,status,stat,statm,maps}` from live process state.
- `/proc/<pid>/{cmdline,environ,status,stat,statm,maps,exe,root,cwd}` for live
  processes (no threads-as-processes; `Tgid`/`Threads` reflect CLONE_THREAD).
- `/proc/self/{fd,ns}` directory enumeration and dynamic `/proc/self/fd/N`
  readlink targets (`pipe:[N]`, `socket:[N]`, `anon_inode:[...]`, paths).
- System files: `/proc/{meminfo,uptime,version,cpuinfo,loadavg,mounts}` from
  frame allocator, monotonic clock, scheduler and mount registry.
- `readlink`/`readlinkat` route through the VFS for all `/proc` symlinks
  (previously only `/proc/self/exe` in `readlinkat`).
- `madvise(MADV_DONTNEED|MADV_FREE)` discards anonymous pages; `MAP_POPULATE`
  eagerly faults mappings; `mincore` reports true PTE residency.

Out of scope (later milestones): `/proc/<pid>/fd`, smaps, task subdirectories,
per-thread stat files, writable proc files, sysctl.

## Data model

### `exec_runtime.rs` — recorded exec metadata
`Loaded` carries what `/proc` must report:
```rust
pub struct MapSeg { pub start: u64, pub end: u64, pub flags: u32 } // PT_LOAD perms
pub cmdline: [u8; 512], cmdline_len: usize,   // argv packed with NULs
pub environ: [u8; 512], environ_len: usize,   // envp packed with NULs
pub segs: [MapSeg; MAX_MAP_SEGS], nsegs: usize,
pub stack_base: u64, stack_top: u64,
```
`load_static_args` records interpreter and main-object `PT_LOAD` extents plus
the initial stack bounds and packs `argv`/`envp` via `pack_vec`.

### `process.rs`
- New fields mirroring the above; `fork_child` inherits them unchanged.
- `set_exec_meta(&Loaded)` stores exec metadata on `execve`.
- `vma_bytes()` sums VMA extents; `resident_pages()` walks the page table and
  counts present user leaves.
- `madvise(addr, len, advice)`: `DONTNEED`/`FREE` unmap present pages inside
  valid VMAs (ENOMEM outside), other advice returns success. Pages re-fault as
  zero-filled, matching anonymous DONTNEED semantics.
- `mmap`: allow `MAP_POPULATE`; after inserting the VMA, fault every page in via
  `handle_page_fault` (rollback on failure), so `mincore` reports residency.

### `task.rs`
- `with_process_pid(pid, f)`: read-only view of another live process.
- `live_pids(cb)`, `ppid_of(pid)`, `thread_count(pid)`, `last_pid()` for
  `/proc` enumeration, `PPid`, `Threads`, and `loadavg`.

### `vfs.rs` — proc routing
- `parse_proc(path) -> Option<(ProcSubject, &str)>` recognizes `/proc/self`
  and numeric PIDs; `proc_with` resolves either subject to a `Process`.
- `proc_fill(subject, file)` renders cmdline/environ/status/stat/statm/maps and
  the system files into `PROC_BUF` (capacity 4096, Linux-style truncation).
  - `maps`: `PT_LOAD` segments with `rwxp` perms from ELF flags, VMAs with
    `rwxp` from `VmFlags` and `[heap]`/`[stack]` labels, file mappings with
    offset and inode number.
  - `stat`: pid/comm/state/ppid/pgrp/session, utime/stime `0`, priority `20`,
    `Threads`, `VmSize` (bytes), `VmResident` (pages), remaining fields zero.
  - `status`: `Name` (basename of `exe_path`), `State`, `Tgid`/`Pid`/`PPid`,
    `Uid`/`Gid` quadruples, `Threads`, `VmSize`/`VmRSS` in kB.
  - `meminfo` from `memory::frame_stats()` (total/free frames, 4 KiB frames).
  - `uptime` from `time::monotonic_ns()` as `seconds.centiseconds`.
  - `loadavg` from live PID count and `last_pid`; `mounts` from the mount
    registry filtered by the caller's mount namespace.
- `readdir` enumerates `/proc` (self + live PIDs + system files) and
  `/proc/self/fd` dynamically from `FdTable::open_fds`; `/proc/<pid>` lists the
  per-process files and links.
- `/proc/self/fd/N` readlink targets are derived from `fd::kind`: console path,
  inode path, `pipe:[N]`, `socket:[N]`, and `anon_inode:[...]` forms.
- `proc_symlink` materializes link targets in a frame; generic
  `sys_readlink`/`sys_readlinkat` now serve every `/proc` link.

### ABI
- `madvise` dispatch wired to `Process::madvise`.
- `mincore` validates alignment, ENOMEM for unmapped ranges, then reports
  present leaves per page into the user vector.
- `MAP_POPULATE` accepted in the mmap flag mask.

## Acceptance criteria
1. `./tools/acceptance-test.sh` prints `acceptance: PASS` with all resource
   reclamation markers intact.
2. `./tools/differential-test.sh` prints `differential test: PASS`, including
   new host/guest parity cases:
   - `DIFF_PROCFS_OK`: status/cmdline/stat/pid-path/maps/uptime/meminfo shapes
     and `fd/1` readlink format.
   - `DIFF_MADVISE_OK`: `MADV_DONTNEED` zeroes anonymous pages on next read.
   - `DIFF_MINCORE_OK`: residency bits for touched vs untouched pages, and
     `MAP_POPULATE` makes a fresh mapping fully resident.
3. `./tools/check-source.sh` passes.

## Verification
- `cargo +nightly build -Zbuild-std=core --release` clean.
- `OVMF_CODE=... OVMF_VARS_TEMPLATE=... ./tools/acceptance-test.sh` PASS.
- `./tools/differential-test.sh` PASS (native Linux vs QEMU guest).
- `./tools/check-source.sh` pass.

## Risks
- `/proc` files are rendered at lookup into a single shared buffer; a second
  lookup between `open` and `read` replaces the contents. Slop/MNU's inode copy
  keeps `size` and page pointers, so readers still see a consistent snapshot
  only if they open+read immediately; acceptable for this milestone.
- `PROC_BUF` is 4096 bytes; large maps lists truncate as on Linux.
- `proc_symlink` allocates a frame per lookup and those frames are not yet
  reclaimed; deferring reclaim keeps the link path allocation-free at read
  time and is bounded by lookup frequency during this milestone.
