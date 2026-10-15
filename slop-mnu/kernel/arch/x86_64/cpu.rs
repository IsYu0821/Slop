//! Bootstrap CPU capability validation and protection-bit setup.
#[derive(Clone,Copy)]pub struct Features{pub syscall:bool,pub nx:bool,pub page_1g:bool,pub smep:bool,pub smap:bool}
#[inline]fn cpuid(leaf:u32,subleaf:u32)->(u32,u32,u32,u32){let mut eax=leaf;let mut ecx=subleaf;let ebx:u32;let edx:u32;unsafe{core::arch::asm!("push rbx","cpuid","mov {ebx:e}, ebx","pop rbx",ebx=lateout(reg)ebx,inout("eax")eax,inout("ecx")ecx,lateout("edx")edx,options(preserves_flags));}(eax,ebx,ecx,edx)}
pub fn detect()->Features{let (_,_,_,edx_ext)=cpuid(0x8000_0001,0);let max=cpuid(0,0).0;let (_,ebx7,_,_)=if max>=7{cpuid(7,0)}else{(0,0,0,0)};Features{syscall:edx_ext&(1<<11)!=0,nx:edx_ext&(1<<20)!=0,page_1g:edx_ext&(1<<26)!=0,smep:ebx7&(1<<7)!=0,smap:ebx7&(1<<20)!=0}}
pub unsafe fn init()->Result<Features,&'static str>{let f=detect();if !f.syscall{return Err("CPU lacks SYSCALL/SYSRET")}if !f.nx{return Err("CPU lacks NX")}
 unsafe{let mut cr0:u64;core::arch::asm!("mov {}, cr0",out(reg)cr0,options(nostack,preserves_flags));cr0|=1<<16;core::arch::asm!("mov cr0, {}",in(reg)cr0,options(nostack,preserves_flags));}
 Ok(f)}
