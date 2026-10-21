//! Port I/O primitives for device drivers built on the x86-64 seam.
#[inline]pub unsafe fn inb(port:u16)->u8{let v:u8;unsafe{core::arch::asm!("in al, dx",in("dx")port,out("al")v,options(nomem,nostack,preserves_flags));}v}
#[inline]pub unsafe fn inw(port:u16)->u16{let v:u16;unsafe{core::arch::asm!("in ax, dx",in("dx")port,out("ax")v,options(nomem,nostack,preserves_flags));}v}
#[inline]pub unsafe fn inl(port:u16)->u32{let v:u32;unsafe{core::arch::asm!("in eax, dx",in("dx")port,out("eax")v,options(nomem,nostack,preserves_flags));}v}
#[inline]pub unsafe fn outb(port:u16,v:u8){unsafe{core::arch::asm!("out dx, al",in("dx")port,in("al")v,options(nomem,nostack,preserves_flags));}}
#[inline]pub unsafe fn outw(port:u16,v:u16){unsafe{core::arch::asm!("out dx, ax",in("dx")port,in("ax")v,options(nomem,nostack,preserves_flags));}}
#[inline]pub unsafe fn outl(port:u16,v:u32){unsafe{core::arch::asm!("out dx, eax",in("dx")port,in("eax")v,options(nomem,nostack,preserves_flags));}}
