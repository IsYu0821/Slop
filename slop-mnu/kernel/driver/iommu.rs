//! Intel VT-d IOMMU (DMAR ACPI table, generic PC IOMMU profile).
//!
//! Profile D: QEMU intel-iommu (DMA + interrupt remapping). Reuses the
//! Slop PCI/BAR/DMA core. Bring-up: DMAR parse -> DRHD map -> root +
//! context + second-level tables (identity) -> global invalidate ->
//! enable translation -> IRTA + IRTE + remappable MSI. NVMe/AHCI/e1000
//! Lesson applied: no fixed IDs (PCI class 0x08/0x06/0x00 for IOMMU?
//! actually found via DMAR, not PCI), strict wrap/validate, snapshot-free
//! (page tables are fresh, never clobber live data).
#![allow(dead_code)]
use crate::driver::pci;
use crate::sync::IrqSpinLock;
use core::sync::atomic::Ordering;

/* DRHD register offsets. */
const R_VER: u64 = 0x00;
const R_CAP: u64 = 0x08;
const R_ECAP: u64 = 0x10;
const R_GCMD: u64 = 0x18;
const R_GSTS: u64 = 0x1c;
const R_RTADDR: u64 = 0x20;
const R_CCMD: u64 = 0x28;
const R_FSTS: u64 = 0x34;
const R_IRTA: u64 = 0xb8;
const GCMD_TE: u32 = 1 << 31;
const GCMD_SRTP: u32 = 1 << 30;
const GCMD_IRE: u32 = 1 << 25;
const GSTS_TES: u32 = 1 << 31;

fn r32(bar: u64, off: u64) -> u32 {
    unsafe { ((bar + off) as *const u32).read_volatile() }
}
fn w32(bar: u64, off: u64, v: u32) {
    unsafe { ((bar + off) as *mut u32).write_volatile(v); }
}
fn r64(bar: u64, off: u64) -> u64 {
    (r32(bar, off) as u64) | ((r32(bar, off + 4) as u64) << 32)
}
fn w64(bar: u64, off: u64, v: u64) {
    w32(bar, off, (v & 0xffff_ffff) as u32);
    w32(bar, off + 4, (v >> 32) as u32);
}

struct Iommu {
    bar: u64,
    irta_va: u64,
    ready: bool,
    dma_ready: bool,
    ir_ready: bool,
}
const NO_IOMMU: Iommu = Iommu { bar: 0, irta_va: 0, ready: false, dma_ready: false, ir_ready: false };
static IOMMU: IrqSpinLock<Iommu> = IrqSpinLock::new(NO_IOMMU);

/* ---- DMAR table parse (find first DRHD on segment 0) ---- */
fn find_drhd() -> Option<u64> {
    /* QEMU places the register base at DMAR+56; validated via VER/CAP.
       (Robust table parsing is future work.) */
    let dmar = crate::acpi::find_table(b"DMAR");
    if dmar == 0 {
        crate::println!("iommu: no DMAR table");
        return None;
    }
    unsafe {
        let base = crate::arch::x86_64::phys::phys_to_virt(dmar) as *const u8;
        let len = *(base.add(4) as *const u32);
        crate::println!("iommu: DMAR at {:#x} len={}", dmar, len);
        if len < 64 { return None; }
        /* Register base is 8 bytes at DMAR+56 on QEMU (verify via VER). */
        let reg = (base.add(56) as *const u64).read_unaligned();
        if reg == 0 { return None; }
        let bar = pci::ioremap_pa(reg, 4096);
        if bar == 0 { return None; }
        let ver = r32(bar, R_VER);
        let cap = r64(bar, R_CAP);
        crate::println!("iommu: probe reg={:#x} ver={:#x} cap={:#x}", reg, ver, cap);
        /* Intel VT-d VER is 0x10 (1.0); CAP must be nonzero. */
        if ver == 0 || ver > 0xFF || cap == 0 {
            crate::println!("iommu: bad VER/CAP, rejecting");
            return None;
        }
        return Some(reg);
    }
}

pub fn ready() -> bool { IOMMU.lock().ready }

