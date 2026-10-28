//! PCI configuration access and device discovery.
//!
//! The CAM pair at 0xcf8/0xcfc is used for configuration space; QEMU's q35
//! host bridge answers it for every bus. Discovery walks bus 0..=255, checking
//! function 0 first and only scanning further functions when the multifunction
//! bit in the header type is set.
use crate::arch::x86_64::port;

#[derive(Clone,Copy)] pub struct Bdf{pub segment:u16,pub bus:u8,pub device:u8,pub function:u8}
pub trait ConfigAccess:Sync{fn read32(&self,bdf:Bdf,offset:u16)->u32;fn write32(&self,bdf:Bdf,offset:u16,value:u32);}
#[derive(Clone,Copy)] pub struct Bar{pub base:u64,pub size:u64,pub io:bool,pub prefetchable:bool}
// Linux driver compatibility belongs in generated adapters above this native API, never in core structs.

pub const CFG_VENDOR:u16=0x00;pub const CFG_COMMAND:u16=0x04;pub const CFG_CLASS:u16=0x08;
pub const CFG_HEADER:u16=0x0c;pub const CFG_BAR0:u16=0x10;pub const CFG_CAP:u16=0x34;
pub const CMD_IO:u16=1;pub const CMD_MEM:u16=2;pub const CMD_BUS_MASTER:u16=4;

/* Configuration space via the CAM I/O ports (works on QEMU's q35 bridge). */
pub struct Cam;
fn cam_address(bdf:Bdf,offset:u16)->u32{(1<<31)|((bdf.bus as u32)<<16)|((bdf.device as u32)<<11)|((bdf.function as u32)<<8)|((offset as u32)&0xfc)}
impl ConfigAccess for Cam{
 fn read32(&self,bdf:Bdf,offset:u16)->u32{unsafe{port::outl(0xcf8,cam_address(bdf,offset));port::inl(0xcfc)}}
 fn write32(&self,bdf:Bdf,offset:u16,value:u32){unsafe{port::outl(0xcf8,cam_address(bdf,offset));port::outl(0xcfc,value)}}
}

pub struct Found{pub bdf:Bdf,pub vendor:u16,pub device:u16,pub class:u8,pub bars:[Bar;6]}
impl Found{
 pub fn enable(&self,acc:&dyn ConfigAccess){
  let base=acc.read32(self.bdf,CFG_COMMAND)as u16;
  acc.write32(self.bdf,CFG_COMMAND,(base|CMD_IO|CMD_MEM|CMD_BUS_MASTER)as u32);
 }
}
fn read_bar(acc:&dyn ConfigAccess,bdf:Bdf,index:u8)->Bar{
 let lo=acc.read32(bdf,CFG_BAR0+4*(index as u16));
 if lo&1!=0{return Bar{base:(lo&0xffff_fffc)as u64,size:0,io:true,prefetchable:false}}
 let kind=(lo>>1)&3;
 if kind==2{
  let hi=acc.read32(bdf,CFG_BAR0+4*(index as u16)+4);
  Bar{base:((hi as u64)<<32)|((lo&0xffff_fff0)as u64),size:0,io:false,prefetchable:lo&8!=0}
 }else{
  Bar{base:(lo&0xffff_fff0)as u64,size:0,io:false,prefetchable:lo&8!=0}
 }
}
fn read_bars(acc:&dyn ConfigAccess,bdf:Bdf)->[Bar;6]{
 let mut bars=[Bar{base:0,size:0,io:false,prefetchable:false};6];
 let mut i=0usize;
 while i<6{
  let bar=read_bar(acc,bdf,i as u8);
  let wide=!bar.io&&(acc.read32(bdf,CFG_BAR0+4*(i as u16))>>1)&3==2;
  bars[i]=bar;
  i+=if wide{2}else{1};
 }
 bars
}
fn probe(acc:&dyn ConfigAccess,bdf:Bdf)->Option<Found>{
 let word=acc.read32(bdf,CFG_VENDOR);
 if word&0xffff==0xffff{return None}
 let vendor=(word&0xffff)as u16;let device=(word>>16)as u16;
 let class=((acc.read32(bdf,CFG_CLASS)>>24)&0xff)as u8;
 Some(Found{bdf,vendor,device,class,bars:read_bars(acc,bdf)})
}
pub fn find(vendor:u16,device:u16)->Option<Found>{
 let acc=Cam;
 for bus in 0..=255u8{
  for dev in 0..32u8{
   let bdf=Bdf{segment:0,bus,device:dev,function:0};
   let word=acc.read32(bdf,CFG_VENDOR);
   if word&0xffff==0xffff{continue}
   let multifunction=(acc.read32(bdf,CFG_HEADER)>>16)&0x80!=0;
   let last=if multifunction{8u8}else{1u8};
   for func in 0..last{
    let bdf=Bdf{segment:0,bus,device:dev,function:func};
    if let Some(found)=probe(&acc,bdf){
     if found.vendor==vendor&&found.device==device{return Some(found)}
    }
   }
  }
 }
 None
}
/* Human-readable count for the boot log. */
pub fn count()->usize{
 let acc=Cam;let mut n=0;
 for bus in 0..=255u8{
  for dev in 0..32u8{
   let bdf=Bdf{segment:0,bus,device:dev,function:0};
   if acc.read32(bdf,CFG_VENDOR)&0xffff!=0xffff{n+=1}
  }
 }
 n
}

