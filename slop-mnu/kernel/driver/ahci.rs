//! AHCI/SATA block driver (PCI class 0x01/0x06/0x01, no fixed IDs).
//!
//! Generic-PC storage path #2 (after NVMe): QEMU ich9-ahci + SATA disk,
//! no virtio. Reuses the Slop PCI/BAR/DMA/IRQ core; no AHCI-only
//! infrastructure. Single port (first with a device), single outstanding
//! command (serialized), polling for completion (interrupts masked at the
//! HBA/port level, stated explicitly).
//!
//! Bring-up order: class probe -> ABAR (BAR5) map -> HBA reset + AE ->
//! port enumerate (PI + SSTS.DET + SIG) -> port stop -> CLB/FB setup ->
//! port start -> IDENTIFY -> READ/WRITE/FLUSH. Queue/ring discipline
//! (NVMe lesson): doorbell/CI bits stay strictly below the slot count,
//! and the selftest never touches live filesystem sectors (snapshot +
//! restore + verify).
use crate::driver::pci::{self, Bdf, Cam, ConfigAccess, CFG_CLASS, CFG_COMMAND, CFG_VENDOR, CMD_BUS_MASTER, CMD_MEM};
use crate::sync::IrqSpinLock;
use core::sync::atomic::{AtomicU32, Ordering};

const SECTOR: usize = 512;
const MAX_SLOTS: usize = 32;

/* HBA generic host control (ABAR+off). */
const R_CAP: u64 = 0x00;
const R_GHC: u64 = 0x04;
const GHC_AE: u32 = 1 << 31;
const GHC_HR: u32 = 1;
const R_PI: u64 = 0x0c;
/* Port registers (ABAR + 0x100 + port*0x80). */
const PORT_BASE: u64 = 0x100;
const PORT_STRIDE: u64 = 0x80;
const R_P_CLB: u64 = 0x00;
const R_P_CLBU: u64 = 0x04;
const R_P_FB: u64 = 0x08;
const R_P_FBU: u64 = 0x0c;
const R_P_IS: u64 = 0x10;
const R_P_CMD: u64 = 0x18;
const CMD_ST: u32 = 1 << 0;
const CMD_SUD: u32 = 1 << 1;
const CMD_POD: u32 = 1 << 2;
const CMD_FRE: u32 = 1 << 4;
const CMD_FR: u32 = 1 << 14;
const CMD_CR: u32 = 1 << 15;
const R_P_TFD: u64 = 0x20;
const TFD_BSY: u32 = 1 << 7;
const TFD_ERR: u32 = 1;
const R_P_SIG: u64 = 0x24;
const SIG_ATA: u32 = 0x0000_0101;
const R_P_SSTS: u64 = 0x28;
const R_P_SCTL: u64 = 0x2c;
const R_P_SERR: u64 = 0x30;
const R_P_CI: u64 = 0x38;
/* ATA commands. */
const ATA_IDENTIFY: u8 = 0xec;
const ATA_READ_DMA_EXT: u8 = 0x25;
const ATA_WRITE_DMA_EXT: u8 = 0x35;
const ATA_FLUSH_EXT: u8 = 0xea;

fn r32(bar: u64, off: u64) -> u32 {
    unsafe { ((bar + off) as *const u32).read_volatile() }
}
fn w32(bar: u64, off: u64, v: u32) {
    unsafe { ((bar + off) as *mut u32).write_volatile(v); }
}

struct Port {
    bdf: Bdf,
    bar: u64,
    port: u32,
    clb_va: u64,
    fb_va: u64,
    ct_va: u64,
    sectors: u64,
    ready: bool,
}
const NO_PORT: Port = Port { bdf: Bdf { segment: 0, bus: 0, device: 0, function: 0 }, bar: 0, port: 0, clb_va: 0, fb_va: 0, ct_va: 0, sectors: 0, ready: false };
static PORT: IrqSpinLock<Port> = IrqSpinLock::new(NO_PORT);
static CMD_LOCK: IrqSpinLock<()> = IrqSpinLock::new(());
static IRQ_FIRED: AtomicU32 = AtomicU32::new(0);

fn pregs(port: u32) -> u64 { PORT_BASE + (port as u64) * PORT_STRIDE }

