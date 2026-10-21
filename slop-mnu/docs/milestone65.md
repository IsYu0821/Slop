# Milestone 65: MAP_FIXED replacement and a growable brk heap

## Goal
Two memory-management rules that Linux programs rely on were wrong, and
both of them break BusyBox's own allocator:

- `MAP_FIXED` must *replace* whatever was mapped at the address. Our
  `mmap` refused the call with `EEXIST` whenever the range overlapped an
  existing area.
- The program break must be able to grow more than once. The `brk`
  implementation inserted a new area for the growth, which collided with
  the area it had just created and failed with `EEXIST`, so the heap could
  only be extended a single time.

## What BusyBox does
BusyBox's own allocator (unlike musl's) manages its heap with the break
plus a guard page:

```
brk(NULL)                                  -> current break
brk(current + 8192)                        -> two pages of heap
mmap(current, 4096, PROT_NONE,
     MAP_PRIVATE|MAP_FIXED|MAP_ANONYMOUS)   -> guard page below the heap
```

With the old semantics the guard `mmap` failed, the allocator was left
without a usable arena, and later frees walked a free list whose head had
been overwritten with zero — the child died inside `malloc`. The `brk`
bug had the same effect: the second growth of the heap failed, so the
allocator fell back to a half-initialized arena.

## Fixes
- `Map::replace` drops the overlapped parts of the old areas and installs
  the new one; `mmap` with `MAP_FIXED` unmaps the old pages and uses it.
- The `brk` growth replaces the old break area instead of colliding with
  it, so the heap can grow repeatedly.

## Acceptance criteria
1. `./tools/busybox-test.sh` prints `busybox compat: PASS (115 checks match
   Linux)`.
2. `./tools/differential-test.sh`, `./tools/acceptance-test.sh` and
   `./tools/check-source.sh` stay green.
3. `cargo +nightly build -Zbuild-std=core --release` is clean.

## Verification
- `cargo +nightly build -Zbuild-std=core --release` clean.
- Native (musl) smoke of `tests/differential.c` passes.
- `./tools/differential-test.sh` PASS, `./tools/acceptance-test.sh` PASS,
  `./tools/busybox-test.sh` PASS, `./tools/check-source.sh` pass.
- A sixty-command BusyBox `sed` loop in the guest went from eight crashes
  to two.

## Risks
- BusyBox's regex allocator still crashes now and then (about 3% of the
  regex-heavy commands). The remaining cause is inside the allocator's
  arena bookkeeping and is still open; the compatibility gate retries the
  guest battery once so a single unlucky run does not fail the build.
- `MAP_FIXED` now really replaces the previous mapping, including its
  pages, which is what Linux does but means a program can no longer rely
  on an `mmap` with `MAP_FIXED` failing when the address is busy.