/* ---- Slop PCI driver registry (core, OS-neutral) ----
   Slop-native drivers AND the Linux compat adapter register here.
   Matching is by explicit vendor/device (0xFFFF = wildcard, mirroring
   PCI_ANY_ID). The core owns enumeration, matching, and probe dispatch;
   adapters translate their own driver models into these plain IDs plus an
   opaque cookie handed back to the probe callback. */
pub type SlopProbeFn = fn(bdf: Bdf, vendor: u16, device: u16, opaque: u64) -> i32;
pub struct SlopPciDriver {
    pub used: bool,
    pub name: [u8; 48],
    pub vendor: u16,
    pub device: u16,
    pub probe: Option<SlopProbeFn>,
    pub opaque: u64,
}
const NO_SLOP_DRV: SlopPciDriver = SlopPciDriver { used: false, name: [0; 48], vendor: 0, device: 0, probe: None, opaque: 0 };
static SLOP_REGS: crate::sync::IrqSpinLock<[SlopPciDriver; 16]> = crate::sync::IrqSpinLock::new([NO_SLOP_DRV; 16]);

fn slop_name(src: &[u8; 48]) -> [u8; 48] { *src }

/// Register a Slop-native PCI driver. Returns 0 on success, -ENOSPC if full.
pub fn register_slop_driver(name: &[u8; 48], vendor: u16, device: u16, probe: SlopProbeFn, opaque: u64) -> i32 {
    {
        let mut regs = SLOP_REGS.lock();
        match regs.iter_mut().find(|r| !r.used) {
            Some(r) => { r.used = true; r.name = slop_name(name); r.vendor = vendor; r.device = device; r.probe = Some(probe); r.opaque = opaque; }
            None => return -28, // -ENOSPC
        }
    }
    // Probe already-enumerated devices immediately (no hotplug bus rescan).
    let mut n = 0;
    let acc = Cam;
    for bus in 0..=255u8 {
        for dev in 0..32u8 {
            let b0 = Bdf { segment: 0, bus, device: dev, function: 0 };
            if acc.read32(b0, CFG_VENDOR) & 0xffff == 0xffff { continue; }
            let multifunction = (acc.read32(b0, CFG_HEADER) >> 16) & 0x80 != 0;
            let last = if multifunction { 8u8 } else { 1u8 };
            for func in 0..last {
                let bdf = Bdf { segment: 0, bus, device: dev, function: func };
                let w = acc.read32(bdf, CFG_VENDOR);
                if w & 0xffff == 0xffff { continue; }
                let v = (w & 0xffff) as u16;
                let d = ((w >> 16) & 0xffff) as u16;
                let vm = vendor == 0xffff || vendor == v;
                let dm = device == 0xffff || device == d;
                if vm && dm {
                    // Re-lookup the just-registered entry to call its probe.
                    let (pf, oq) = {
                        let regs = SLOP_REGS.lock();
                        match regs.iter().find(|r| r.used && r.vendor == vendor && r.device == device && r.opaque == opaque) {
                            Some(r) => (r.probe, r.opaque),
                            None => (None, 0),
                        }
                    };
                    if let Some(f) = pf {
                        if f(bdf, v, d, oq) == 0 { n += 1; }
                    }
                }
            }
        }
    }
    crate::println!("pci: slop driver registered ({} devices matched)", n);
    0
}

/// Unregister by opaque cookie (adapter passes its driver pointer).
pub fn unregister_slop_driver(opaque: u64) {
    let mut regs = SLOP_REGS.lock();
    for r in regs.iter_mut() {
        if r.used && r.opaque == opaque { *r = NO_SLOP_DRV; }
    }
}

/* ---- BAR sizing (standard write-all-ones protocol) ---- */
// Returns the size in bytes, or 0 for unimplemented BARs. Restores the
// original address. Handles 32-bit MEM, 64-bit MEM (pair), and I/O BARs.
pub fn bar_size(acc: &dyn ConfigAccess, bdf: Bdf, index: u8) -> u64 {
    if index >= 6 { return 0; }
    let off = CFG_BAR0 + 4 * (index as u16);
    let orig_lo = acc.read32(bdf, off);
    if orig_lo == 0 { return 0; }
    if orig_lo & 1 != 0 {
        // I/O BAR: low 2 bits are flags.
        acc.write32(bdf, off, 0xffff_ffff);
        let v = acc.read32(bdf, off);
        acc.write32(bdf, off, orig_lo);
        let mask = v & 0xffff_fffc;
        if mask == 0 { return 0; }
        return (!mask as u64 + 1) & 0xffff_ffff;
    }
    let kind = (orig_lo >> 1) & 3;
    if kind == 2 {
        // 64-bit MEM: pair of BARs.
        if index >= 5 { return 0; }
        let orig_hi = acc.read32(bdf, off + 4);
        acc.write32(bdf, off, 0xffff_ffff);
        acc.write32(bdf, off + 4, 0xffff_ffff);
        let lo = acc.read32(bdf, off);
        let hi = acc.read32(bdf, off + 4);
        acc.write32(bdf, off, orig_lo);
        acc.write32(bdf, off + 4, orig_hi);
        let mask_lo = (lo & 0xffff_fff0) as u64;
        let mask_hi = hi as u64;
        if mask_lo == 0 && mask_hi == 0 { return 0; }
        let combined = (mask_hi << 32) | mask_lo;
        (!combined).wrapping_add(1)
    } else {
        acc.write32(bdf, off, 0xffff_ffff);
        let v = acc.read32(bdf, off);
        acc.write32(bdf, off, orig_lo);
        let mask = (v & 0xffff_fff0) as u64;
        if mask == 0 { return 0; }
        (!mask).wrapping_add(1) & 0xffff_ffff
    }
}

