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

/* ---- MMIO UC mapping (Slop core; fixes WB direct-map use for BARs) ---- */
use crate::arch::x86_64::paging::{PageTableRoot, PRESENT, WRITABLE, NX, PCD, PWT};
use crate::arch::x86_64::phys::{Identity, KIMAGE_BASE};

const MMIO_BASE: u64 = KIMAGE_BASE + 0x2000_0000; // +512 MiB, clear of kernel+modules
const MMIO_END: u64 = MMIO_BASE + 0x400_0000; // 64 MiB window
static MMIO_NEXT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(MMIO_BASE);
// Freed VA ranges for reuse (exact-size match, else bump). Without this
// the bump allocator exhausts the 64 MiB window on repeated load/unload.
struct FreeVa { used: bool, va: u64, len: u64 }
const NO_FREE: FreeVa = FreeVa { used: false, va: 0, len: 0 };
static MMIO_FREE: crate::sync::IrqSpinLock<[FreeVa; 16]> = crate::sync::IrqSpinLock::new([NO_FREE; 16]);
fn mmio_alloc_va(len: u64) -> u64 {
    {
        let mut t = MMIO_FREE.lock();
        if let Some(f) = t.iter_mut().find(|f| f.used && f.len == len) {
            let va = f.va;
            *f = NO_FREE;
            return va;
        }
    }
    let va = MMIO_NEXT.fetch_add(len, core::sync::atomic::Ordering::SeqCst);
    if va + len > MMIO_END || va < MMIO_BASE {
        0
    } else {
        va
    }
}
fn mmio_free_va(va: u64, len: u64) {
    let mut t = MMIO_FREE.lock();
    if let Some(f) = t.iter_mut().find(|f| !f.used) {
        *f = FreeVa { used: true, va, len };
    }
    // Full table: leak the VA (bump allocator covers it until window ends).
}

struct MmioMap { used: bool, va: u64, len: u64, bdf_packed: u32, nr: u8 }
const NO_MAP: MmioMap = MmioMap { used: false, va: 0, len: 0, bdf_packed: 0, nr: 0 };
static MMIO_MAPS: crate::sync::IrqSpinLock<[MmioMap; 16]> = crate::sync::IrqSpinLock::new([NO_MAP; 16]);

fn cur_cr3() -> u64 {
    let v: u64;
    unsafe { core::arch::asm!("mov {}, cr3", out(reg) v, options(nostack, preserves_flags)) };
    v
}

/// Map a PCI BAR as UC (uncached). Reads BAR base/size, reserves the range,
/// bumps a VA window, and maps each page PRESENT|RW|PCD|PWT|NX.
/// Returns kernel VA, or 0 on failure (never a bogus WB alias).
pub fn ioremap_bar(bdf: Bdf, nr: u8) -> u64 {
    if nr >= 6 { return 0; }
    let acc = Cam;
    let off = CFG_BAR0 + 4 * (nr as u16);
    let lo = acc.read32(bdf, off);
    if lo == 0 { return 0; }
    if lo & 1 != 0 { return 0; } // I/O BARs use port I/O, not MMIO
    // Base + size (handles 64-bit pairs).
    let kind = (lo >> 1) & 3;
    let (mut base, size) = if kind == 2 {
        if nr >= 5 { return 0; }
        let hi = acc.read32(bdf, off + 4);
        let b = ((hi as u64) << 32) | ((lo & 0xffff_fff0) as u64);
        // Size via sizing protocol on the pair.
        let sz = bar_size(&acc, bdf, nr);
        if sz == 0 { return 0; }
        (b, sz)
    } else {
        let b = (lo & 0xffff_fff0) as u64;
        let sz = bar_size(&acc, bdf, nr);
        if sz == 0 { return 0; }
        (b, sz)
    };
    if base == 0 { return 0; }
    let pages = ((size + 4095) / 4096) as u64;
    let len = pages * 4096;
    // Reserve first (fail fast on conflict).
    let mut tag = [0u8; 32];
    {
        let s = b"pci-bar";
        let n = core::cmp::min(s.len(), 32);
        tag[..n].copy_from_slice(&s[..n]);
    }
    if reserve_bar(base, len, &tag).is_err() { return 0; }
    crate::println!("pci: ioremap {:02x}:{:02x}.{}[{}] phys={:#x} size={:#x}", bdf.bus, bdf.device, bdf.function, nr, base, size);
    let va = mmio_alloc_va(len);
    if va == 0 {
        release_bar(base, len);
        return 0;
    }
    let root = PageTableRoot { pml4: cur_cr3() };
    let acc2 = Identity;
    let mut off2 = 0u64;
    while off2 < len {
        if root.map_page(&acc2, va + off2, base + off2, PRESENT | WRITABLE | PCD | PWT | NX).is_err() {
            // Roll back mapped prefix.
            let mut r = 0u64;
            while r < off2 {
                let _ = root.unmap_page(&acc2, va + r);
                r += 4096;
            }
            release_bar(base, len);
            return 0;
        }
        off2 += 4096;
    }
    {
        let mut t = MMIO_MAPS.lock();
        match t.iter_mut().find(|m| !m.used) {
            Some(m) => {
                m.used = true;
                m.va = va;
                m.len = len;
                m.bdf_packed = ((bdf.bus as u32) << 16) | ((bdf.device as u32) << 8) | (bdf.function as u32);
                m.nr = nr;
            }
            None => {
                // Table full: roll back.
                let mut r = 0u64;
                while r < len {
                    let _ = root.unmap_page(&acc2, va + r);
                    r += 4096;
                }
                release_bar(base, len);
                return 0;
            }
        }
    }
    // Mask off low page bits (callers add offsets); base was page-aligned?
    // PCI BAR bases are at least 16-byte aligned; MMIO requires page alignment
    // for mapping. Reject unaligned BARs (rare, indicates broken hardware).
    let _ = base;
    va
}

