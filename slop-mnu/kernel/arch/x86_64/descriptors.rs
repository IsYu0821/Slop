#[repr(C,packed)]struct Ptr{limit:u16,base:u64}
#[repr(C,packed)]#[derive(Clone,Copy)]pub struct Tss{r0:u32,pub rsp:[u64;3],r1:u64,pub ist:[u64;7],r2:u64,r3:u16,pub iomap:u16}
impl Tss{pub const fn new()->Self{Self{r0:0,rsp:[0;3],r1:0,ist:[0;7],r2:0,r3:0,iomap:core::mem::size_of::<Self>()as u16}}}
pub const KCODE:u16=0x08;pub const KDATA:u16=0x10;pub const UDATA:u16=0x1b;pub const UCODE:u16=0x23;pub const TSS:u16=0x28;
static mut GDT:[u64;7]=[0;7];pub static mut BOOT_TSS:Tss=Tss::new();
pub unsafe fn init(rsp0:u64,ist1:u64,ist2:u64){unsafe{BOOT_TSS.rsp[0]=rsp0;BOOT_TSS.ist[0]=ist1;BOOT_TSS.ist[1]=ist2;GDT[1]=0x00af9a000000ffff;GDT[2]=0x00cf92000000ffff;GDT[3]=0x00cff2000000ffff;GDT[4]=0x00affa000000ffff;let b=core::ptr::addr_of!(BOOT_TSS)as u64;let l=(core::mem::size_of::<Tss>()-1)as u64;GDT[5]=(l&0xffff)|((b&0xffffff)<<16)|(0x89<<40)|((l&0xf0000)<<32)|((b&0xff000000)<<32);GDT[6]=b>>32;let p=Ptr{limit:(core::mem::size_of::<[u64;7]>()-1)as u16,base:core::ptr::addr_of!(GDT)as u64};core::arch::asm!("lgdt [{}]",in(reg)&p,options(readonly,nostack));core::arch::asm!("ltr ax",in("ax")TSS,options(nostack));}}

pub unsafe fn set_rsp0(value:u64){unsafe{BOOT_TSS.rsp[0]=value}}