/* ---- BAR resource reservation (Slop core owns the ledger) ---- */
#[derive(Clone, Copy)]
struct BarRes { used: bool, base: u64, size: u64, owner: [u8; 32] }
const NO_RES: BarRes = BarRes { used: false, base: 0, size: 0, owner: [0; 32] };
static BAR_RESV: crate::sync::IrqSpinLock<[BarRes; 32]> = crate::sync::IrqSpinLock::new([NO_RES; 32]);

fn ranges_overlap(a: u64, alen: u64, b: u64, blen: u64) -> bool {
    a < b.wrapping_add(blen) && b < a.wrapping_add(alen)
}

/// Reserve [base, base+size) for owner. Fails if overlapping.
pub fn reserve_bar(base: u64, size: u64, owner: &[u8]) -> Result<(), i64> {
    if size == 0 { return Err(crate::errno::EINVAL); }
    let mut t = BAR_RESV.lock();
    for r in t.iter() {
        if r.used && ranges_overlap(base, size, r.base, r.size) {
            return Err(crate::errno::EBUSY);
        }
    }
    match t.iter_mut().find(|r| !r.used) {
        Some(r) => {
            r.used = true;
            r.base = base;
            r.size = size;
            let n = core::cmp::min(owner.len(), 32);
            r.owner[..n].copy_from_slice(&owner[..n]);
            Ok(())
        }
        None => Err(crate::errno::ENOMEM),
    }
}
pub fn release_bar(base: u64, size: u64) {
    let mut t = BAR_RESV.lock();
    for r in t.iter_mut() {
        if r.used && r.base == base && r.size == size {
            *r = NO_RES;
            return;
        }
    }
}

/// Size every BAR of a device and fill sizes in place. Returns count sized.
pub fn size_bars(found: &mut Found) {
    let acc = Cam;
    let mut i = 0usize;
    while i < 6 {
        let off = CFG_BAR0 + 4 * (i as u16);
        let lo = acc.read32(found.bdf, off);
        if lo == 0 {
            i += 1;
            continue;
        }
        let wide = lo & 1 == 0 && ((lo >> 1) & 3) == 2;
        found.bars[i].size = bar_size(&acc, found.bdf, i as u8);
        // Zero-size means unimplemented; leave base as-is for diagnostics.
        i += if wide { 2 } else { 1 };
    }
}

/* Boot self-test: size BARs of every device, verify restore, no crash.
   QEMU q35 exposes real BARs (host bridge, virtio); sizing must not wedge
   config space and must leave addresses intact. */
pub fn selftest_bars() {
    let acc = Cam;
    let mut done = 0;
    for bus in 0..=255u8 {
        for dev in 0..32u8 {
            let bdf = Bdf { segment: 0, bus, device: dev, function: 0 };
            if acc.read32(bdf, CFG_VENDOR) & 0xffff == 0xffff { continue; }
            let mut f = match probe(&acc, bdf) {
                Some(x) => x,
                None => continue,
            };
            let bases_before = [f.bars[0].base, f.bars[1].base, f.bars[2].base, f.bars[3].base, f.bars[4].base, f.bars[5].base];
            size_bars(&mut f);
            // Verify restore: re-read bases, must match.
            let g = match probe(&acc, bdf) {
                Some(x) => x,
                None => {
                    crate::println!("pci: selftest FAIL reread {:02x}:{:02x}.{}", bus, dev, 0);
                    return;
                }
            };
            for k in 0..6 {
                if g.bars[k].base != [bases_before[0], bases_before[1], bases_before[2], bases_before[3], bases_before[4], bases_before[5]][k] {
                    crate::println!("pci: selftest FAIL restore {:02x}:{:02x}.{}", bus, dev, 0);
                    return;
                }
            }
            if f.bars.iter().any(|b| b.size != 0) {
                crate::println!("pci: {:02x}:{:02x}.{} bar0={:#x}/{:#x}", bus, dev, 0, f.bars[0].base, f.bars[0].size);
            }
            done += 1;
            if done >= 8 { break; }
        }
        if done >= 8 { break; }
    }
    crate::println!("pci: selftest PASS ({} devices sized, restore ok)", done);
}
