# Milestone 26: clean-room static C runtime

The acceptance chain now ends in a C executable built from the in-tree `libc/` runtime instead of hand-written assembly. `/init` execs `/bin/hello` (assembly ABI checks), which then uses Linux `execve` to hand the process to `/bin/hello-c`. The C program reaches `main` with `argc`, `argv` and `envp`, exercises the runtime, prints `c runtime: passed` and returns 0.

The runtime covers freestanding startup (`libc/start.S`), a syscall shim translating the System V C calling convention to the Linux x86-64 syscall ABI (`libc/syscall.S`), first-fit `malloc`/`calloc`/`realloc`/`free` over anonymous mappings, string functions, a `printf` subset, `getenv` over the initial environment, and `openat`/`read`/`write`/`close` wrappers. The build uses Clang and LLD only; no host libc or GCC is involved.

Bootstrap limits are deliberate: only the listed libc subset exists, error reporting uses raw negative return values rather than `errno`, stdout is unbuffered, and `printf` supports `%s %d %u %x %c %p %%` without width or length modifiers. The linker script gained the large-code-model `.ltext`/`.ldata`/`.lbss` input sections so C objects can use the 64 TiB user ELF base.

The next acceptance gate is running an existing statically linked libc program (for example a musl-linked PID 1 and shell) unmodified through the Linux ABI.