fn find_ahci() -> Option<Bdf> {
    let acc = Cam;
    for bus in 0..=255u8 {
        for dev in 0..32u8 {
            for fun in 0..8u8 {
                let bdf = Bdf { segment: 0, bus, device: dev, function: fun };
                if acc.read32(bdf, CFG_VENDOR) & 0xffff == 0xffff { continue; }
                let class = acc.read32(bdf, CFG_CLASS);
                if (class >> 24) as u8 == 0x01 && ((class >> 16) & 0xff) as u8 == 0x06 && ((class >> 8) & 0xff) as u8 == 0x01 {
                    if fun > 0 { continue; }
                    return Some(bdf);
                }
            }
        }
    }
    None
}

/* Wait for (reg & mask) == want, tick-based deadline. */
fn wait_reg(bar: u64, off: u64, mask: u32, want: u32, timeout_ms: u64) -> Result<(), &'static str> {
    let deadline = crate::arch::x86_64::apic::ticks().wrapping_add(timeout_ms / 4 + 2);
    loop {
        if r32(bar, off) & mask == want { return Ok(()); }
        core::hint::spin_loop();
        if crate::arch::x86_64::apic::ticks().wrapping_sub(deadline) < 0x8000_0000 {
            return Err("ahci timeout");
        }
    }
}

/* Stop a port's DMA + FIS engines (spec order: ST then FRE). */
fn port_stop(bar: u64, port: u32) -> Result<(), &'static str> {
    let p = pregs(port);
    let mut cmd = r32(bar, p + R_P_CMD);
    if cmd & CMD_ST != 0 {
        cmd &= !CMD_ST;
        w32(bar, p + R_P_CMD, cmd);
        wait_reg(bar, p + R_P_CMD, CMD_CR, 0, 2000).map_err(|_| "stop CR")?;
    }
    let mut cmd = r32(bar, p + R_P_CMD);
    if cmd & CMD_FRE != 0 {
        cmd &= !CMD_FRE;
        w32(bar, p + R_P_CMD, cmd);
        wait_reg(bar, p + R_P_CMD, CMD_FR, 0, 2000).map_err(|_| "stop FR")?;
    }
    Ok(())
}

fn port_start(bar: u64, port: u32) -> Result<(), &'static str> {
    let p = pregs(port);
    /* Clear latched errors first (write-1-to-clear). */
    w32(bar, p + R_P_SERR, 0xffff_ffff);
    w32(bar, p + R_P_IS, 0xffff_ffff);
    let mut cmd = r32(bar, p + R_P_CMD);
    cmd |= CMD_SUD | CMD_POD | CMD_FRE;
    w32(bar, p + R_P_CMD, cmd);
    let mut cmd = r32(bar, p + R_P_CMD);
    cmd |= CMD_ST;
    w32(bar, p + R_P_CMD, cmd);
    Ok(())
}

/* COMRESET a port (SCTL.DET=1, wait, DET=0, wait for PHY ready). */
fn port_reset(bar: u64, port: u32) -> Result<(), &'static str> {
    let p = pregs(port);
    port_stop(bar, port)?;
    w32(bar, p + R_P_SCTL, (r32(bar, p + R_P_SCTL) & !0xf) | 0x1);
    /* ~1ms+ of COMRESET (busy wait, IRQs may be off this early). */
    for _ in 0..1_000_000 { core::hint::spin_loop(); }
    w32(bar, p + R_P_SCTL, r32(bar, p + R_P_SCTL) & !0xf);
    wait_reg(bar, p + R_P_SSTS, 0xf, 0x3, 5000).map_err(|_| "phy ready")?;
    w32(bar, p + R_P_SERR, 0xffff_ffff);
    w32(bar, p + R_P_IS, 0xffff_ffff);
    port_start(bar, port)
}

/* Build + issue one command on `slot`; poll CI until the device clears it.
   `fis` is the 20-byte Register H2D FIS; data moves via PRDT (single
   entry, caller guarantees the buffer fits one page). */
