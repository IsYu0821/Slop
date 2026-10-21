# Milestone 33: dynamic linking

The kernel now runs dynamically linked Linux executables. `/bin/hello-dyn` is a
musl-linked binary that prints through libc, and the acceptance chain execs it
as its final stage.

## Interpreter loading

`elf::plan` records the `PT_INTERP` segment offset and length.
`exec_runtime::load_static_args` resolves the interpreter path, loads that ELF
from the initramfs, and maps both images before entering:

- The interpreter is mapped at `LD_BASE` (0x4000_0000_0000) and the PIE main
  image above it at `EXE_BASE` (0x4001_0000_0000), both inside the same PML4
  slot. A static `ET_EXEC` keeps its absolute entry-derived slot, so existing
  static binaries are unaffected.
- Entry is the interpreter's entry point (`LD_BASE + interp.entry`); the
  interpreter performs symbol resolution and relocation itself.
- The initial stack carries `AT_BASE` (interpreter base), `AT_PHDR`/`AT_PHENT`/
  `AT_PHNUM` and `AT_ENTRY` describing the main image at its load bias.

The initramfs carries `/lib/libc.so` and `/lib/ld-musl-x86_64.so.1` (the same
musl shared object under both names) plus `/lib` as a directory node.

## RELRO fix

`mprotect` previously returned `EINVAL` when the target range had no VMA
ledger entry. The dynamic linker mprotects its own `PT_GNU_RELRO` range right
after relocation, which lives in the execve image and therefore has no ledger
entry. PTE permission changes still succeed; the ledger update is now treated
as best-effort bookkeeping.

## Bugs fixed along the way

- The private PML4 slot was derived from `max(exe_base, LD_BASE)` for every
  image, which broke static `ET_EXEC` binaries whose segments live in the slot
  implied by their absolute entry point. The slot is now chosen per image kind.
- `mprotect` returning `EINVAL` for ledger-less ranges made every musl
  dynamic binary abort with "RELRO protection failed".

## Deliberate limits

No `dlopen` of paths outside the initramfs, no `LD_LIBRARY_PATH` handling, no
`LD_PRELOAD`, no lazy binding (`RTLD_LAZY` behaves as `RTLD_NOW`), and no
`TLS`-heavy workloads beyond what musl's initial-exec model needs. A
non-PIE dynamically linked binary is not supported.
