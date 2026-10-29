//! Loadable Linux kernel modules (.ko): ET_REL loader, kernel symbol
//! resolution, PCI driver registry, and init/exit lifecycle.
//!
//! Design notes (why not the direct map): `-mcmodel=kernel` objects use
//! R_X86_64_PC32/PLT32 (±2 GiB). The direct map (PHYS_BASE) is ~16 TiB from
//! the kernel image, so PC32 overflows. Modules are therefore mapped in a
//! reserved area 256 MiB above KIMAGE_BASE (well within ±2 GiB), with RX
//! text and RW data (W^X). Unresolved symbols fail the load, except
//! `module_layout` (a version CRC meaningless here: we are deliberately not
//! the kernel the module was built for; vermagic mismatch only warns).
//!
//! -mcmodel=large (absolute 64-bit) is the current tested path. Stock
//! `-mcmodel=kernel` emits R_X86_64_32/32S absolute data refs, which need
//! sign-extended high addresses (0xffffffff80000000+); Slop at 0xffff9000…
//! cannot satisfy both that *and* ±2 GiB PC32 reachability at once. The
//! compatible end-state is a high-half kernel alias (map the image a second
//! time at 0xffffffff80000000, export the alias VAs, load modules nearby):
//! then PC32 stays in range *and* 32-bit abs sign-extends correctly. That
//! move is intentionally deferred until the RDI call-ABI fault (see ABI
//! battery) is root-caused, since no .ko can be meaningfully validated
//! before argument passing works.
use crate::arch::x86_64::paging::{PageTableRoot, PRESENT, WRITABLE, NX, ADDR};
use crate::arch::x86_64::phys::{Identity, KIMAGE_BASE};
use crate::memory::{alloc_frame, free_frame};
use crate::sync::IrqSpinLock;
use core::sync::atomic::{AtomicU64, Ordering};

/* ---- ELF ET_REL constants (Section Headers, not Program Headers) ---- */
const ET_REL: u16 = 1;
const EM_X86_64: u16 = 62;
const SHT_NULL: u32 = 0;
const SHT_NOBITS: u32 = 8;
const SHT_RELA: u32 = 4;
const SHF_ALLOC: u64 = 2;
const SHF_EXECINSTR: u64 = 4;
const STB_GLOBAL: u8 = 1;
const STT_NOTYPE: u8 = 0;
const STT_OBJECT: u8 = 1;
const STT_FUNC: u8 = 2;
const SHN_UNDEF: u16 = 0;
const SHN_ABS: u16 = 0xfff1;
/* Rela types we apply. */
const R_NONE: u32 = 0;
const R_64: u32 = 1;
const R_PC32: u32 = 2;
const R_GOT32: u32 = 3;
const R_PLT32: u32 = 4;
const R_32: u32 = 10;
const R_32S: u32 = 11;
const R_GOTPCREL: u32 = 9;

/* Module area: 256 MiB above the kernel image, 64 MiB window, bump-allocated. */
const MODULE_BASE: u64 = KIMAGE_BASE + 0x1000_0000;
const MODULE_END: u64 = MODULE_BASE + 0x400_0000;
static MODULE_NEXT: AtomicU64 = AtomicU64::new(MODULE_BASE);

const MAX_MODS: usize = 32;
struct LoadedMod {
    used: bool,
    name: [u8; 64],
    namelen: usize,
    base: u64,
    pages: usize,
    init: u64,
    exit: u64,
}
const EMPTY_MOD: LoadedMod = LoadedMod { used: false, name: [0; 64], namelen: 0, base: 0, pages: 0, init: 0, exit: 0 };
static MODS: IrqSpinLock<[LoadedMod; MAX_MODS]> = IrqSpinLock::new([EMPTY_MOD; MAX_MODS]);

fn u16at(b: &[u8], o: usize) -> Result<u16, &'static str> {
    Ok(u16::from_le_bytes(b.get(o..o + 2).ok_or("trunc")?.try_into().map_err(|_| "trunc")?))
}
fn u32at(b: &[u8], o: usize) -> Result<u32, &'static str> {
    Ok(u32::from_le_bytes(b.get(o..o + 4).ok_or("trunc")?.try_into().map_err(|_| "trunc")?))
}
fn u64at(b: &[u8], o: usize) -> Result<u64, &'static str> {
    Ok(u64::from_le_bytes(b.get(o..o + 8).ok_or("trunc")?.try_into().map_err(|_| "trunc")?))
}
fn cur_cr3() -> u64 {
    let v: u64;
    unsafe { core::arch::asm!("mov {}, cr3", out(reg) v, options(nostack, preserves_flags)) };
    v
}
/* Translate a module VA to a direct-map pointer for the write path.
   Reads/writes go through the RW alias; execution uses the RX mapping,
   so fetch never observes a partially-written instruction stream. */