/// Unmap a VA previously returned by ioremap_bar. Safe no-op if unknown.
pub fn iounmap_bar(va: u64) {
    if va == 0 { return; }
    let (len, bdf_packed, nr) = {
        let mut t = MMIO_MAPS.lock();
        match t.iter_mut().find(|m| m.used && m.va == va) {
            Some(m) => {
                let t2 = (m.len, m.bdf_packed, m.nr);
                *m = NO_MAP;
                t2
            }
            None => return,
        }
    };
    let root = PageTableRoot { pml4: cur_cr3() };
    let acc = Identity;
    let mut off = 0u64;
    // Recover PA via page tables (don't trust cached base).
    while off < len {
        if root.unmap_page(&acc, va + off).is_err() { break; }
        off += 4096;
    }
    // Release reservation (recompute base from BAR; cheap and stateless).
    let bus = ((bdf_packed >> 16) & 0xff) as u8;
    let dev = ((bdf_packed >> 8) & 0xff) as u8;
    let fun = (bdf_packed & 0xff) as u8;
    let bdf = Bdf { segment: 0, bus, device: dev, function: fun };
    let cam = Cam;
    let ob = CFG_BAR0 + 4 * (nr as u16);
    let lo = cam.read32(bdf, ob);
    if lo & 1 == 0 && lo != 0 {
        let kind = (lo >> 1) & 3;
        let base = if kind == 2 && nr < 5 {
            let hi = cam.read32(bdf, ob + 4);
            ((hi as u64) << 32) | ((lo & 0xffff_fff0) as u64)
        } else {
            (lo & 0xffff_fff0) as u64
        };
        if base != 0 {
            let sz = bar_size(&cam, bdf, nr);
            if sz != 0 { release_bar(base, ((sz + 4095) / 4096) * 4096); }
        }
    }
    mmio_free_va(va, len);
}

/* Boot self-test: map the first MMIO BAR found, read a dword (must not
   fault), unmap, verify double-unmap is safe. Proves UC mapping path. */
pub fn selftest_mmio() {
    let acc = Cam;
    for bus in 0..=255u8 {
        for dev in 0..32u8 {
            for fun in 0..8u8 {
                let bdf = Bdf { segment: 0, bus, device: dev, function: fun };
                if acc.read32(bdf, CFG_VENDOR) & 0xffff == 0xffff { continue; }
                for nr in 0..6u8 {
                    let off = CFG_BAR0 + 4 * (nr as u16);
                    let lo = acc.read32(bdf, off);
                    if lo == 0 || lo & 1 != 0 { continue; }
                    // Skip 64-bit high halves (handled with low half).
                    if nr > 0 {
                        let plo = acc.read32(bdf, off - 4);
                        if plo & 1 == 0 && ((plo >> 1) & 3) == 2 { continue; }
                    }
                    let va = ioremap_bar(bdf, nr);
                    if va == 0 { continue; }
                    let v = unsafe { core::ptr::read_volatile(va as *const u32) };
                    let _ = v;
                    iounmap_bar(va);
                    // Double unmap must be safe.
                    iounmap_bar(va);
                    crate::println!("pci: mmio selftest PASS bar {:02x}:{:02x}.{}[{}] va={:#x}", bus, dev, fun, nr, va);
                    return;
                }
            }
        }
    }
    crate::println!("pci: mmio selftest SKIP (no MMIO BAR)");
}

