pub const IA32_FS_BASE:u32=0xc000_0100; pub const IA32_GS_BASE:u32=0xc000_0101; pub const IA32_KERNEL_GS_BASE:u32=0xc000_0102;
pub const IA32_EFER:u32=0xc000_0080; pub const IA32_STAR:u32=0xc000_0081; pub const IA32_LSTAR:u32=0xc000_0082; pub const IA32_FMASK:u32=0xc000_0084;
#[inline] pub unsafe fn rdmsr(msr:u32)->u64{let lo:u32;let hi:u32;unsafe{core::arch::asm!("rdmsr",in("ecx")msr,out("eax")lo,out("edx")hi,options(nostack,nomem));}((hi as u64)<<32)|lo as u64}
#[inline] pub unsafe fn wrmsr(msr:u32,v:u64){unsafe{core::arch::asm!("wrmsr",in("ecx")msr,in("eax")v as u32,in("edx")(v>>32)as u32,options(nostack,nomem));}}