fn module_write_ptr(base: u64, frames: &[u64; 256], va: u64) -> *mut u8 {
    let off = va.wrapping_sub(base);
    let pi = (off / 4096) as usize;
    let po = (off % 4096) as usize;
    let pa = frames[pi];
    (crate::arch::x86_64::phys::phys_to_virt(pa) + po as u64) as *mut u8
}

/* Read a section header field block. shoff/shentsize/shnum/shstrndx drive it. */
struct Shdr {
    name: u32,
    stype: u32,
    flags: u64,
    addr: u64,
    offset: u64,
    size: u64,
    link: u32,
    info: u32,
    addralign: u64,
    entsize: u64,
}
fn read_shdr(img: &[u8], shoff: u64, entsize: usize, idx: usize) -> Result<Shdr, &'static str> {
    let o = (shoff as usize).checked_add(idx.checked_mul(entsize).ok_or("shdr overflow")?).ok_or("shdr overflow")?;
    if o.checked_add(64).ok_or("shdr overflow")? > img.len() { return Err("shdr truncated"); }
    Ok(Shdr {
        name: u32at(img, o)?, stype: u32at(img, o + 4)?, flags: u64at(img, o + 8)?,
        addr: u64at(img, o + 16)?, offset: u64at(img, o + 24)?, size: u64at(img, o + 32)?,
        link: u32at(img, o + 40)?, info: u32at(img, o + 44)?,
        addralign: u64at(img, o + 48)?, entsize: u64at(img, o + 56)?,
    })
}
/* Fetch section `idx` bytes (file content; NOBITS has none). */
fn sec_bytes<'a>(img: &'a [u8], sh: &Shdr) -> Result<&'a [u8], &'static str> {
    if sh.stype == SHT_NOBITS { return Ok(&[]); }
    let o = sh.offset as usize;
    let e = o.checked_add(sh.size as usize).ok_or("section overflow")?;
    img.get(o..e).ok_or("section truncated")
}

/* C string from a strtab at `off`. */
fn cstr<'a>(tab: &'a [u8], off: usize) -> Result<&'a str, &'static str> {
    let mut e = off;
    while *tab.get(e).ok_or("strtab truncated")? != 0 { e += 1; }
    core::str::from_utf8(tab.get(off..e).ok_or("strtab truncated")?).map_err(|_| "strtab utf8")
}

