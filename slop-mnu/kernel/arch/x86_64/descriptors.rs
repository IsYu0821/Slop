#[repr(C,packed)]struct Ptr{limit:u16,base:u64}
#[repr(C,packed)]#[derive(Clone,Copy)]pub struct Tss{r0:u32,pub rsp:[u64;3],r1:u64,pub ist:[u64;7],r2:u64,r3:u16,pub iomap:u16}
impl Tss{pub const fn new()->Self{Self{r0:0,rsp:[0;3],r1:0,ist:[0;7],r2:0,r3:0,iomap:core::mem::size_of::<Self>()as u16}}}
pub const KCODE:u16=0x08;pub const KDATA:u16=0x10;pub const UDATA:u16=0x1b;pub const UCODE:u16=0x23;pub const TSS:u16=0x28;
static mut GDT:[u64;7]=[0;7];pub static mut BOOT_TSS:Tss=Tss::new();
/* Fills a 7-slot GDT with the same layout the bootstrap CPU uses, pointing
   its TSS descriptor at the given task-state segment. Shared by the BSP
   and by every application processor so all CPUs run the same selectors. */
pub unsafe fn fill_gdt(gdt:*mut [u64;7],tss_addr:u64){unsafe{let g=&mut *gdt;g[1]=0x00af9a000000ffff;g[2]=0x00cf92000000ffff;g[3]=0x00cff2000000ffff;g[4]=0x00affa000000ffff;let l=(core::mem::size_of::<Tss>()-1)as u64;g[5]=(l&0xffff)|((tss_addr&0xffffff)<<16)|(0x89<<40)|((l&0xf0000)<<32)|((tss_addr&0xff000000)<<32);g[6]=tss_addr>>32;}}
/* Loads a GDT built by fill_gdt and refreshes the data segments. The
   caller must already run with CS == KCODE (the trampoline far jump
   establishes it), so no far return is needed here. */
pub unsafe fn load_gdt(gdt_addr:u64){unsafe{let p=Ptr{limit:(core::mem::size_of::<[u64;7]>()-1)as u16,base:gdt_addr};core::arch::asm!("lgdt [{}]",in(reg)&p,options(readonly,nostack));core::arch::asm!("mov ds, ax","mov es, ax","mov ss, ax",in("ax")KDATA,options(nostack));}}
pub unsafe fn load_tr(){unsafe{core::arch::asm!("ltr ax",in("ax")TSS,options(nostack))}}
pub unsafe fn init(rsp0:u64,ist1:u64,ist2:u64){unsafe{BOOT_TSS.rsp[0]=rsp0;BOOT_TSS.ist[0]=ist1;BOOT_TSS.ist[1]=ist2;fill_gdt(core::ptr::addr_of_mut!(GDT),core::ptr::addr_of!(BOOT_TSS) as u64);let p=Ptr{limit:(core::mem::size_of::<[u64;7]>()-1)as u16,base:core::ptr::addr_of!(GDT)as u64};core::arch::asm!("lgdt [{}]",in(reg)&p,options(readonly,nostack));
/* Firmware selectors must not survive the GDT replacement: reload CS through a far return and refresh the data segments, otherwise the first interrupt return reloads a stale selector and #GPs. */
core::arch::asm!("push {cs}","lea {tmp},[rip + 2f]","push {tmp}","retfq","2:","mov ds, ax","mov es, ax","mov ss, ax",cs=in(reg)KCODE as u64,tmp=out(reg)_,in("ax")KDATA);
core::arch::asm!("ltr ax",in("ax")TSS,options(nostack));}}

pub unsafe fn set_rsp0(value:u64){unsafe{BOOT_TSS.rsp[0]=value}}