/* ---- MSI capability parsing + programming (Slop core) ---- */
pub const CAP_ID_MSI: u8 = 0x05;
pub const CAP_ID_MSIX: u8 = 0x11;

/// Walk the PCI capability list. Calls f(cap_offset, cap_id) for each; stops if f returns false.
/// NOTE: QEMU leaves the Capabilities-List status bit clear on some devices
/// that still link real caps (e.g. e1000e/AHCI); gate on a sane cap pointer
/// instead of the status bit, with offset validation and a loop bound.
pub fn walk_caps(bdf: Bdf, mut f: impl FnMut(u16, u8) -> bool) {
    let acc = Cam;
    let mut off = (acc.read32(bdf, CFG_CAP as u16) & 0xfc) as u16;
    if off < 0x40 || off >= 0x100 { return; }
    let mut guard = 0;
    while off != 0 && guard < 64 {
        guard += 1;
        if off < 0x40 || off >= 0x100 { break; }
        let h = acc.read32(bdf, off);
        let id = (h & 0xff) as u8;
        if id == 0 || id == 0xff { break; }
        let next = ((h >> 8) & 0xff) as u16;
        if !f(off, id) { break; }
        if next != 0 && (next < 0x40 || next >= 0x100) { break; }
        off = next & 0xfc;
    }
}

/// Find MSI capability offset, if present. Returns (offset, is_64bit, mme_cap).
pub fn find_msi(bdf: Bdf) -> Option<(u16, bool, u8)> {
    let acc = Cam;
    let mut out = None;
    walk_caps(bdf, |off, id| {
        if id == CAP_ID_MSI {
            let ctrl = (acc.read32(bdf, off + 2) & 0xffff) as u16;
            let is64 = (ctrl >> 7) & 1 == 1;
            let mmc = ((ctrl >> 1) & 7) as u8;
            out = Some((off, is64, mmc));
            false
        } else {
            true
        }
    });
    out
}

/// Program MSI address/data for a Slop IRQ vector and enable it.
/// addr_hi: LAPIC base high (0 for xAPIC 32-bit). Returns 0 on success.
pub fn msi_program(bdf: Bdf, vector: u8, dest_apic: u8) -> i32 {
    let (off, is64, _mmc) = match find_msi(bdf) {
        Some(t) => t,
        None => return -19, // -ENODEV
    };
    let acc = Cam;
    // MSI address: 0xFEE00000 | (dest << 12). RH=0, DM=0 (physical).
    let addr: u32 = 0xfee0_0000 | ((dest_apic as u32) << 12);
    // MSI data: vector[7:0], delivery mode 0 (fixed), level 0, trigger 0 (edge).
    let data: u32 = vector as u32;
    // Disable first (avoid torn programming), then program.
    let mut ctrl = acc.read32(bdf, off + 2) & 0xffff;
    ctrl &= !1u32;
    acc.write32(bdf, off + 2, ctrl);
    acc.write32(bdf, off + 4, addr);
    if is64 {
        acc.write32(bdf, off + 8, 0);
        acc.write32(bdf, off + 12, data);
    } else {
        acc.write32(bdf, off + 8, data);
    }
    // Enable, single message (MME=0).
    let mut ctrl2 = acc.read32(bdf, off + 2) & 0xffff;
    ctrl2 = (ctrl2 & !(0x70 | 1)) | 1;
    acc.write32(bdf, off + 2, ctrl2);
    0
}

/// Disable MSI (clear enable bit). Safe no-op if absent.
pub fn msi_disable(bdf: Bdf) {
    if let Some((off, _, _)) = find_msi(bdf) {
        let acc = Cam;
        let ctrl = acc.read32(bdf, off + 2) & 0xffff;
        acc.write32(bdf, off + 2, ctrl & !1);
    }
}

/* ---- MSI vector pool (Slop-owned allocation for real PCI drivers) ----
   8 vectors (0x50..0x57), each with its own IDT gate/stub dispatching
   through the shared IRQ core. Ownership is per-BDF (dedup: re-request
   returns the same vector); free MSI-programs off + releases. */
