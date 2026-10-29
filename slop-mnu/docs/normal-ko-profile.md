# Normal-kbuild .ko profile (Track C1 target)

Fixed compatibility profile for removing the Slop-specific rebuild.
Do NOT design against assumptions; this file is the mechanically
enumerated ground truth for the pinned toolchain.

## Pinned environment

- Target Linux series: Ubuntu `7.0.0-38-generic` (`SMP preempt mod_unload modversions`)
- Config markers (vermagic): `SMP preempt mod_unload modversions`
- Compiler: `gcc (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0` via kbuild defaults
- Module ABI: `vermagic=7.0.0-38-generic SMP preempt mod_unload modversions`
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

## Implication for C2/C3

- The high-alias design must serve `R_64` (absolute data) and small `PC32`
  ranges from module text to nearby sections — full `R_32/32S` support in
  ALLOC sections is NOT required by this profile (none observed), but the
  loader must explicitly reject them, not truncate.
- `.init.text`/`.exit.text` placement and RX mapping must work when `.text`
  is empty (zero-page section edge case for the page-granular loader).
