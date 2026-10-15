# Milestone 7: ring 3 and system-call boundary

The kernel now has source-level CPU paths for entering a 64-bit user context and receiving Linux x86-64 `syscall` instructions.

`syscall::init` enables EFER.SCE, programs STAR from the installed GDT selectors, points LSTAR at the assembly entry, and masks IF and DF through FMASK. The assembly entry captures the user stack, selects the dedicated kernel stack, preserves nonvolatile registers and the hardware return state, translates Linux syscall registers to the Rust/System V function ABI, and invokes `linux_abi::dispatch`.

Before `sysretq`, both user RIP and RSP are restricted to the canonical lower half. A rejected return switches back to the kernel stack and stops with diagnostics. A future signal path can replace this fatal fallback with an `iretq` return.

`user::enter` validates initial RIP/RSP and uses an IRET frame containing user SS, RSP, RFLAGS, CS and RIP. It is not called until real user PTEs and the ELF image are installed.

The acceptance program in `tests/minimal-linux.S` is a static, libc-free Linux x86-64 ELF. Its only required syscalls are `write(1, ...)` and `exit_group(0)`.

## Known single-core limitation

The current syscall stack pointers are global statics. They are valid only for the first single-core, single-thread acceptance test. SMP requires per-CPU storage, normally addressed through a kernel GS base and `swapgs` on every user/kernel transition.
