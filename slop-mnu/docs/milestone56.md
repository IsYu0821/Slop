# Milestone 56: Memory completeness

## Goal
Make the virtual-memory subsystem production-capable: demand-paged anonymous and
file-backed mappings, copy-on-write fork, lazy brk, correct unmap/teardown under
sharing, and user-mode page-fault recovery. Target: musl-linked binaries with
`malloc`/`fork`/`exec` heavy workloads run unmodified with zero frame leaks.

## Scope
- Anonymous mmap becomes lazy (reserve VMA, allocate on first touch).
- File-backed mmap (read) becomes lazy and shareable; private writes use COW.
- `fork` shares pages read-only with reference counts instead of deep-copying.
- Write faults to a shared anonymous page allocate a private copy.
- `brk` growth reserves pages; faults allocate zeroed frames.
- `mremap(MREMAP_MAYMOVE|FIXED)`, `mlock`/`munlock`, `mincore`.
- User-mode `#PF` handler resolves faults against the VMA ledger before
  terminating the process.

Out of scope (M60): writable shared file-backed mappings, page cache write-back,
`msync`, `madvise` beyond hints.

## Data structures

### `memory.rs` — frame reference counts
Add a fixed-capacity refcount table sized to the configured guest (65536 frames
for 256 MiB). Frames are indexed by `pa / PAGE_SIZE`.

```rust
static REFCOUNTS: SpinLock<[u16; MAX_FRAMES]> = ...;
pub fn refcount(pa: u64) -> u16;
pub fn refcount_inc(pa: u64);
/// Decrements; returns true if the caller must free the frame.
pub fn refcount_dec(pa: u64) -> bool;
```

`alloc_frame` initializes the slot to 1. `free_frame` is unchanged for
non-shared (kernel) callers; user paths use `release_frame` which decrements and
frees only at zero. External (io_uring) mappings keep their own refcount and are
untouched.

### `vm.rs` — VMA kind and metadata
Extend `Area`:

```rust
pub enum VmKind { Anon, File { ino: u64, offset: u64 }, Stack, Brk }
pub struct Area {
    pub start: usize, pub end: usize,
    pub flags: VmFlags,
    pub kind: VmKind,
    pub cow: bool,        // read-only shared, copy on write
}
```

`Map` gains `find_vma(addr) -> Option<Area>` (already derivable from
`flags_at`/iteration) and `set_cow(start, end, bool)`.

### `process.rs`
- `map_zeroed` is removed from the eager path; `mmap` reserves the VMA only.
- New `handle_page_fault(addr: u64, write: bool) -> Result<(), ()>`:
  1. Look up VMA containing `addr`. Missing => return Err (caller delivers
     SIGSEGV).
  2. If present PTE already maps the page:
     - write fault on a COW VMA => allocate copy, map writable, decrement old.
     - otherwise => Err.
  3. If no PTE:
     - Anon/Brk/Stack => allocate zeroed frame, map with VMA prot.
     - File => allocate frame, read file page (respecting EOF -> zero), map
       read-only (or per prot for private).
     - If VMA is COW-shared, map read-only and set cow.
  4. Update TLB via invlpg (map_page already does).
- `clone_address_space` (fork): instead of `copy_slot` deep copy, mark both
  parent and child pages read-only, set `cow=true` on shared VMAs, increment
  frame refcounts. The PML4 slots are cloned at the table level (shallow) with
  pages shared.
- `teardown_exit`/`unmap_pages`/`rollback` use `release_frame` for non-external
  pages so shared frames survive until last unmap.
- `brk`: reserve `[old_page, new_page)` in the VMA ledger (kind=Brk) without
  allocating; faults fill.

## Page-fault path
`arch/x86_64/idt.rs` (or exceptions.S trampoline) already routes `#PF` to
`exit_fault`. Change the user-mode branch:

```
if user_mode {
    if process::handle_page_fault(cr2, err_code & 2 != 0).is_ok() { return; } // iretq
    task::exit_fault(vector, err, rip, cr2);
}
```

Kernel-mode faults remain fatal (no recovery this milestone). `usercopy` faults
are still unrecoverable and stay fatal; exception-table recovery is M63.

## COW details
- On fork, after shallow-cloning page tables, walk each shared slot and clear
  `WRITABLE` on every present leaf PTE in both parent and child, setting the
  software COW marker. Linux uses a software bit in the PTE; here we use the
  VMA `cow` flag plus read-only PTEs (sufficient because VMA lookup decides).