/* ---- Kernel symbol exports for .ko undefined symbols ---- */
struct Export { name: &'static str, addr: u64 }
fn export_table() -> [Export; 84] {
    use crate::linux_compat as lc;
    let e = |n: &'static str, a: u64| Export { name: n, addr: a };
    [
        e("printk", lc::printk as usize as u64),
        e("_printk", lc::printk as usize as u64),
        e("kmalloc", lc::kmalloc as usize as u64),
        e("kzalloc", lc::kzalloc as usize as u64),
        e("kcalloc", lc::kcalloc as usize as u64),
        e("kmemdup", lc::kmemdup as usize as u64),
        e("kfree", lc::kfree as usize as u64),
        e("ksize", lc::ksize as usize as u64),
        e("__kmalloc", lc::kmalloc as usize as u64),
        e("krealloc", lc::krealloc as usize as u64),
        e("msleep", lc::msleep as usize as u64),
        e("msleep_interruptible", lc::msleep as usize as u64),
        e("mdelay", lc::mdelay as usize as u64),
        e("udelay", lc::udelay as usize as u64),
        e("__const_udelay", lc::__const_udelay as usize as u64),
        e("__udelay", lc::__udelay as usize as u64),
        e("usleep_range", lc::usleep_range as usize as u64),
        e("jiffies", lc::jiffies_addr()),
        e("pci_register_driver", lc::pci_register_driver as usize as u64),
        e("__pci_register_driver", lc::pci_register_driver as usize as u64),
        e("pci_unregister_driver", lc::pci_unregister_driver as usize as u64),
        e("pci_enable_device", lc::pci_enable_device as usize as u64),
        e("pci_disable_device", lc::pci_disable_device as usize as u64),
        e("pci_request_regions", lc::pci_request_regions as usize as u64),
        e("pci_release_regions", lc::pci_release_regions as usize as u64),
        e("pci_set_master", lc::pci_set_master as usize as u64),
        e("pci_clear_master", lc::pci_clear_master as usize as u64),
        e("pci_set_drvdata", lc::pci_set_drvdata as usize as u64),
        e("pci_get_drvdata", lc::pci_get_drvdata as usize as u64),
        e("pci_ioremap_bar", lc::pci_ioremap_bar as usize as u64),
        e("pci_iounmap", lc::pci_iounmap as usize as u64),
        e("ioremap", lc::ioremap as usize as u64),
        e("iounmap", lc::iounmap as usize as u64),
        e("ioread8", lc::ioread8 as usize as u64),
        e("ioread16", lc::ioread16 as usize as u64),
        e("ioread32", lc::ioread32 as usize as u64),
        e("iowrite8", lc::iowrite8 as usize as u64),
        e("iowrite16", lc::iowrite16 as usize as u64),
        e("iowrite32", lc::iowrite32 as usize as u64),
        e("request_irq", lc::request_irq as usize as u64),
        e("free_irq", lc::free_irq as usize as u64),
        e("capable", lc::capable as usize as u64),
        e("test_u64_arg", lc::test_u64_arg as usize as u64),
        e("test_store_addr", lc::test_store_addr as usize as u64),
        e("dma_alloc_coherent", lc::dma_alloc_coherent as usize as u64),
        e("dma_free_coherent", lc::dma_free_coherent as usize as u64),
        // Modern kernels route dma_alloc_coherent through _attrs (extra trailing
        // attrs arg, ignored by our 4-arg shim — SysV extra args are harmless).
        e("dma_alloc_attrs", lc::dma_alloc_coherent as usize as u64),
        e("dma_free_attrs", lc::dma_free_coherent as usize as u64),
        e("dma_map_single", lc::dma_map_single as usize as u64),
        e("dma_unmap_single", lc::dma_unmap_single as usize as u64),
        e("dma_sync_single_for_cpu", lc::dma_sync_single_for_cpu as usize as u64),
        e("dma_sync_single_for_device", lc::dma_sync_single_for_device as usize as u64),
        e("dma_map_sg", lc::dma_map_sg as usize as u64),
        e("dma_unmap_sg", lc::dma_unmap_sg as usize as u64),
        e("abi_probe_6", lc::abi_probe_6 as usize as u64),
        e("abi_verify", lc::abi_verify as usize as u64),
        e("pci_register_simple", lc::pci_register_simple as usize as u64),
        e("__fentry__", lc::ret_thunk_addr()),
        e("__x86_return_thunk", lc::ret_thunk_addr()),
        e("__stack_chk_fail", lc::stack_chk_fail as usize as u64),
        e("memset", lc::k_memset as usize as u64),
        e("memcpy", lc::k_memcpy as usize as u64),
        e("memmove", lc::k_memmove as usize as u64),
        e("strlen", lc::k_strlen as usize as u64),
        e("strcmp", lc::k_strcmp as usize as u64),
        e("strncmp", lc::k_strncmp as usize as u64),
        e("strncpy", lc::k_strncpy as usize as u64),
        e("sprintf", lc::k_sprintf as usize as u64),
        e("snprintf", lc::k_snprintf as usize as u64),
        e("vsnprintf", lc::k_vsnprintf as usize as u64),
        e("", 0), e("", 0), e("", 0), e("", 0), e("", 0), e("", 0), e("", 0),
        e("", 0), e("", 0), e("", 0), e("", 0), e("", 0), e("", 0), e("", 0),
    ]
}
fn export_lookup(name: &str) -> Option<u64> {
    if name == "module_layout" { return Some(0); } // version CRC: intentionally not enforced
    for e in export_table() {
        if !e.name.is_empty() && e.name == name { return Some(e.addr); }
    }
    None
}

/* ---- Loader ---- */
#[derive(Clone, Copy)]
struct LoadedSec { used: bool, va: u64, size: u64 }

pub fn load_ko(img: &[u8]) -> Result<(), &'static str> {
    if img.len() < 64 { return Err("too small"); }
    if img[0..4] != *b"\x7fELF" { return Err("bad magic"); }
    if img[4] != 2 || img[5] != 1 || img[6] != 1 { return Err("not ELF64 LE v1"); }
    if u16at(img, 16)? != ET_REL { return Err("not ET_REL"); }
    if u16at(img, 18)? != 62 { return Err("wrong machine"); }
    let shoff = u64at(img, 40)?;
    let shentsize = u16at(img, 58)? as usize;
    let shnum = u16at(img, 60)? as usize;
    let shstrndx = u16at(img, 62)? as usize;
    if shentsize < 64 || shnum == 0 || shnum > 128 { return Err("bad shdr table"); }
    if shstrndx >= shnum { return Err("bad shstrndx"); }
    // Section names.
    let shstr = read_shdr(img, shoff, shentsize, shstrndx)?;
    let shstrtab = sec_bytes(img, &shstr)?;
    let secname = |idx: usize| -> Result<&str, &'static str> {
        let sh = read_shdr(img, shoff, shentsize, idx)?;
        cstr(shstrtab, sh.name as usize)
    };
    // Locate key sections.
    let mut symtab: Option<usize> = None;
    let mut strtab: Option<usize> = None;
    let mut modinfo: Option<usize> = None;
    for i in 0..shnum {
        match secname(i)? {
            ".symtab" => symtab = Some(i),
            ".strtab" => strtab = Some(i),
            ".modinfo" => modinfo = Some(i),
            _ => {}
        }
    }
    let symtab = symtab.ok_or("no symtab")?;
    let strtab = strtab.ok_or("no strtab")?;
    let symsec = read_shdr(img, shoff, shentsize, symtab)?;
    let strsec = read_shdr(img, shoff, shentsize, strtab)?;
    let syms = sec_bytes(img, &symsec)?;
    let strs = sec_bytes(img, &strsec)?;
    let nsym = syms.len() / 24;
    // modinfo: name + vermagic (warn only).
    let mut modname = "unnamed";
    if let Some(mi) = modinfo {
        let msec = read_shdr(img, shoff, shentsize, mi)?;
        let mbytes = sec_bytes(img, &msec)?;
        let mut p = 0;
        while p < mbytes.len() {
            let mut e = p;
            while e < mbytes.len() && mbytes[e] != 0 { e += 1; }
            if let Ok(s) = core::str::from_utf8(&mbytes[p..e]) {
                if let Some(n) = s.strip_prefix("name=") { modname = n; }
                if let Some(v) = s.strip_prefix("vermagic=") {
                    crate::println!("module: vermagic {} (accepted, tainted)", v);
                }
            }
            p = e + 1;
        }
    }
    // Duplicate load?
    {
        let mods = MODS.lock();
        for m in mods.iter() {
            if m.used && m.namelen == modname.len() && &m.name[..m.namelen] == modname.as_bytes() {
                return Err("already loaded");
            }
        }
    }
    // Layout SHF_ALLOC sections (page-granular, in file order).
    let mut secs = [LoadedSec { used: false, va: 0, size: 0 }; 128];
    let mut total_pages: usize = 0;
    for i in 0..shnum {
        let sh = read_shdr(img, shoff, shentsize, i)?;
        if sh.stype == SHT_NULL || (sh.flags & SHF_ALLOC) == 0 { continue; }
        if i >= 128 { return Err("too many sections"); }
        let pages = ((sh.size as usize + 4095) / 4096).max(1);
        secs[i].used = true;
        secs[i].size = (pages * 4096) as u64;
        total_pages += pages;
    }
    if total_pages == 0 || total_pages > (MODULE_END - MODULE_BASE) as usize / 4096 {
        return Err("bad image size");
    }
    let base = MODULE_NEXT.fetch_add((total_pages * 4096) as u64, Ordering::SeqCst);
    if base + (total_pages * 4096) as u64 > MODULE_END { return Err("module area exhausted"); }
    // Map with final perms immediately (RX text, RW data). All writes go
    // through the direct-map alias, never through the executable mapping,
    // so there is no self-modifying-code window for fetch staleness.
    let root = PageTableRoot { pml4: cur_cr3() };
    let acc = Identity;
    let mut va = base;
    let mut frames: [u64; 256] = [0; 256];
    let mut nframes = 0usize;
    let mut is_text = [false; 128];
    for i in 0..shnum {
        if !secs[i].used { continue; }
        let sh = read_shdr(img, shoff, shentsize, i)?;
        let rx = (sh.flags & SHF_EXECINSTR) != 0;
        if i < 128 { is_text[i] = rx; }
        let pages = (secs[i].size / 4096) as usize;
        for _ in 0..pages {
            let pa = alloc_frame().ok_or("oom")?;
            if nframes < 256 { frames[nframes] = pa; nframes += 1; }
            let flags = PRESENT | if rx { 0 } else { WRITABLE | NX };
            root.map_page(&acc, va, pa, flags).map_err(|_| "map failed")?;
            va += 4096;
        }
    }
    // Assign VAs in order (recompute cleanly).

    let mut cur = base;
    for i in 0..shnum {
        if !secs[i].used { continue; }
        secs[i].va = cur;
        cur += secs[i].size;
        let sh = read_shdr(img, shoff, shentsize, i)?;
        // Copy content via direct map (NOBITS stays zeroed; frames may be dirty).
        let dst = module_write_ptr(base, &frames, secs[i].va);
        unsafe { core::ptr::write_bytes(dst, 0, secs[i].size as usize) };
        let sh = read_shdr(img, shoff, shentsize, i)?;
        if sh.stype != SHT_NOBITS && sh.size > 0 {
            let src = sec_bytes(img, &sh)?;
            unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), dst, src.len()) };
        }
    }
    // Apply RELA sections.
    for i in 0..shnum {
        let sh = read_shdr(img, shoff, shentsize, i)?;
        if sh.stype != SHT_RELA { continue; }
        let tgt = sh.info as usize; // section index this rela applies to
        if tgt >= 128 || !secs[tgt].used { return Err("bad rela target"); }
        let rela = sec_bytes(img, &sh)?;
        if rela.len() % 24 != 0 { return Err("bad rela size"); }
        for r in 0..rela.len() / 24 {
            let o = r * 24;
            let offset = u64at(rela, o)?;
            let info = u64at(rela, o + 8)?;
            let addend = i64::from_le_bytes(rela[o + 16..o + 24].try_into().map_err(|_| "trunc")?);
            let sym = (info >> 32) as usize;
            let typ = (info & 0xffff_ffff) as u32;
            if typ == R_NONE { continue; }
            // Symbol value.
            let so = sym * 24;
            let sname = u32at(syms, so)?;
            let sbind = syms.get(so + 4).copied().unwrap_or(0) >> 4;
            let sshndx = u16at(syms, so + 6)?;
            let sval = u64at(syms, so + 8)?;
            let _ = sbind;
            let sva = if sshndx == SHN_UNDEF {
                let nm = cstr(strs, sname as usize)?;
                match export_lookup(nm) {
                    Some(0) => 0, // module_layout / ignored
                    Some(a) => a,
                    None => {
                        crate::println!("module: unresolved symbol '{}'", nm);
                        return Err("unresolved symbol");
                    }
                }
            } else if sshndx == SHN_ABS {
                sval
            } else {
                let si = sshndx as usize;
                if si >= 128 || !secs[si].used { return Err("bad rela sym"); }
                secs[si].va + sval
            };
            let loc = secs[tgt].va + offset;
            let p = loc;
            match typ {
                R_64 => unsafe {
                    let v = sva.wrapping_add(addend as u64);
                    let wptr = module_write_ptr(base, &frames, loc);
                    core::ptr::write_unaligned(wptr as *mut u64, v);
                    // Read-back verification: catches mapping/aliasing bugs
                    // where the write lands elsewhere than the fetch path.
                    let back = core::ptr::read_volatile(wptr as *const u64);
                    if back != v {
                        crate::println!("module: RELOC MISMATCH loc={:#x} wrote={:#x} read={:#x}", loc, v, back);
                        return Err("reloc verify failed");
                    }
                },
                R_32 | R_32S => unsafe {
                    let wptr = module_write_ptr(base, &frames, loc);
                    core::ptr::write_unaligned(wptr as *mut u32, sva.wrapping_add(addend as u64) as u32)
                }
                R_PC32 | R_PLT32 => unsafe {
                    let v = (sva.wrapping_add(addend as u64)).wrapping_sub(p) as u32;
                    let wptr = module_write_ptr(base, &frames, loc);
                    core::ptr::write_unaligned(wptr as *mut u32, v)
                }
                _ => return Err("unsupported reloc"),
            }
        }
    }
    // Text was mapped RX from the start; writes went through the direct-map
    // alias, so no mprotect dance is needed (and fetch never observes tearing).
    // Find init_module / cleanup_module.
    let mut init = 0u64;
    let mut exit = 0u64;
    for s in 0..nsym {
        let o = s * 24;
        let nm = u32at(syms, o)?;
        let shndx = u16at(syms, o + 6)?;
        let val = u64at(syms, o + 8)?;
        if shndx == SHN_UNDEF || shndx == SHN_ABS { continue; }
        let si = shndx as usize;
        if si >= 128 || !secs[si].used { continue; }
        if let Ok(n) = cstr(strs, nm as usize) {
            if n == "init_module" { init = secs[si].va + val; }
            if n == "cleanup_module" { exit = secs[si].va + val; }
        }
    }
    if init == 0 { return Err("no init_module"); }
    // Register before calling init (so init can use our registry consistently).
    let slot = {
        let mut mods = MODS.lock();
        match mods.iter().position(|m| !m.used) {
            Some(i) => {
                let mut m = &mut mods[i];
                m.used = true;
                let nb = modname.as_bytes();
                let n = core::cmp::min(nb.len(), 63);
                m.name[..n].copy_from_slice(&nb[..n]);
                m.namelen = n;
                m.base = base;
                m.pages = total_pages;
                m.init = init;
                m.exit = exit;
                i
            }
            None => return Err("too many modules"),
        }
    };
    // DIAG-ABI: dump .init.text bytes via direct map to verify loader wrote file content.
    for i in 0..shnum {
        if !secs[i].used { continue; }
        let sh = read_shdr(img, shoff, shentsize, i)?;
        if let Ok(nm) = cstr(shstrtab, sh.name as usize) {
            if nm == ".init.text" {
                let basep = module_write_ptr(base, &frames, secs[i].va) as *const u8;
                let mut hb = [0u8; 32];
                for k in 0..32 {
                    hb[k] = unsafe { core::ptr::read_volatile(basep.add(k)) };
                }
                crate::println!("module: init.text+0 {:02x?}", &hb[..]);
                break;
            }
        }
    }
    // DIAG-ABI: dump .data first 16 bytes to verify copy.
    for i in 0..shnum {
        if !secs[i].used { continue; }
        let sh = read_shdr(img, shoff, shentsize, i)?;
        if let Ok(nm) = cstr(shstrtab, sh.name as usize) {
            if nm == ".data" {
                let basep = module_write_ptr(base, &frames, secs[i].va) as *const u64;
                for k in 0..2usize {
                    let v = unsafe { core::ptr::read_unaligned(basep.add(k)) };
                    crate::println!("module: .data[{}]={:#x}", k, v);
                }
                break;
            }
        }
    }
    // DIAG-ABI: verify .data content via module VA (catch copy/mapping bugs).
    // Reads through the EXEC mapping (same path CPU fetches), not direct map.
    for i in 0..shnum {
        if !secs[i].used { continue; }
        let sh = read_shdr(img, shoff, shentsize, i)?;
        if let Ok(nm) = cstr(shstrtab, sh.name as usize) {
            if nm == ".data" {
                let v = unsafe { core::ptr::read_volatile(secs[i].va as *const u64) };
                let f = match sec_bytes(img, &sh) {
                    Ok(b) if b.len() >= 8 => u64::from_le_bytes(b[..8].try_into().unwrap_or([0u8; 8])),
                    _ => 0xffff_ffff_ffff_ffff,
                };
                if v != f {
                    crate::println!("module: .data MISMATCH va={:#x} mem={:#x} file={:#x}", secs[i].va, v, f);
                } else {
                    crate::println!("module: .data VA-OK {:#x}", v);
                }
                // Also compare direct-map read at same PA (alias coherence).
                let wptr = module_write_ptr(base, &frames, secs[i].va);
                let w = unsafe { core::ptr::read_volatile(wptr as *const u64) };
                if w != v {
                    crate::println!("module: ALIAS MISMATCH va={:#x} vs direct={:#x}", v, w);
                }
            }
        }
    }
    // Call init: returns 0 on success, negative errno on failure.
    // DIAG: mask local APIC timer across init to rule out entry/exit clobber.
    unsafe {
        let lvt: u32 = crate::arch::x86_64::apic::read(0x320);
        crate::arch::x86_64::apic::write(0x320, lvt | 0x10000);
    }
    // DIAG-ABI infra (HW break + TF step) retained dormant; arm only when
    // chasing a call-ABI fault. The Win64/SysV mismatch is fixed (sysv64
    // exports), so normal loads run without tracing.
    let rc: i32 = unsafe {
        let f: unsafe extern "C" fn() -> i32 = core::mem::transmute(init as usize);
        let r = f();
        r
    };
    unsafe {
        let lvt: u32 = crate::arch::x86_64::apic::read(0x320);
        crate::arch::x86_64::apic::write(0x320, lvt & !0x10000);
    }
    if rc != 0 {
        // Unload: unmap + free frames, drop slot. Also drop any IRQ/PCI the
        // failed init registered (exit won't run, so no normal cleanup).
        crate::linux_compat::force_unregister_all_irqs();
        crate::linux_compat::force_unregister_all_pci();
        unload_pages(base, total_pages);
        let mut mods = MODS.lock();
        mods[slot] = EMPTY_MOD;
        crate::println!("module: {} init failed rc={}", modname, rc);
        return Err("init failed");
    }
    crate::println!("module: {} loaded @ {:#x} ({} pages)", modname, base, total_pages);
    Ok(())
}

