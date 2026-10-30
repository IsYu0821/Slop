//! Physical access through the kernel's high-half direct map.
//!
//! After `kmain_high_continue` the kernel runs in the high half and owns no
//! low mappings, so every physical address is reached through the direct map
//! installed by `paging::install_direct_map` at `PHYS_BASE`.
use super::paging::PhysAccess;
/// Base of the direct physical map: PML4 slot 256 (canonical high half).
pub const PHYS_BASE:u64=0xffff_8000_0000_0000;
/// Where the kernel relocates its image at boot: PML4 slot 288, outside the
/// direct map so 4 KiB pages can back it.
pub const KIMAGE_BASE:u64=0xffff_9000_0000_0000;
#[inline] pub const fn phys_to_virt(pa:u64)->u64{PHYS_BASE+pa}
/// Translates a kernel virtual address through the active page tables. Covers
/// both the direct map and the relocated kernel image; returns 0 if unmapped.
pub fn virt_to_phys(va:u64)->u64{
    let root=super::paging::PageTableRoot::current();
    root.translate(&Identity,va).map(|e|(e&super::paging::ADDR)|(va&0xfff)).unwrap_or(0)
}
/// Full VA→PA walk including 1 GiB / 2 MiB huge pages (the direct map uses
/// 1 GiB pages, which `translate` rejects). Used by the Slop DMA core, which
/// must resolve direct-mapped buffers without direct-map arithmetic.
pub fn virt_to_phys_full(va:u64)->u64{
    use super::paging::{PageTableRoot, ADDR, PRESENT};
    let root=PageTableRoot::current();
    let acc=Identity;
    let t0=unsafe{acc.table(root.pml4)};
    let e0=t0[((va>>39)&511)as usize];
    if e0&PRESENT==0{return 0}
    if e0&(1<<7)!=0{return (e0&ADDR)|(va&0x3fff_ffff)}
    let t1=unsafe{acc.table(e0&ADDR)};
    let e1=t1[((va>>30)&511)as usize];
    if e1&PRESENT==0{return 0}
    if e1&(1<<7)!=0{return (e1&ADDR)|(va&0x3fff_ffff)}
    let t2=unsafe{acc.table(e1&ADDR)};
    let e2=t2[((va>>21)&511)as usize];
    if e2&PRESENT==0{return 0}
    if e2&(1<<7)!=0{return (e2&ADDR)|(va&0x1f_ffff)}
    let t3=unsafe{acc.table(e2&ADDR)};
    let e3=t3[((va>>12)&511)as usize];
    if e3&PRESENT==0{return 0}
    (e3&ADDR)|(va&0xfff)
}
pub struct Identity;
impl PhysAccess for Identity {
    unsafe fn table(&self, pa:u64)->&mut[u64;512] {
        unsafe { &mut *(phys_to_virt(pa) as *mut [u64;512]) }
    }
}
impl Identity {
    pub unsafe fn bytes(&self,pa:u64,len:usize)->&mut[u8]{unsafe{core::slice::from_raw_parts_mut(phys_to_virt(pa) as *mut u8,len)}}
}
/// Identity accessor used only while building the direct map, before the
/// kernel can address the high half.
pub struct EarlyPhys;
impl PhysAccess for EarlyPhys {
    unsafe fn table(&self, pa:u64)->&mut[u64;512] {
        unsafe { &mut *(pa as *mut [u64;512]) }
    }
}
impl EarlyPhys {
    pub unsafe fn bytes(&self,pa:u64,len:usize)->&mut[u8]{unsafe{core::slice::from_raw_parts_mut(pa as *mut u8,len)}}
}