- Write fault: if PTE present and read-only and VMA.cow:
  - if refcount(pa) == 1: just re-map writable (no copy needed).
  - else: alloc frame, copy 4 KiB, dec old refcount, map new frame writable,
    set VMA.cow=false for that page range (split VMA if needed — for simplicity,
    split the VMA at fault page boundaries on first COW write).
- VMA splitting: implement `Map::split(start, len)` to carve a non-COW sub-range
  so accounting stays accurate. If splitting exhausts `MAX_AREAS`, fall back to
  marking the whole VMA non-COW (acceptable, slightly over-approximates).

## Demand paging for anonymous mmap
- `mmap` (anon): validate, insert VMA with kind=Anon, flags from prot, cow=false.
  No frame allocation. Return start.
- First access faults -> `handle_page_fault` allocates.
- `MAP_POPULATE`: eager `map_zeroed` (keep for compatibility).
- `munmap`: walk pages, unmap + release_frame each present PTE, remove VMA.

## File-backed mmap (read, private)
- VMA kind=File{ino, offset}. Fault reads the page from the inode into a fresh
  frame. For tmpfs, use `vfs::read_off` into the frame; for ext2, `ext2::read`.
- MAP_PRIVATE writes: mark VMA cow=true, PTE read-only; first write copies.
- MAP_SHARED writes: deferred to M60 (return ENODEV for writable shared file
  mappings, as today).

## brk
- `brk` reserves VMA range kind=Brk; faults allocate zeroed frames. Shrink
  (`new_page < old_page`) remains unsupported (return old), matching current
  behavior, to avoid complex unmap of partial ranges.

## mremap
- Existing implementation copies bytes and frees old. Keep, but update to use
  `release_frame` and to preserve VMA kind. Add `MREMAP_FIXED`: unmap target
  range first (release frames), then move.
- If old range is COW-shared, materialize copies before moving (current copy
  loop already does).

## mlock / munlock / mincore
- `mlock`: set a `locked` bit on the VMA; no reclaim exists, so correctness is
  tracking only. `RLIMIT_MEMLOCK` check (stub: allow).
- `munlock`: clear bit.
- `mincore`: for each page in range, report resident if a PTE is present
  (all mapped pages are resident in this model).

## Acceptance criteria
1. `build-uefi.sh` still produces a bootable image; acceptance test passes.
2. Differential test passes (host + guest) including new mmap/fork tests added to
   `tests/differential.c` (mmap lazy touch, fork COW write, brk growth, mremap).
3. New self-tests:
   - `vm::cow_self_test()` in boot: fork a process, write to a shared page in
     child, verify parent value unchanged and frame counts balance.
   - `memory::refcount_self_test()`.
4. Soak: run `/bin/differential` in a loop 1000 times under QEMU; no
   "cleanup incomplete" markers and stable frame count.

## Implementation order (within M56)
1. `memory.rs` refcount API + self-test.
2. `vm.rs` Area extension + `find_vma` + self-test update.
3. `process.rs` `handle_page_fault` + lazy anonymous mmap + lazy brk.
4. Wire `#PF` user branch in `idt.rs`.
5. COW fork: shallow clone + refcount + read-only PTEs.
6. File-backed lazy mmap (read).
7. `mremap` FIXED, `mlock`/`munlock`/`mincore` syscalls.
8. Tests + differential updates.

## Risks
- Lazy mmap changes observable timing; programs that rely on eager OOM (touch
  all pages) still fault on access, preserving semantics.
- COW VMA splitting may exhaust `MAX_AREAS` (64); if so, keep whole-VMA COW
  and accept slight over-sharing (correct but less granular).
- Frame refcount table must cover io_uring external frames without double
  counting; external frames are never in refcount table (they use their own
  `IOURING_MAP_REFS`).
- Kernel-mode faults remain fatal; `usercopy` recovery deferred to M63.

## Verification
- `./tools/check-source.sh` and `./tools/check-toolchain.sh` pass.
- `cargo +nightly build -Zbuild-std=core --release` clean.
- `./tools/build-uefi.sh` succeeds.
- `OVMF_CODE=... OVMF_VARS_TEMPLATE=... ./tools/acceptance-test.sh` prints
  `acceptance: PASS` with no incomplete-cleanup markers.
- `./tools/differential-test.sh` prints `differential test: PASS`.
