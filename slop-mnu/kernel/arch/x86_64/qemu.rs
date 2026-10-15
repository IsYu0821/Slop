//! QEMU isa-debug-exit support for deterministic acceptance tests.
const PORT:u16=0xf4;
/// With isa-debug-exit, QEMU returns host status `(value << 1) | 1`.
pub fn exit(value:u8)->!{
    crate::console::flush();
    unsafe{core::arch::asm!("out dx, eax",in("dx")PORT,in("eax")value as u32,options(nomem,nostack,preserves_flags));}
    crate::arch::halt()
}
