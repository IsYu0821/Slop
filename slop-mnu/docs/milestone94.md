# Milestone 94 — File-backed execve from ext2/ext4 (streaming ELF loader)

Executables whose bytes are not memory-resident (the ext2/ext4 root disk)
now load and run. The loader streams PT_LOAD segments page by page straight
into the freshly mapped frames of the new address space through
`vfs::read_off`, so no contiguous image copy is needed for images of any
size. `PT_INTERP` interpreters resolve the same way (initramfs-resident
or disk-streamed), and the same loader path now serves every execve.

## Why it crashed before

Anything nontrivial died immediately at user RIP `0x0` (`#PF`, error
`0x14`/`0x4`) after one or two syscalls — static binaries, static-PIE,
and the glibc interpreter — while trivial programs occasionally got
through. The image bytes were exonerated early: FNV-1a digests of the
loaded pages showed the text segment was byte-exact and only the
unaligned segments corrupted. The bug was in the `ByteSrc::File` branch
of `map_segment` (kernel/exec_runtime.rs):

```rust
let pa = root.translate(access, cur)?;   // (frame & !4095) | (cur & 4095)
core::ptr::copy_nonoverlapping(chunk[page_off..cend].as_ptr(),
    (phys_to_virt(pa) + page_off as u64) as *mut u8, n);  // BUG: off added twice
```

`translate()` already returns the physical address of the byte, so the
write went to `frame + 2*page_off`. The first unaligned chunk of every
unaligned segment landed at the wrong frame, clobbering unrelated pages
(BSS zeros of later segments, adjacent image pages) and leaving the
segment's own partial page unwritten. Single-page-aligned segments
(`page_off == 0`) copied correctly, which is why initramfs-resident and
page-aligned disk images worked and why the corruption only showed on
real-world layouts. Fix: `phys_to_virt(pa)` with no second offset.

The fault signature follows directly: `#PF` at `RIP=0x0` (jump through a
garbage indirect target) or inside the interpreter with `cr2=0x8`/`0x0`.

The dynamic path (ld-musl/ld-linux) crashed at a different RIP every
time for the same reason: the main image's `.data`/`.got` or the
interpreter's own mapping was subtly corrupt.

## What now works (measured in this milestone)

- `execve` of ET_EXEC (static), ET_DYN static-pie, and ET_DYN dynamic PIE
  with PT_INTERP, from both ext2 and ext4 images, with the interpreter
  either initramfs-resident or on disk. Verified with
  `tools/fileexec-test.sh` (new gate): guest mounts the image, forks,
  `execve`s each of static / static-pie / musl-PIE binaries, and the
  harness asserts the mount result, the children's output markers
  (matching native Linux behavior, incl. hello-dyn's `rc=1`), and the
  probe's completion marker; QEMU must exit 33 (isa-debug-exit).
- Unmodified CPython 3.12.3 boots from an ext4 image — interpreter,
  glibc, stdlib and the official test suite all from disk — and passes
  the official `test_bool` suite (31 tests, `PYOFF_RESULT test_bool PASS
  93.4 ran=31 skipped=0`) through the harness's fork+execve-per-test
  subprocess model with pipes. Previously the interpreter itself died in
  `_start` (`cr2=0x8`).
- `/bin/hello` style statics keep working from the initramfs
  (acceptance PASS), and the differential battery passes with
  byte-exact platter verification (e2fsck clean, exact free counts).

## Filesystem and VFS hardening riding along

- ext2 mounts validate the superblock eagerly (magic + feature
  whitelist) like Linux: an ext4-with-extents/journal image mounted as
  ext2 used to "succeed" and fail only at first I/O; now the mount is
  refused and the offending feature bits are printed
  (`ext2: unsupported features …`). Only `FILETYPE` is whitelisted for
  incompat — everything htree/extent/journal/64bit/metadata-checksummed
  is refused, matching this driver's on-disk capabilities.
- Every mutating VFS operation now honors the per-mount read-only flag
  with `EROFS` (create/unlink/mkdir/rmdir/rename/truncate/chmod/mknod/
  symlink/link). Previously only `open(O_WRONLY|O_CREAT|O_TRUNC)`
  enforced it, so `mkdir` on the read-only ext4 legacy disk mount
  created a real directory on the platter (caught by the ext4 battery,
  repaired in this milestone).
- `/proc/mounts` reports `ro` for read-only mounts instead of always
  `rw`.
- ext4 `rmdir` marks the removed directory inode deleted
  (`i_links_count=0`, `i_dtime`, size 0) before clearing it; the host
  fsck previously reported a phantom zero-length unattached directory
  inode every run and the test printed the warning without failing.
  The mutate/write/extend batteries now gate on e2fsck success.
- `tools/smp-test.sh` and `tools/stop-test.sh` no longer depend on the
  uncommitted `/tmp/opencode/uefi-user.sh` scratch helper (same defect
  class as the ext4 tools fixed here); the stop battery passes on
  1 and 2 vCPUs.

## Boot-time and spawn surface

- `SIGKILL` is process-directed like Linux: killing a thread-group
  member marks every live member of the group, so a killed leader can
  no longer strand sleeping threads pinning fd/pipe references.
- `EXEC_MAX_VEC` 8→32 and `EXEC_MAX_STR` 128→512: real software passes
  far more than 8 arguments with longer strings.

## Harness

- `tools/python-official-test.sh`, `tests/pyofficial-driver.py`,
  `tests/pyrun.c`: the seed now uses an FHS layout
  (`usr/bin/python3`, `usr/lib/python3.12/{,lib-dynload,test}`) so
  unmodified CPython locates its stdlib exe-relative and re-executes
  subprocesses with `-E`/`-I` without `PYTHONPATH`. `tests/pyrun.c`
  execs the disk-hosted interpreter instead of the stale
  `PYTHONPATH=/mntpy/lib` scheme; the ext4 seed's own layout makes
  `python -m test.<name>` work end to end.

## Verification (all run this milestone)

acceptance PASS · differential PASS (e2fsck clean, free counts exact) ·
ext4 read-only battery PASS · ext4 mutate/write/extend PASS with new
e2fsck gates (fsck-clean images, net-zero leaks) · fileexec gate PASS
(ext2+ext4) · stop battery PASS (1/2 vCPUs) · check-source PASS ·
unmodified `test_bool` PASS in-guest from ext4 (31 tests).

## Known remaining (tracked in known-issues.md)

- KI-5: official `test_int` wedges the guest at ~127–158 guest-seconds
  on a long arithmetic stress (no #PF storm; the runone watchdog never
  regains control). `test_bool` passes end to end. Root cause not yet
  isolated; not a regression from the loader fix (the suite previously
  died at interpreter startup).
- `/init` exec chain stays BSP-pinned (MS93 scope); exec migration to
  APs remains deferred.
- Crash-consistency of the writable ext4 path (no journal, MS95).
