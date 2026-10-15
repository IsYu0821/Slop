# Compatibility provenance ledger

Every externally observed constant, structure layout and behavior receives an entry with: interface, architecture/version, public specification or black-box test source, implementer, reviewer and tests. No implementation contributor may consult Linux implementation source for the same component during a clean-room reimplementation.

Initial ABI facts used in this scaffold:
- ELF64 little-endian x86-64 machine ID 62.
- Linux x86-64 syscall numbers: write=1, getpid=39, exit=60, uname=63, exit_group=231.
- Linux kernel errors are represented as negative errno at the raw syscall boundary.

Before release, legal counsel must approve the clean-room protocol, licenses, trademarks and distribution process.