fn xfer_slot(slot: usize, fis: &[u8; 20], data_pa: u64, data_len: usize, write: bool, timeout_ms: u64) -> Result<(), i64> {
    let (bar, port, clb_va, ct_va) = {
        let p = PORT.lock();
        if !p.ready { return Err(crate::errno::ENODEV); }
        (p.bar, p.port, p.clb_va, p.ct_va)
    };
    let _g = CMD_LOCK.lock();
    let pp = pregs(port);
    /* Find a free slot (single outstanding in practice, but honor CI). */
    let mut s = slot;
    if r32(bar, pp + R_P_CI) & (1u32 << s) != 0 {
        let mut found = None;
        for i in 0..MAX_SLOTS {
            if r32(bar, pp + R_P_CI) & (1u32 << i) == 0 { found = Some(i); break; }
        }
        s = found.ok_or(crate::errno::EBUSY)?;
    }
    /* Command header (32B). */
    let hdr = clb_va + (s as u64) * 32;
    unsafe {
        let h = hdr as *mut u32;
        let cfl = 5u32; /* 20-byte FIS = 5 dwords */
        let prdtl = 1u32;
        h.write_volatile(cfl | ((write as u32) << 6) | (prdtl << 16));
        h.add(1).write_volatile(0); /* PRDBC */
        let ct_pa = crate::arch::x86_64::phys::virt_to_phys_full(ct_va + (s as u64) * 512);
        if ct_pa == 0 { return Err(crate::errno::EFAULT); }
        h.add(2).write_volatile((ct_pa & 0xffff_ffff) as u32);
        h.add(3).write_volatile((ct_pa >> 32) as u32);
        for i in 4..8 { h.add(i).write_volatile(0); }
        /* Command table: CFIS + PRDT. */
        let ct = (ct_va + (s as u64) * 512) as *mut u8;
        core::ptr::write_bytes(ct, 0, 128);
        core::ptr::copy_nonoverlapping(fis.as_ptr(), ct, 20);
        let prd = ct.add(128) as *mut u32;
        prd.write_volatile((data_pa & 0xffff_ffff) as u32);
        prd.add(1).write_volatile((data_pa >> 32) as u32);
        prd.add(2).write_volatile(0);
        let dbc = (data_len as u32).wrapping_sub(1) & 0x3f_ffff;
        prd.add(3).write_volatile(dbc);
    }
    core::sync::atomic::compiler_fence(Ordering::SeqCst);
    w32(bar, pp + R_P_CI, r32(bar, pp + R_P_CI) | (1u32 << s));
    let deadline = crate::arch::x86_64::apic::ticks().wrapping_add(timeout_ms / 4 + 2);
    let mut waited = 0u64;
    loop {
        if r32(bar, pp + R_P_CI) & (1u32 << s) == 0 { break; }
        core::hint::spin_loop();
        waited += 1;
        if waited % 100 == 99 {
            unsafe { core::arch::asm!("sti", options(nomem, nostack, preserves_flags)) };
            unsafe { core::arch::asm!("hlt", options(nomem, nostack, preserves_flags)) };
            unsafe { core::arch::asm!("cli", options(nomem, nostack, preserves_flags)) };
        }
        if crate::arch::x86_64::apic::ticks().wrapping_sub(deadline) < 0x8000_0000 {
            crate::println!("ahci: poll timeout port={} slot={}", port, s);
            return Err(crate::errno::ETIMEDOUT);
        }
    }
    /* Check for device errors. */
    let tfd = r32(bar, pp + R_P_TFD);
    if tfd & (TFD_ERR | TFD_BSY) != 0 {
        crate::println!("ahci: tfd error {:#x} port={} slot={}", tfd, port, s);
        w32(bar, pp + R_P_IS, 0xffff_ffff);
        return Err(crate::errno::EIO);
    }
    if r32(bar, pp + R_P_IS) & (1u32 << 30) != 0 {
        crate::println!("ahci: TFES port={} slot={}", port, s);
        w32(bar, pp + R_P_IS, 0xffff_ffff);
        return Err(crate::errno::EIO);
    }
    w32(bar, pp + R_P_IS, 0xffff_ffff);
    Ok(())
}

/* Register H2D FIS for an LBA48 DMA command (ata_tf_to_fis order). */
fn reg_fis(cmd: u8, lba: u64, count: u16) -> [u8; 20] {
    let mut f = [0u8; 20];
    f[0] = 0x27; /* Register H2D */
    f[1] = 0x80; /* C = command */
    f[2] = cmd;
    f[3] = 0; /* features 7:0 */
    f[4] = lba as u8;
    f[5] = (lba >> 8) as u8;
    f[6] = (lba >> 16) as u8;
    f[7] = 0xe0; /* device: LBA mode, master */
    f[8] = (lba >> 24) as u8;
    f[9] = (lba >> 32) as u8;
    f[10] = (lba >> 40) as u8;
    f[11] = 0; /* features 15:8 */
    f[12] = count as u8;
    f[13] = (count >> 8) as u8;
    f[14] = 0;
    f[15] = 0x08; /* control: ATA_HOB (use LBA48 registers) */
    f
}

/* 512B sector read/write through a coherent scratch page (handles any
   caller alignment, including page-crossing buffers). */