fn unload_pages(base: u64, pages: usize) {
    let root = PageTableRoot { pml4: cur_cr3() };
    let acc = Identity;
    for i in 0..pages {
        let va = base + (i * 4096) as u64;
        if let Ok(pa) = root.unmap_page(&acc, va) {
            let _ = free_frame(pa);
        }
    }
}

pub fn unload(name: &str) -> Result<(), i64> {
    let (base, pages, exit) = {
        let mut mods = MODS.lock();
        match mods.iter().position(|m| m.used && m.namelen == name.len() && &m.name[..m.namelen] == name.as_bytes()) {
            Some(i) => {
                let m = &mods[i];
                let t = (m.base, m.pages, m.exit);
                mods[i] = EMPTY_MOD;
                t
            }
            None => return Err(crate::errno::ENOENT),
        }
    };
    if exit != 0 {
        unsafe {
            let f: unsafe extern "C" fn() = core::mem::transmute(exit as usize);
            f();
        }
    }
    // Unload safety net: drop any Linux IRQ/PCI registrations the module left
    // behind (Slop core waits out in-flight handlers; no UAF). Slop-native
    // drivers are unaffected (separate paths). Report DMA leaks (don't
    // silently free in-use buffers; that would corrupt ongoing transfers).
    crate::linux_compat::force_unregister_all_irqs();
    crate::linux_compat::force_unregister_all_pci();
    crate::linux_compat::pci_unregister_for_module();
    let live = crate::dma::live_maps();
    if live != 0 {
        crate::println!("module: {} unload with {} live DMA mappings (leaked, not force-freed)", name, live);
    }
    unload_pages(base, pages);
    crate::println!("module: {} unloaded", name);
    Ok(())
}