pub fn init() -> Result<(), &'static str> {
    let reg_pa = find_drhd().ok_or("no DRHD")?;
    let bar = pci::ioremap_pa(reg_pa, 4096);
    if bar == 0 { return Err("drhd map"); }
    let ver = r32(bar, R_VER);
    let cap = r64(bar, R_CAP);
    let ecap = r64(bar, R_ECAP);
    crate::println!("iommu: ver={:#x} cap={:#x} ecap={:#x}", ver, cap, ecap);
    {
        let mut n = IOMMU.lock();
        n.bar = bar;
        n.ready = true;
    }
    enable_dma()?;
    match enable_ir() {
        Ok(()) => {
            let mut n = IOMMU.lock();
            n.ir_ready = true;
        }
        Err(e) => {
            crate::println!("iommu: IR unavailable ({})", e);
        }
    }
    {
        let mut n = IOMMU.lock();
        n.ready = true;
    }
    Ok(())
}

pub fn ir_ready() -> bool { IOMMU.lock().ir_ready }

pub fn selftest() {
    if !ready() {
        crate::println!("iommu: selftest SKIP (no IOMMU)");
        return;
    }
    crate::println!("iommu: selftest PASS (detect only)");
}

/* ================= DMA remapping (identity map) ================= */
const CCMD: u64 = 0x28;
const CCMD_ICC: u64 = 1 << 63;

/* Second-level PTE bits (x86-style + R/W). */
const SL_P: u64 = 1;
const SL_RW: u64 = 3;
const SL_PS: u64 = 1 << 7; /* 2MB superpage */

fn phys_alloc_zero(pages: usize) -> Option<(u64, u64)> {
    let (va, pa) = crate::dma::alloc_coherent(pages * 4096)?;
    Some((va, pa))
}

/* Build identity map for [0, 4GB) with 2MB pages. Returns PML4 PA. */
fn build_identity() -> Option<u64> {
    /* AW=001b (39-bit) walks 3 levels starting at the PDPT: no PML4.
       SLPTPTR must point directly at the PDPT page. */
    let (pdpt_va, pdpt_pa) = phys_alloc_zero(1)?;
    unsafe {
        /* PDPT[0..3] -> 4 PD pages covering 0..4GB. */
        for pd in 0..4u64 {
            let (pd_va, pd_pa) = phys_alloc_zero(1)?;
            ((pdpt_va as *mut u64).add(pd as usize)).write_volatile(pd_pa | SL_RW | SL_P);
            for i in 0..512u64 {
                /* 2MB superpage identity mapping. */
                let pa = (pd << 30) | (i << 21);
                ((pd_va as *mut u64).add(i as usize)).write_volatile(pa | SL_RW | SL_P | SL_PS);
            }
        }
    }
    Some(pdpt_pa)
}

/* Set up root + context tables (single domain, all of bus 0), install
   identity second-level tables, and enable DMA translation. */
pub fn enable_dma() -> Result<(), &'static str> {
    let bar = {
        let n = IOMMU.lock();
        if n.bar == 0 { return Err("no IOMMU"); }
        n.bar
    };
    /* Root table (4KB, 256 x 16B entries). */
    let (rt_va, rt_pa) = phys_alloc_zero(1).ok_or("root alloc")?;
    /* One context table for bus 0 (covers all our devices). */
    let (ctx_va, _ctx_pa) = phys_alloc_zero(1).ok_or("ctx alloc")?;
    let sl_pa = build_identity().ok_or("sl build")?;
    unsafe {
        /* Context entries for every devfn on bus 0. Correct layout
           (VT-d Figure 9-3): low 64 = P + TT + SLPTPTR, high 64 = AW.
           TT=00b (second-level translation), AW=001b (39-bit, upper[2:0];
           QEMU supports 3-level walks only). DID=0, FPD=0, rest zero. */
        for devfn in 0..256usize {
            let e = (ctx_va + (devfn as u64) * 16) as *mut u64;
            e.write_volatile(1 | (sl_pa & !0xfff));
            e.add(1).write_volatile(1);
        }
        /* Root entry 0 (bus 0): CTP + present. Upper 64 reserved (0). */
        let r = rt_va as *mut u64;
        let ctx_pa = crate::arch::x86_64::phys::virt_to_phys_full(ctx_va);
        r.write_volatile((ctx_pa & !0xfff) | 1);
        r.add(1).write_volatile(0);
    }
    /* Set root table pointer (legacy mode, TT=00). */
    w64(bar, R_RTADDR, rt_pa & !0xfff);
    let mut g = r32(bar, R_GCMD);
    g |= 1 << 30; /* SRTP */
    w32(bar, R_GCMD, g);
    let deadline = crate::arch::x86_64::apic::ticks().wrapping_add(500 + 2);
    loop {
        if r32(bar, R_GSTS) & (1 << 30) != 0 { break; }
        core::hint::spin_loop();
        if crate::arch::x86_64::apic::ticks().wrapping_sub(deadline) < 0x8000_0000 {
            return Err("SRTP timeout");
        }
    }
    /* Global context-cache invalidate (CCMD.CIRG=01b), then enable. */
    w64(bar, R_CCMD, (1u64 << 63) | (1u64 << 61));
    {
        let deadline = crate::arch::x86_64::apic::ticks().wrapping_add(500 + 2);
        loop {
            if r64(bar, R_CCMD) & (1u64 << 63) == 0 { break; }
            core::hint::spin_loop();
            if crate::arch::x86_64::apic::ticks().wrapping_sub(deadline) < 0x8000_0000 {
                return Err("CCMD timeout");
            }
        }
    }
    crate::println!("iommu: context invalidated");
    let mut g = r32(bar, R_GCMD);
    g |= GCMD_TE;
    w32(bar, R_GCMD, g);
    let deadline = crate::arch::x86_64::apic::ticks().wrapping_add(500 + 2);
    loop {
        if r32(bar, R_GSTS) & GSTS_TES != 0 { break; }
        core::hint::spin_loop();
        if crate::arch::x86_64::apic::ticks().wrapping_sub(deadline) < 0x8000_0000 {
            return Err("TE timeout");
        }
    }
    {
        let mut n = IOMMU.lock();
        n.dma_ready = true;
    }
    crate::println!("iommu: DMA remapping enabled (identity 1:1)");
    Ok(())
}