fn xfer_lba(lba: u64, va: u64, is_write: bool, timeout_ms: u64) -> Result<(), i64> {
    let opcode = if is_write { ATA_WRITE_DMA_EXT } else { ATA_READ_DMA_EXT };
    let fis = reg_fis(opcode, lba, 1);
    if is_write {
        let (sva, spa) = crate::dma::alloc_coherent(4096).ok_or(crate::errno::ENOMEM)?;
        unsafe { core::ptr::copy_nonoverlapping(va as *const u8, sva as *mut u8, SECTOR) };
        let r = xfer_slot(0, &fis, spa, SECTOR, true, timeout_ms);
        crate::dma::free_coherent(sva, spa, 4096);
        r
    } else {
        let (sva, spa) = crate::dma::alloc_coherent(4096).ok_or(crate::errno::ENOMEM)?;
        let r = xfer_slot(0, &fis, spa, SECTOR, false, timeout_ms);
        if r.is_ok() {
            unsafe { core::ptr::copy_nonoverlapping(sva as *const u8, va as *mut u8, SECTOR) };
        }
        crate::dma::free_coherent(sva, spa, 4096);
        r
    }
}

pub fn ready() -> bool { PORT.lock().ready }
pub fn capacity() -> u64 { PORT.lock().sectors }

pub fn read_sector(sector: u64, dst: &mut [u8]) -> Result<(), i64> {
    if dst.len() < SECTOR { return Err(crate::errno::EINVAL); }
    let (lba, off) = (sector, 0usize);
    let _ = off;
    xfer_lba(lba, dst.as_ptr() as u64, false, 10000)?;
    Ok(())
}

pub fn write_sector(sector: u64, src: &[u8]) -> Result<(), i64> {
    if src.len() < SECTOR { return Err(crate::errno::EINVAL); }
    xfer_lba(sector, src.as_ptr() as u64, true, 10000)?;
    Ok(())
}

pub fn flush() -> Result<(), i64> {
    let fis = reg_fis(ATA_FLUSH_EXT, 0, 0);
    let (sva, spa) = crate::dma::alloc_coherent(4096).ok_or(crate::errno::ENOMEM)?;
    let r = xfer_slot(0, &fis, spa, 0, false, 10000);
    crate::dma::free_coherent(sva, spa, 4096);
    r
}

/* Full port reset + re-setup (allocation-neutral). */
pub fn recover() -> Result<(), &'static str> {
    let (bar, port) = {
        let p = PORT.lock();
        if p.bar == 0 { return Err("not ready"); }
        (p.bar, p.port)
    };
    port_reset(bar, port).map_err(|_| "port reset")?;
    {
        let mut p = PORT.lock();
        p.ready = true;
    }
    Ok(())
}

fn identify() -> Result<u64, &'static str> {
    let fis = reg_fis(ATA_IDENTIFY, 0, 0);
    let (sva, spa) = crate::dma::alloc_coherent(4096).ok_or("identify alloc")?;
    let r = xfer_slot(0, &fis, spa, SECTOR, false, 10000);
    let sectors = if r.is_ok() {
        unsafe {
            let b = sva as *const u8;
            /* Word 83 bit 10: LBA48 support. */
            let w83 = u16::from_le_bytes([*b.add(166), *b.add(167)]);
            if w83 & (1 << 10) != 0 {
                /* Words 100-103 (bytes 200-207): 48-bit sector count. */
                let lo = u32::from_le_bytes([*b.add(200), *b.add(201), *b.add(202), *b.add(203)]);
                let hi = u32::from_le_bytes([*b.add(204), *b.add(205), *b.add(206), *b.add(207)]);
                ((hi as u64) << 32) | (lo as u64)
            } else {
                /* Words 60-61 (bytes 120-123): 28-bit sector count. */
                u32::from_le_bytes([*b.add(120), *b.add(121), *b.add(122), *b.add(123)]) as u64
            }
        }
    } else { 0 };
    crate::dma::free_coherent(sva, spa, 4096);
    r.map_err(|_| "identify")?;
    if sectors == 0 { return Err("empty device"); }
    Ok(sectors)
}