/* ---- Syscall entry points (bytes from user) ---- */
/* Staging buffer for .ko files (no global allocator in-kernel yet).
   Serialized by lock; 256 KiB cap keeps real-world test drivers loadable
   while large vendor drivers fail cleanly until demand paging lands. */
const STAGE_MAX: usize = 256 * 1024;
static mut STAGE: [u8; STAGE_MAX] = [0; STAGE_MAX];
static STAGE_LOCK: IrqSpinLock<()> = IrqSpinLock::new(());

fn stage_load(mut read: impl FnMut(&mut [u8]) -> Result<usize, i64>) -> Result<usize, i64> {
    let _g = STAGE_LOCK.lock();
    let mut total = 0usize;
    loop {
        if total >= STAGE_MAX { return Err(crate::errno::EFBIG); }
        let buf = unsafe { &mut STAGE[total..] };
        match read(buf) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) => {
                if total == 0 { return Err(e); }
                break;
            }
        }
    }
    if total < 64 { return Err(crate::errno::EINVAL); }
    // Copy out? No: parse + load synchronously under the lock (brief).
    let r = load_ko(unsafe { &STAGE[..total] });
    match r {
        Ok(()) => Ok(0),
        Err(e) => {
            crate::println!("module: load failed: {}", e);
            Err(crate::errno::ENOEXEC)
        }
    }
}