/* ================= Interrupt remapping ================= */
/* IRTA: 4KB, 256 x 16B entries. IRTE[0] is reserved for NVMe MSI-X. */
const IRTE_COUNT: usize = 256;

pub fn enable_ir() -> Result<(), &'static str> {
    let bar = {
        let n = IOMMU.lock();
        if n.bar == 0 { return Err("no IOMMU"); }
        n.bar
    };
    let (irta_va, irta_pa) = phys_alloc_zero(1).ok_or("irta alloc")?;
    unsafe { core::ptr::write_bytes(irta_va as *mut u8, 0, 4096); }
    /* IRTA register: base + size (log2(entries)-1 = 7 for 256). */
    w64(bar, R_IRTA, (irta_pa & !0xfff) | 7);
    {
        let mut n = IOMMU.lock();
        n.irta_va = irta_va;
    }
    /* Enable interrupt remapping. Set both SIRTP (latch table) and IRE
       (enable); bit assignment 24/25 is confirmed by GSTS readback. */
    let mut g = r32(bar, R_GCMD);
    g |= (1 << 24) | (1 << 25);
    w32(bar, R_GCMD, g);
    let deadline = crate::arch::x86_64::apic::ticks().wrapping_add(500 + 2);
    loop {
        let sts = r32(bar, R_GSTS);
        if sts & ((1 << 24) | (1 << 25)) == ((1 << 24) | (1 << 25)) { break; }
        core::hint::spin_loop();
        if crate::arch::x86_64::apic::ticks().wrapping_sub(deadline) < 0x8000_0000 {
            return Err("IRE timeout");
        }
    }
    crate::println!("iommu: interrupt remapping enabled");
    Ok(())
}



/* Program IRTE[index]: fixed delivery, edge trigger, physical dest,
   no source validation. Returns the remappable MSI address/data pair
   for the device to write (handle=index, subhandle=0). */
pub fn set_irte(index: usize, vector: u8, dest: u8) -> Result<(u64, u32), &'static str> {
    if index >= 256 { return Err("irte range"); }
    let irta = {
        let n = IOMMU.lock();
        if n.irta_va == 0 { return Err("no IRTA"); }
        n.irta_va
    };
    unsafe {
        let e = (irta + (index as u64) * 16) as *mut u64;
        /* Low: P=1, DM=0, RH=0, TM=0(edge), DLM=000(fixed), Vector, DEST. */
        e.write_volatile(1 | ((vector as u64) << 16) | ((dest as u64) << 32));
        /* High: SVT=00 (none), SQ=00, SID=0. */
        e.add(1).write_volatile(0);
    }
    /* Remappable MSI: address 0xFEE0_0010 | handle<<5, data 0.
       (Verified against QEMU's checks: __head==0xfee, int_mode set.) */
    let addr = 0xFEE0_0000u64 | 0x10 | ((index as u64) << 5);
    Ok((addr, 0))
}