/* Bring up the first implemented port with a SATA ATA device. */
fn setup_port(bdf: Bdf, bar: u64) -> Result<(u32, u64), &'static str> {
    let pi = r32(bar, R_PI);
    crate::println!("ahci: {:02x}:{:02x}.{} PI={:#x}", bdf.bus, bdf.device, bdf.function, pi);
    for port in 0..32u32 {
        if pi & (1u32 << port) == 0 { continue; }
        let p = pregs(port);
        let mut det = r32(bar, p + R_P_SSTS) & 0xf;
        let mut sig = r32(bar, p + R_P_SIG);
        if det != 0x3 || sig != SIG_ATA {
            /* PHY up but no signature yet (or stale state): COMRESET and
               re-probe. Some models only latch SIG after a reset. */
            crate::println!("ahci: port{} det={:#x} sig={:#x}, resetting", port, det, sig);
            if port_reset(bar, port).is_err() { continue; }
            det = r32(bar, p + R_P_SSTS) & 0xf;
            sig = r32(bar, p + R_P_SIG);
            crate::println!("ahci: port{} after reset det={:#x} sig={:#x}", port, det, sig);
            if det != 0x3 || sig != SIG_ATA { continue; }
        } else {
            crate::println!("ahci: port{} det=3 sig={:#x}", port, sig);
        }
        /* Stop, set up CLB (1K) + FB (256B) + command tables. */
        port_stop(bar, port).map_err(|_| "port stop")?;
        let (clb_va, clb_pa) = crate::dma::alloc_coherent(4096).ok_or("clb alloc")?;
        let (fb_va, fb_pa) = crate::dma::alloc_coherent(4096).ok_or("fb alloc")?;
        let (ct_va, ct_pa) = crate::dma::alloc_coherent(4096 * 8).ok_or("ct alloc")?;
        let _ = ct_pa;
        unsafe {
            core::ptr::write_bytes(clb_va as *mut u8, 0, 1024);
            core::ptr::write_bytes(fb_va as *mut u8, 0, 256);
        }
        w32(bar, p + R_P_CLB, (crate::arch::x86_64::phys::virt_to_phys_full(clb_va) & 0xffff_ffff) as u32);
        w32(bar, p + R_P_CLBU, (crate::arch::x86_64::phys::virt_to_phys_full(clb_va) >> 32) as u32);
        w32(bar, p + R_P_FB, (crate::arch::x86_64::phys::virt_to_phys_full(fb_va) & 0xffff_ffff) as u32);
        w32(bar, p + R_P_FBU, (crate::arch::x86_64::phys::virt_to_phys_full(fb_va) >> 32) as u32);
        {
            let mut q = PORT.lock();
            q.bdf = bdf;
            q.bar = bar;
            q.port = port;
            q.clb_va = clb_va;
            q.fb_va = fb_va;
            q.ct_va = ct_va;
        }
        port_start(bar, port).map_err(|_| "port start")?;
        /* Mark usable before IDENTIFY: transfers check ready(). */
        {
            let mut q = PORT.lock();
            q.ready = true;
        }
        let sectors = match identify() {
            Ok(v) => v,
            Err(e) => {
                let mut q = PORT.lock();
                q.ready = false;
                q.bar = 0;
                crate::dma::free_coherent(clb_va, crate::arch::x86_64::phys::virt_to_phys_full(clb_va), 4096);
                crate::dma::free_coherent(fb_va, crate::arch::x86_64::phys::virt_to_phys_full(fb_va), 4096);
                crate::dma::free_coherent(ct_va, crate::arch::x86_64::phys::virt_to_phys_full(ct_va), 4096 * 8);
                return Err(e);
            }
        };
        /* Logical sector size: require 512 (QEMU provides it). */
        crate::println!("ahci: port{} ready sectors={}", port, sectors);
        {
            let mut q = PORT.lock();
            q.sectors = sectors;
        }
        return Ok((port, sectors));
    }
    Err("no SATA device")
}

pub fn init() -> Result<u64, &'static str> {
    let bdf = find_ahci().ok_or("ahci not found")?;
    {
        let acc = Cam;
        let cmd = acc.read32(bdf, CFG_COMMAND);
        acc.write32(bdf, CFG_COMMAND, cmd | CMD_MEM as u32 | CMD_BUS_MASTER as u32);
    }
    let bar = pci::ioremap_bar(bdf, 5);
    if bar == 0 { return Err("abar map"); }
    /* HBA reset, then enable AHCI mode. */
    let mut ghc = r32(bar, R_GHC);
    ghc |= GHC_HR;
    w32(bar, R_GHC, ghc);
    wait_reg(bar, R_GHC, GHC_HR, 0, 5000).map_err(|_| "hba reset")?;
    let mut ghc = r32(bar, R_GHC);
    ghc |= GHC_AE;
    w32(bar, R_GHC, ghc);
    crate::println!("ahci: {:02x}:{:02x}.{} cap={:#x} abar={:#x}", bdf.bus, bdf.device, bdf.function, r32(bar, R_CAP), bar);
    let (_port, sectors) = setup_port(bdf, bar)?;
    Ok(sectors)
}