/* finit_module(fd, uargs, flags): read the .ko from fd, load it. */
pub fn sys_finit_module(fd: usize, _uargs: usize, _flags: u64) -> i64 {
    // Resolve fd -> OpenFile id once (brief PROCS use; the read loop below
    // holds no process-table lock, so blocking I/O cannot wedge the timer).
    let id = match crate::task::with_current_process(|p| p.fds.get(fd)) {
        Some(Some(v)) => v,
        _ => return crate::errno::neg(crate::errno::EBADF),
    };
    let r = stage_load(|buf| {
        let n = buf.len().min(8192);
        crate::fd::read(id, &mut buf[..n])
    });
    match r {
        Ok(_) => 0,
        Err(e) => crate::errno::neg(e),
    }
}

/* init_module(image, len): legacy; image is a user pointer. */
pub fn sys_init_module(image: usize, len: usize) -> i64 {
    if image == 0 || len == 0 || len > STAGE_MAX {
        return crate::errno::neg(crate::errno::EINVAL);
    }
    let mut done = 0usize;
    let r = stage_load(|buf| {
        let n = core::cmp::min(buf.len(), len - done);
        if n == 0 { return Ok(0); }
        crate::usercopy::copy_from_user(&mut buf[..n], image + done)?;
        done += n;
        Ok(n)
    });
    match r {
        Ok(_) => 0,
        Err(e) => crate::errno::neg(e),
    }
}

