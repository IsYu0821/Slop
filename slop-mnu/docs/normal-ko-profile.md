# Normal-kbuild .ko profile (Track C1 target)

Fixed compatibility profile for removing the Slop-specific rebuild.
Do NOT design against assumptions; this file is the mechanically
enumerated ground truth for the pinned toolchain.

## Pinned environment

- Target Linux series: Ubuntu `7.0.0-38-generic` (`SMP preempt mod_unload modversions`)
- Config markers (vermagic): `SMP preempt mod_unload modversions`
- Compiler: `gcc (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0` via kbuild defaults
- binutils: `GNU ld 2.42` (Ubuntu)
- Module ABI: `vermagic=7.0.0-38-generic SMP preempt mod_unload modversions`
- Module config: `CONFIG_SMP=y`, `CONFIG_PREEMPT` (preempt), `CONFIG_64BIT=y`,
  `CONFIG_MODVERSIONS=y`, `CONFIG_MODULE_SIG=y`, `CONFIG_RETPOLINE=y`
  (metadata; Slop enforces the vermagic string, not the config set)
- Page size: 4 KiB (`CONFIG_PAGE_SIZE_4KB=y`)
- Module compression: distro ships `.ko.zst` (zstd). The Slop loader has
  no in-kernel decompressor and refuses compressed images explicitly
  (`compressed module`); feed it uncompressed bytes (same bytes as
  `zstd -d`, whose identity the loader does NOT re-verify — record the
  decompression step in the test log).
- Module signing: `CONFIG_MODULE_SIG=y` on the distro; Slop has no keyring
  and does not verify signatures (loads tainted, like `module_sig_check`
  bypassed — documented, never silent: the vermagic gate still applies).
- modversions: present (`__versions`, `__version_ext_names`, `__version_ext_crcs`)
- retpoline: `retpoline=Y` (`.call_sites` + `.return_sites`, `__x86_return_thunk`)
- IBT/CET: `.call_sites`/`.return_sites` metadata present; no `endbr64` in
  `.init.text` for the minimal module (record as observed, not assumed absent)
- SMP/preemption: per vermagic above
- Fixture: minimal `hellonormal.c` (`module_init` prints, empty `module_exit`),
  built with plain kbuild (no `ccflags-y` overrides, no `-mcmodel=large`)

## ELF (fixture `hellonormal.ko`)

- ELF class: ELF64, little-endian, `ET_REL`, machine X86-64, no program headers
- 47 section headers (including 14 `.debug_*`/`.rela.debug_*`)

## ALLOC sections (loader-relevant, file order)

1. `.note.gnu.build-id` / `.note.Linux` (NOTE, ALLOC, no relocs)
2. `__patchable_function_entries` (fentry patch sites, ALLOC)
3. `.text` — EMPTY (0 bytes) for the minimal module; real code lives in
   `.init.text`/`.exit.text` (kbuild `-ffunction-sections`-style split is
   normal; the loader must not assume `.text` is non-empty)
4. `.exit.text` / `.init.text` (16-byte `__pfx_*` NOP prefix + body)
5. `.bss`, `.data` (both 0 bytes here), `.exit.data`, `.init.data`
   (`init_module`/`cleanup_module` pointers, `R_X86_64_64`)
6. `.gnu.linkonce.this_module` (`struct module`, `R_X86_64_64` to init/exit)
7. `.rodata` (format strings), `__mcount_loc` (`R_X86_64_64` to `.init.text+10`)
8. `.modinfo` (`name/license/depends/srcversion/retpoline/vermagic`)
9. `.return_sites` + `.rela.return_sites` (`R_X86_64_PC32` to exit/init text)
10. `.call_sites` + `.rela.call_sites` (`R_X86_64_PC32` to init text fentry sites)
11. `__version_ext_names`, `__version_ext_crcs`, `__versions`
12. No `.altinstructions`, no `.orc_unwind*`, no `.ibt_*`, no `.initcall*`
    in the minimal module (larger drivers will have them; each is an
    explicit-reject-or-support decision, never silently ignored)

## Relocations (ALLOC sections only)

- `R_X86_64_64` — data pointers (init/exit, this_module, mcount)
- `R_X86_64_PC32` — return/call-site metadata
- `R_X86_64_PLT32` — calls (`__fentry__`, `_printk`, `__x86_return_thunk` jmp)
- `R_X86_64_32`: 3223 instances, ALL in non-ALLOC (`.debug_*`) sections —
  the loader already skips non-ALLOC sections and must keep doing so
- `R_X86_64_32S`: exactly 1, in `.rela.debug_*` (non-ALLOC) — no ALLOC use
- Overflow policy (unchanged): signed/unsigned checked per type, refuse to
  load with symbol+reloc logged, never silently truncate

## Undefined symbols (fixture)

- `__fentry__`, `_printk`, `__x86_return_thunk` (all provided by the Slop
  export table today; `module_layout` is version-CRC-ignored by design)

## Disassembly notes (`.init.text`)

- `__pfx_init_module`: 16 NOP prefix (fentry patch area)
- `init_module`: `call __fentry__; ...; call _printk; ...; jmp __x86_return_thunk`
  (returns go through the retpoline thunk even in tiny modules)
- No `endbr64` observed in the minimal fixture

## Compiler instrumentation

- fentry: `__patchable_function_entries` + `__mcount_loc` + `__fentry__` calls
- Stack protector: fixture builds clean either way; assume nothing, record
  only what the loader observes (no `__stack_chk_fail` UND in fixture)

## Required symbols (minimal set)

`_printk`, `__fentry__`, `__x86_return_thunk`, plus loader-internal
`module_layout` handling (CRC ignored, name/params still parsed).

## C2 as-built (high-half alias)

- Kernel image mirrored at `ALIAS_KBASE=0xffffffff80000000` (same frames,
  same permission bits, discovered by walking the live tables at first
  module load; installed once, never unmapped).
- Modules load into the 64 MiB window at `ALIAS_MBASE=0xffffffff90000000`
  (+256 MiB above the kernel alias, mirroring the real layout); all
  section VAs, reloc places, init/exit/probe/handler addresses and kernel
  exports resolve inside the alias. `R_32S` sign-extends correctly there;
  `R_64` is unaffected; module-to-kernel `PC32` spans ~256 MiB.
- No PLT, no per-symbol thunks: everything referenced stays within ±2 GiB
  of its user, so relocs bind directly (the alias itself is the reusable
  mechanism). No dual-execution: modules execute only via the alias
  mapping; the old real-half module window is gone.
- W^X preserved (RX text / RW+NX data, mirrored per page); cache
  attributes identical (same frames); unload unmaps alias pages with
  local invlpg (same SMP pattern as before — cross-CPU shootdown remains
  future work, as for all module mappings).
- Stack traces show alias VAs: subtract `ALIAS_KBASE-KIMAGE_BASE` to get
  the link address from `build/kernel.map`.

## Implication for C2/C3 (original notes, kept)

- Full `R_32/32S` support in ALLOC sections IS required after all (the
  normal-kbuild slotest carries 28 `R_32S` in `.text`/`.init.text`/
  `.exit.text`): the alias makes them validate; anything still
  overflowing is refused with symbol+reloc logged, never truncated.
- `.init.text`/`.exit.text` placement and RX mapping must work when `.text`
  is empty (zero-page section edge case for the page-granular loader).
- Relocs targeting non-allocated (debug) sections are skipped, not
  rejected — unstripped distro `.ko` bytes load as-is.
