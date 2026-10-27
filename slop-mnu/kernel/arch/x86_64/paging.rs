use crate::memory; pub const PAGE:u64=4096;
pub const PRESENT:u64=1;pub const WRITABLE:u64=1<<1;pub const USER:u64=1<<2;pub const GLOBAL:u64=1<<8;pub const NX:u64=1<<63;pub const ADDR:u64=0x000f_ffff_ffff_f000;
pub trait PhysAccess{unsafe fn table(&self,pa:u64)->&mut[u64;512];}
pub struct PageTableRoot{pub pml4:u64}
impl PageTableRoot{
 pub fn current()->Self{let p:u64;unsafe{core::arch::asm!("mov {}, cr3",out(reg)p,options(nostack,preserves_flags));}Self{pml4:p&ADDR}}
 pub fn query<A:PhysAccess>(&self,a:&A,va:u64)->Option<(u64,u64)>{let idx=[(va>>39)&511,(va>>30)&511,(va>>21)&511,(va>>12)&511];let mut table=self.pml4;let mut effective=u64::MAX;for &i in &idx[..3]{let e=unsafe{a.table(table)}[i as usize];if e&PRESENT==0||e&(1<<7)!=0{return None}effective&=e;table=e&ADDR;}let e=unsafe{a.table(table)}[idx[3]as usize];if e&PRESENT==0{None}else{Some(((e&ADDR)|(va&4095),effective&e))}}
 pub fn translate<A:PhysAccess>(&self,a:&A,va:u64)->Option<u64>{self.query(a,va).map(|x|x.0)}
 pub fn clone_bootstrap<A:PhysAccess>(&self,a:&A,private_slots:&[usize])->Result<Self,i64>{let out=Self::new(a)?;let src=unsafe{a.table(self.pml4)};let dst=unsafe{a.table(out.pml4)};dst.copy_from_slice(src);for &slot in private_slots{if slot>=512{return Err(crate::errno::EINVAL)}dst[slot]=0;}Ok(out)}
 pub fn new<A:PhysAccess>(a:&A)->Result<Self,i64>{let pa=memory::alloc_frame().ok_or(crate::errno::ENOMEM)?;unsafe{a.table(pa).fill(0)}Ok(Self{pml4:pa})}
 pub fn map_page<A:PhysAccess>(&self,a:&A,va:u64,pa:u64,flags:u64)->Result<(),i64>{if va&4095!=0||pa&4095!=0{return Err(crate::errno::EINVAL)}let idx=[(va>>39)&511,(va>>30)&511,(va>>21)&511,(va>>12)&511];let mut table=self.pml4;for &i in &idx[..3]{let t=unsafe{a.table(table)};let e=&mut t[i as usize];if *e&PRESENT==0{let n=memory::alloc_frame().ok_or(crate::errno::ENOMEM)?;unsafe{a.table(n).fill(0)};*e=n|PRESENT|WRITABLE|if flags&USER!=0{USER}else{0};}else if *e&(1<<7)!=0{return Err(crate::errno::EINVAL)}if flags&USER!=0{*e|=USER}table=*e&ADDR;}let leaf=unsafe{a.table(table)};if leaf[idx[3]as usize]&PRESENT!=0{return Err(crate::errno::EEXIST)}leaf[idx[3]as usize]=(pa&ADDR)|PRESENT|flags;unsafe{invlpg(va)}Ok(())}
 pub fn unmap_page<A:PhysAccess>(&self,a:&A,va:u64)->Result<u64,i64>{let e=self.walk_mut(a,va)?;if *e&PRESENT==0{return Err(crate::errno::ENOENT)}let pa=*e&ADDR;*e=0;unsafe{invlpg(va)}Ok(pa)}
 pub fn protect_page<A:PhysAccess>(&self,a:&A,va:u64,flags:u64)->Result<(),i64>{let e=self.walk_mut(a,va)?;if *e&PRESENT==0{return Err(crate::errno::ENOENT)}*e=(*e&ADDR)|PRESENT|flags;unsafe{invlpg(va)}Ok(())}
 fn walk_mut<'a,A:PhysAccess>(&self,a:&'a A,va:u64)->Result<&'a mut u64,i64>{let idx=[(va>>39)&511,(va>>30)&511,(va>>21)&511,(va>>12)&511];let mut table=self.pml4;for &i in &idx[..3]{let e=unsafe{a.table(table)}[i as usize];if e&PRESENT==0{return Err(crate::errno::ENOENT)}table=e&ADDR;}Ok(&mut unsafe{a.table(table)}[idx[3]as usize])}
 pub fn destroy_private_slot<A:PhysAccess>(&self,a:&A,slot:usize)->Result<(),i64>{if slot>=256{return Err(crate::errno::EINVAL)}
 let pml4=unsafe{a.table(self.pml4)};let top=pml4[slot];
 if top&PRESENT==0{pml4[slot]=0;return Ok(())}
 if top&(1<<7)!=0{return Err(crate::errno::EINVAL)}let pdpt_pa=top&ADDR;let pdpt=unsafe{a.table(pdpt_pa)};for i in 0..512{let e3=pdpt[i];if e3&PRESENT==0{continue}if e3&(1<<7)!=0{return Err(crate::errno::EINVAL)}let pd_pa=e3&ADDR;let pd=unsafe{a.table(pd_pa)};for j in 0..512{let e2=pd[j];if e2&PRESENT==0{continue}if e2&(1<<7)!=0{return Err(crate::errno::EINVAL)}let pt_pa=e2&ADDR;let pt=unsafe{a.table(pt_pa)}; for k in 0..512{let e1=pt[k];if e1&PRESENT!=0{crate::memory::release_frame(e1&ADDR)?;pt[k]=0;let va=((slot as u64)<<39)|((i as u64)<<30)|((j as u64)<<21)|((k as u64)<<12);unsafe{invlpg(va);}}}if let Err(e)=crate::memory::free_frame(pt_pa){return Err(e)}pd[j]=0;}
    if let Err(e)=crate::memory::free_frame(pd_pa){return Err(e)}pdpt[i]=0;}
   if let Err(e)=crate::memory::free_frame(pdpt_pa){return Err(e)}pml4[slot]=0;Ok(())}
 pub unsafe fn activate(&self){unsafe{core::arch::asm!("mov cr3, {}",in(reg)self.pml4,options(nostack,preserves_flags));}}
 /* Installs the kernel direct map: `slots` PML4 entries of 512 GiB each at
    PHYS_BASE, backed by 1 GiB pages covering the whole physical address
    space (RAM and MMIO alike). Runs before the kernel moves to the high
    half, so it must be given an identity accessor. */
 pub fn install_direct_map<A:PhysAccess>(&self,a:&A,slots:usize)->Result<(),&'static str>{
  let base_slot=((crate::arch::x86_64::phys::PHYS_BASE>>39)&511)as usize;
  for s in 0..slots{
   if base_slot+s>=512{return Err("direct map exceeds PML4")}
   let pdpt=memory::alloc_frame().ok_or("direct map allocation failed")?;
   let t=unsafe{a.table(pdpt)};
   for i in 0..512{t[i]=(((s*512+i)as u64)<<30)|PRESENT|WRITABLE|(1<<7);}
   let p=unsafe{a.table(self.pml4)};
   p[base_slot+s]=pdpt|PRESENT|WRITABLE;
  }
  Ok(())
 }
 /* Zeroes every PML4 entry except `keep`: used once after the move to the
    high half to drop all firmware mappings in one step. */
 pub unsafe fn keep_slots<A:PhysAccess>(&self,a:&A,keep:&[usize]){
  let t=unsafe{a.table(self.pml4)};
  for i in 0..512{if !keep.contains(&i){t[i]=0}}
  unsafe{core::arch::asm!("mov cr3, {}",in(reg)self.pml4,options(nostack,preserves_flags));}
 }
}
#[inline]pub unsafe fn invlpg(va:u64){unsafe{core::arch::asm!("invlpg [{}]",in(reg)va,options(nostack,preserves_flags));}}
/* One-way move to the high half. `rsp` and `entry` are direct-map addresses;
   the firmware page table stays active but already contains the direct map. */
#[unsafe(naked)]pub unsafe extern "sysv64" fn enter_high(rsp:u64,entry:u64)->!{core::arch::naked_asm!("mov rsp, rdi","jmp rsi")}