/* delete_module(name, flags). */
pub fn sys_delete_module(name_ptr: usize, _flags: u64) -> i64 {
    if name_ptr == 0 { return crate::errno::neg(crate::errno::EFAULT); }
    let mut buf = [0u8; 64];
    if crate::usercopy::copy_from_user(&mut buf, name_ptr).is_err() {
        return crate::errno::neg(crate::errno::EFAULT);
    }
    let e = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let name = match core::str::from_utf8(&buf[..e]) {
        Ok(s) => s,
        Err(_) => return crate::errno::neg(crate::errno::EINVAL),
    };
    // delete_module must not run while the module holds PROCS (same deadlock
    // class as the tcp_close fix: exit/teardown waits under the table lock).
    // Our unload path takes no scheduler locks across teardown (exit runs
    // first, then pages are unmapped), so this is safe by construction.
    match unload(name) {
        Ok(()) => 0,
        Err(e) => crate::errno::neg(-e),
    }
}

// DIAG-ABI: TF single-step tracer for module init (RDI fault isolation).
// Arms before the init call; the #DB handler logs RIP+RDI per step while
// RIP stays in [STEP_BASE, STEP_END) and steps remain. Distinguishes
// movabs-broken (RDI wrong right after movabs) from call-broken.
static mut STEP_ARMED: u64 = 0;
static mut STEP_BASE: u64 = 0;
static mut STEP_END: u64 = 0;
static mut STEP_LEFT: u64 = 0;
pub fn dbg_step_arm(base: u64, end: u64, n: u64) {
    unsafe { STEP_BASE = base; STEP_END = end; STEP_LEFT = n; STEP_ARMED = 0; }
}
/// Called from the HW-break handler at init entry: starts stepping.
/// Returns true (handler must SET TF in saved RFLAGS before IRET).
pub fn dbg_step_start() -> bool {
    unsafe {
        if STEP_BASE == 0 || STEP_LEFT == 0 { return false; }
        STEP_ARMED = 1;
        true
    }
}
pub fn dbg_step_disarm() {
    unsafe { STEP_ARMED = 0; STEP_BASE = 0; }
}
/// Called from the #DB handler. Returns true to keep TF set (continue
/// stepping), false to clear TF (done). Logs RIP + saved RDI per step.
/// Two-phase: ARMED=1 waits (RIP outside module, keep TF, no log);
/// ARMED=2 traces (RIP inside, log); leaving the module while tracing
/// disarms (the call into the kernel is the last traced step).
pub fn dbg_step_fire(frame: u64, rip: u64) -> bool {
    unsafe {
        if STEP_ARMED == 0 { return false; }
        if STEP_LEFT == 0 { STEP_ARMED = 0; return false; }
        let in_mod = rip >= STEP_BASE && rip < STEP_END;
        if !in_mod {
            if STEP_ARMED == 2 {
                // Trace into the kernel callee (find where RDI is lost).
                // Keep stepping until LEFT exhausts (no disarm here).
                let rdi = core::ptr::read_volatile(frame.wrapping_sub(320) as *const u64);
                let rax = core::ptr::read_volatile(frame.wrapping_sub(280) as *const u64);
                let left = STEP_LEFT;
                crate::println!("step OUT rip={:#x} rdi={:#x} rax={:#x} left={}", rip, rdi, rax, left);
                if STEP_LEFT == 0 { STEP_ARMED = 0; return false; }
                STEP_LEFT -= 1;
                return true;
            }
            STEP_LEFT -= 1;
            return true;
        }
        STEP_ARMED = 2;
        STEP_LEFT -= 1;
        let rdi = core::ptr::read_volatile(frame.wrapping_sub(320) as *const u64);
        let rax = core::ptr::read_volatile(frame.wrapping_sub(280) as *const u64);
        let left = STEP_LEFT;
        crate::println!("step rip={:#x} rdi={:#x} rax={:#x} left={}", rip, rdi, rax, left);
        if STEP_LEFT == 0 { STEP_ARMED = 0; return false; }
        true
    }
}