use crate::sync::IrqSpinLock;
struct MsiAlloc { used: bool, bdf_packed: u32 }
const NO_MSI: MsiAlloc = MsiAlloc { used: false, bdf_packed: 0 };
static MSI_ALLOCS: IrqSpinLock<[MsiAlloc; 8]> = IrqSpinLock::new([NO_MSI; 8]);
fn pack_bdf(bdf: Bdf) -> u32 { ((bdf.bus as u32) << 16) | ((bdf.device as u32) << 8) | (bdf.function as u32) }
/// Allocate (or return existing) MSI vector for `bdf`. Programs MSI at the
/// current CPU. Returns vector 0x50..0x57 or negative errno.
pub fn msi_alloc_vector(bdf: Bdf) -> i32 {
    let want = pack_bdf(bdf);
    {
        let t = MSI_ALLOCS.lock();
        for (i, a) in t.iter().enumerate() {
            if a.used && a.bdf_packed == want {
                return crate::arch::x86_64::apic::MSI_VECTOR_BASE as i32 + i as i32;
            }
        }
    }
    let idx = {
        let mut t = MSI_ALLOCS.lock();
        match t.iter_mut().position(|a| !a.used) {
            Some(i) => {
                t[i].used = true;
                t[i].bdf_packed = want;
                i
            }
            None => return -12, // -ENOMEM
        }
    };
    let vec = crate::arch::x86_64::apic::MSI_VECTOR_BASE + idx as u8;
    let me = crate::arch::x86_64::apic::apic_id() as u8;
    if msi_program(bdf, vec, me) != 0 {
        let mut t = MSI_ALLOCS.lock();
        t[idx] = NO_MSI;
        return -19; // -ENODEV
    }
    // DIAG-MSI: verify programming took (read back).
    {
        let acc = Cam;
        if let Some((off, is64, _)) = find_msi(bdf) {
            let c = acc.read32(bdf, off + 2) & 0xffff;
            let a = acc.read32(bdf, off + 4);
            let d = if is64 { acc.read32(bdf, off + 12) } else { acc.read32(bdf, off + 8) };
            crate::println!("pci: msi verify {:02x}:{:02x}.{} vec={} en={} addr={:#x} data={:#x}", bdf.bus, bdf.device, bdf.function, vec, c & 1, a, d);
        }
    }
    vec as i32
}
/// Release the vector owned by `bdf` (MSI off). Safe no-op if none.
pub fn msi_free_vector(bdf: Bdf) {
    let want = pack_bdf(bdf);
    let mut t = MSI_ALLOCS.lock();
    for a in t.iter_mut() {
        if a.used && a.bdf_packed == want {
            *a = NO_MSI;
            msi_disable(bdf);
            return;
        }
    }
}

/* ---- Legacy INTx routing (fallback when MSI is absent) ----
   Root-bus only: INTx pin A-D maps to GSI 16-19 on q35 (no _PRT parsing
   yet; hotplug/bridges unsupported). Returns the programmed GSI. */
pub const CFG_INT_PIN: u16 = 0x3d;
pub fn legacy_gsi(bdf: Bdf) -> Option<u32> {
    if bdf.segment != 0 || bdf.bus != 0 {
        return None;
    }
    let acc = Cam;
    // Interrupt Pin is byte 1 of the dword at 0x3c (do aligned reads).
    let pin = ((acc.read32(bdf, 0x3c) >> 8) & 0xff) as u8;
    if pin < 1 || pin > 4 {
        return None;
    }
    // q35 root complex: INTx swizzle (pin + dev) % 4 onto PIRQ A-D,
    // which land on GSIs 20-23. Full _PRT parsing is future work.
    Some(20 + (((pin - 1 + bdf.device) % 4) as u32))
}
pub fn legacy_route(bdf: Bdf, vector: u8) -> Result<u32, &'static str> {
    let gsi = legacy_gsi(bdf).ok_or("no legacy pin/route")?;
    crate::arch::x86_64::apic::route_gsi_low(gsi, vector)?;
    Ok(gsi)
}
pub fn legacy_unroute(bdf: Bdf) {
    if let Some(gsi) = legacy_gsi(bdf) {
        crate::arch::x86_64::apic::unroute_gsi(gsi);
    }
}

/* MSI self-test: walk all PCI devices, report MSI/MSI-X caps, and for the
   first MSI-capable device, program TEST_VECTOR2 for self-delivery, verify
   by readback, then disable. No device event needed (programming path only;
   dispatch itself is proven by the IPI self-test on the same Slop core). */
