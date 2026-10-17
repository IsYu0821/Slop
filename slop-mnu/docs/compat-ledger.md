# Compatibility provenance ledger

Every externally observed constant, structure layout and behavior receives an entry with: interface, architecture/version, public specification or black-box test source, implementer, reviewer and tests. No implementation contributor may consult Linux implementation source for the same component during a clean-room reimplementation.

Initial ABI facts used in this scaffold:
- ELF64 little-endian x86-64 machine ID 62.
- Linux x86-64 syscall numbers: write=1, getpid=39, exit=60, uname=63, exit_group=231.
- Linux kernel errors are represented as negative errno at the raw syscall boundary.

C runtime facts used by the in-tree `libc/`:
- ISO C function names and signatures (`memcpy`, `memmove`, `memset`, `memcmp`, `strlen`, `strcmp`, `strncmp`, `strchr`, `strcpy`, `malloc`, `calloc`, `realloc`, `free`, `getenv`, and the `printf` subset): public specification ISO/IEC 9899; implemented clean-room by sloppy-coder0821 from function semantics only. Reviewer: pending. Tests: `/bin/hello-c` checks in `tools/acceptance-test.sh`.
- Linux x86-64 syscall numbers used by the wrappers (`read`=0, `write`=1, `close`=3, `mmap`=9, `openat`=257, `exit_group`=231): public Linux man-pages. Implementer: sloppy-coder0821. Reviewer: pending. Tests: C runtime acceptance chain.
- Wrapper constants `AT_FDCWD`=-100, `O_RDONLY`=0, `PROT_READ`=1, `PROT_WRITE`=2, `MAP_PRIVATE`=0x02, `MAP_ANONYMOUS`=0x20: public Linux man-pages. Implementer: sloppy-coder0821. Reviewer: pending. Tests: `openat`/`mmap` checks in `/bin/hello-c`.

Before release, legal counsel must approve the clean-room protocol, licenses, trademarks and distribution process.