// DIAG-ABI: hardware breakpoint probe to verify post-movabs RDI.
static mut DBG_BP_HIT: u64 = 0;
static mut DBG_BP_RIP: u64 = 0;
static mut DBG_BP_RDI: u64 = 0;
static mut DBG_BP_ARMED: u64 = 0;

pub fn dbg_arm_hwbreak(va: u64) {
    unsafe {
        DBG_BP_HIT = 0;
        DBG_BP_ARMED = 1;
        core::arch::asm!("mov dr0, {0}", in(reg) va, options(nostack, preserves_flags));
        // DR7: L0=1 (local enable BP0), RW0=00 (exec), LEN0=00 (1 byte).
        core::arch::asm!("mov dr7, {0}", in(reg) 0x1u64, options(nostack, preserves_flags));
    }
}
pub fn dbg_disarm_hwbreak() {
    unsafe {
        DBG_BP_ARMED = 0;
        core::arch::asm!("mov dr7, {0}", in(reg) 0u64, options(nostack, preserves_flags));
    }
}
pub fn dbg_hwbreak_hit() -> (u64, u64) {
    unsafe { (DBG_BP_HIT, (DBG_BP_RIP, DBG_BP_RDI).0) }
}
#[allow(dead_code)]
pub fn dbg_hwbreak_vals() -> (u64, u64, u64) {
    unsafe { (DBG_BP_HIT, DBG_BP_RIP, DBG_BP_RDI) }
}
/// Called from the #DB handler when DR6.BS0 is set and a probe is armed.
/// `frame` is the IRET-frame base (RIP slot); saved RDI is 320 bytes below it.
/// If single-step tracing was armed (STEP_BASE set), starts it and returns
/// true so the handler sets TF (steps from init entry). Else false (clear TF).
pub fn dbg_hwbreak_fire(frame: u64) -> bool {
    unsafe {
        if DBG_BP_ARMED == 0 { return false; }
        // Read DR6 to confirm HW break (BS0). We clear it by rewriting.
        let dr6: u64;
        core::arch::asm!("mov {}, dr6", out(reg) dr6, options(nostack, preserves_flags));
        if dr6 & 1 == 0 { return false; }
        // Saved RDI lives 320 bytes below the RIP slot (entry pushes 15 GPRs).
        let rdi = core::ptr::read_volatile((frame.wrapping_sub(320)) as *const u64);
        let rip = core::ptr::read_volatile(frame as *const u64);
        DBG_BP_HIT = 1;
        DBG_BP_RIP = rip;
        DBG_BP_RDI = rdi;
        // Acknowledge + disarm so IRET re-executes once, cleanly.
        core::arch::asm!("mov dr6, {0}", in(reg) 0xffff0ff0u64, options(nostack, preserves_flags));
        core::arch::asm!("mov dr7, {0}", in(reg) 0u64, options(nostack, preserves_flags));
        DBG_BP_ARMED = 0;
        // Start single-step tracing from init entry (handler sets TF).
        dbg_step_start()
    }
}