/* ---- boot selftest (skips cleanly without AHCI hardware) ---- */
fn fill_pattern(buf: &mut [u8], seed: u64) {
    for (i, b) in buf.iter_mut().enumerate() {
        *b = ((i as u64).wrapping_mul(0x11).wrapping_add(seed) & 0xff) as u8;
    }
}

pub fn selftest() {
    if !ready() {
        crate::println!("ahci: selftest SKIP (no controller)");
        return;
    }
    let cap = capacity();
    crate::println!("ahci: selftest start sectors={}", cap);
    if cap < 16 {
        crate::println!("ahci: selftest FAIL tiny device");
        return;
    }
    let (free_base, _) = crate::memory::frame_stats();
    let mut wbuf = [0u8; 512];
    let mut rbuf = [0u8; 512];
    /* Snapshot high sectors first: never touch live filesystem areas. */
    let tsecs = [cap - 8, cap - 7, cap - 6, cap - 5, cap - 1];
    let (snap_va, snap_pa) = match crate::dma::alloc_coherent(4096) {
        Some(v) => v,
        None => {
            crate::println!("ahci: selftest FAIL snapshot alloc");
            return;
        }
    };
    let mut snap = unsafe { core::slice::from_raw_parts_mut(snap_va as *mut u8, 3072) };
    for (i, &sec) in tsecs.iter().enumerate() {
        if read_sector(sec, &mut snap[i * 512..(i + 1) * 512]).is_err() {
            crate::println!("ahci: selftest FAIL snapshot sec={}", sec);
            crate::dma::free_coherent(snap_va, snap_pa, 4096);
            return;
        }
    }
    /* Write/read-back across several LBAs, incl. the last sector. */
    for (i, &lba_sec) in tsecs.iter().enumerate() {
        fill_pattern(&mut wbuf, 0xC0 + i as u64);
        for b in rbuf.iter_mut() { *b = 0; }
        if write_sector(lba_sec, &wbuf).is_err() {
            crate::println!("ahci: selftest FAIL write sec={}", lba_sec);
            return;
        }
        if read_sector(lba_sec, &mut rbuf).is_err() {
            crate::println!("ahci: selftest FAIL read sec={}", lba_sec);
            return;
        }
        if wbuf != rbuf {
            crate::println!("ahci: selftest FAIL mismatch sec={}", lba_sec);
            return;
        }
    }
    /* Flush works. */
    if flush().is_err() {
        crate::println!("ahci: selftest FAIL flush");
        return;
    }
    /* Reset recovery on a live controller; data must survive. */
    {
        let (free_pre, _) = crate::memory::frame_stats();
        if recover().is_err() {
            crate::println!("ahci: selftest FAIL recover");
            return;
        }
        let (free_post, _) = crate::memory::frame_stats();
        if free_pre != free_post {
            crate::println!("ahci: selftest FAIL leak after recover ({} -> {})", free_pre, free_post);
            return;
        }
        for b in rbuf.iter_mut() { *b = 0; }
        fill_pattern(&mut wbuf, 0xC0);
        if read_sector(tsecs[0], &mut rbuf).is_err() || wbuf != rbuf {
            crate::println!("ahci: selftest FAIL data after recover");
            return;
        }
    }
    /* Restore every test sector from the snapshot and verify. */
    for (i, &sec) in tsecs.iter().enumerate() {
        if write_sector(sec, &snap[i * 512..(i + 1) * 512]).is_err() {
            crate::println!("ahci: selftest FAIL restore sec={}", sec);
            return;
        }
        for b in rbuf.iter_mut() { *b = 0; }
        if read_sector(sec, &mut rbuf).is_err() || rbuf != snap[i * 512..(i + 1) * 512] {
            crate::println!("ahci: selftest FAIL restore verify sec={}", sec);
            return;
        }
    }
    crate::dma::free_coherent(snap_va, snap_pa, 4096);
    {
        let (free_end, _) = crate::memory::frame_stats();
        if free_base != free_end {
            crate::println!("ahci: selftest FAIL leak overall ({} -> {})", free_base, free_end);
            return;
        }
    }
    crate::println!("ahci: selftest PASS resets=1");
}