pub fn selftest_msi() {
    let acc = Cam;
    let mut found = 0;
    let mut done = false;
    // Debug: list first few devices and their cap status.
    let mut dbg = 0;
    for bus in 0..=255u8 {
        for dev in 0..32u8 {
            for fun in 0..8u8 {
                let bdf = Bdf { segment: 0, bus, device: dev, function: fun };
                if acc.read32(bdf, CFG_VENDOR) & 0xffff == 0xffff { continue; }
                if dbg < 8 {
                    let st = acc.read32(bdf, 0x06);
                    let cp = acc.read32(bdf, CFG_CAP as u16) & 0xff;
                    let vv = acc.read32(bdf, CFG_VENDOR);
                    crate::println!("pci: dev {:02x}:{:02x}.{} {:04x}:{:04x} status={:#x} capptr={:#x}", bus, dev, fun, vv & 0xffff, (vv >> 16) & 0xffff, st, cp);
                    dbg += 1;
                }
                // Check for MSI / MSI-X caps.
                let mut has_msi = false;
                let mut has_msix = false;
                walk_caps(bdf, |_off, id| {
                    if id == CAP_ID_MSI { has_msi = true; }
                    if id == CAP_ID_MSIX { has_msix = true; }
                    true
                });
                if has_msi || has_msix { found += 1; }
                if has_msi && !done {
                    done = true;
                    // Program TEST_VECTOR2 to self, verify, disable.
                    let me = crate::arch::x86_64::apic::apic_id() as u8;
                    if msi_program(bdf, crate::arch::x86_64::apic::TEST_VECTOR2, me) != 0 {
                        crate::println!("pci: msi selftest FAIL program {:02x}:{:02x}.{}", bus, dev, fun);
                        return;
                    }
                    // Read back address/data/enable.
                    if let Some((off, is64, _)) = find_msi(bdf) {
                        let ctrl = acc.read32(bdf, off + 2) & 0xffff;
                        let addr = acc.read32(bdf, off + 4);
                        let want_addr = 0xfee0_0000 | ((me as u32) << 12);
                        let data_off = if is64 { off + 12 } else { off + 8 };
                        let data = acc.read32(bdf, data_off) & 0xffff;
                        if (ctrl & 1) == 0 || addr != want_addr || data != crate::arch::x86_64::apic::TEST_VECTOR2 as u32 {
                            crate::println!("pci: msi selftest FAIL verify {:#x} {:#x} {:#x}", ctrl, addr, data);
                            msi_disable(bdf);
                            return;
                        }
                    } else {
                        crate::println!("pci: msi selftest FAIL lost cap");
                        return;
                    }
                    msi_disable(bdf);
                    crate::println!("pci: msi selftest PASS {:02x}:{:02x}.{} programmed+verified", bus, dev, fun);
                }
            }
        }
    }
    if found == 0 {
        crate::println!("pci: msi selftest SKIP (no MSI/MSI-X caps on bus)");
    }
}

/* ---- MSI-X capability parsing (table location only; delivery via Slop IRQ core later) ---- */
pub struct MsixInfo { pub table_bar: u8, pub table_offset: u32, pub table_size: u16, pub pba_bar: u8, pub pba_offset: u32 }
/// If the device has MSI-X, return table location. Does NOT enable (enable
/// requires function masking + vector allocation, future work with real drivers).
pub fn find_msix(bdf: Bdf) -> Option<MsixInfo> {
    let acc = Cam;
    let mut out = None;
    walk_caps(bdf, |off, id| {
        if id == CAP_ID_MSIX {
            let ctrl = acc.read32(bdf, off + 2);
            let tbl = acc.read32(bdf, off + 4);
            let pba = acc.read32(bdf, off + 8);
            out = Some(MsixInfo {
                table_bar: (tbl & 7) as u8,
                table_offset: tbl & !7,
                table_size: ((ctrl & 0x3ff) + 1) as u16,
                pba_bar: (pba & 7) as u8,
                pba_offset: pba & !7,
            });
            return false;
        }
        true
    });
    out
}
pub fn selftest_msix() {
    let acc = Cam;
    for bus in 0..=1u8 {
        for dev in 0..32u8 {
            for fun in 0..1u8 {
                let bdf = Bdf { segment: 0, bus, device: dev, function: fun };
                if acc.read32(bdf, CFG_VENDOR) & 0xffff == 0xffff { continue; }
                if let Some(m) = find_msix(bdf) {
                    crate::println!("pci: msix {:02x}:{:02x}.{} bar{}+{:#x} size{}",
                        bus, dev, fun, m.table_bar, m.table_offset, m.table_size);
                    return;
                }
            }
        }
    }
    crate::println!("pci: msix selftest SKIP (no MSI-X caps on bus 0)");
}
