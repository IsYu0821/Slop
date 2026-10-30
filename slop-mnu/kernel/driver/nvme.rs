//! NVMe controller driver (PCI class 0x01/0x08/0x02, no fixed IDs).
//!
//! Bring-up order: class probe -> BAR0 map -> controller reset ->
//! admin queues -> identify controller/namespace -> I/O queues ->
//! MSI-X -> read/write/flush. All DMA uses coherent identity-mapped
//! memory (no IOMMU yet: valid only while DMA addresses equal physical
//! addresses; Track 8 replaces this with explicit IOVA mapping).
//! One command outstanding per queue (serialized); completion is
//! phase-tag polled with a bounded timeout, never depending on IRQ
//! delivery. The MSI-X handler only records delivery (proved by counter
//! in selftest). Single-sector transfers use PRP1/PRP2 directly.
use crate::driver::pci::{self, Bdf, Cam, ConfigAccess, CFG_CLASS, CFG_COMMAND, CFG_VENDOR, CMD_BUS_MASTER, CMD_MEM};
use crate::sync::IrqSpinLock;
use core::sync::atomic::{AtomicU32, Ordering};

/* Controller registers (BAR0). */
const R_CAP: u64 = 0x00;
const R_VS: u64 = 0x08;
const R_CC: u64 = 0x14;
const R_CSTS: u64 = 0x1c;
const R_AQA: u64 = 0x24;
const R_ASQ: u64 = 0x28;
const R_ACQ: u64 = 0x30;
const CC_EN: u32 = 1;
const CC_IOSQES_64: u32 = 6 << 16;
const CC_IOCQES_16: u32 = 4 << 20; /* log2(16B CQE) */
const CC_SHN_NORMAL: u32 = 1 << 14;
const CSTS_RDY: u32 = 1;
const CSTS_SHST_MASK: u32 = 0b11 << 2;
const CSTS_SHST_DONE: u32 = 0b10 << 2;
const OP_IDENTIFY: u8 = 0x06;
const OP_FLUSH: u8 = 0x00;
const OP_WRITE: u8 = 0x01;
const OP_READ: u8 = 0x02;
const CNS_NAMESPACE: u32 = 0;
const CNS_CONTROLLER: u32 = 1;
const MSI_ADDR_BASE: u32 = 0xfee0_0000;
const PCI_CMD_INTX_OFF: u32 = 1 << 10;

const QUEUE_DEPTH: usize = 64;
const SQE_BYTES: usize = 64;
const CQE_BYTES: usize = 16;
/* Queue size (0-based): 64 entries. CDW10 layout is QID[15:0] | QSIZE[31:16]. */
const QSIZE_64: u32 = 63;

fn spin_pause() {
    for _ in 0..1000 { core::hint::spin_loop(); }
}

struct Queue {
    sq_va: u64, sq_pa: u64,
    cq_va: u64, cq_pa: u64,
    sq_tail: u16,
    cq_head: u16,
    cq_phase: u8,
    cid: u16,
}
const NO_QUEUE: Queue = Queue { sq_va: 0, sq_pa: 0, cq_va: 0, cq_pa: 0, sq_tail: 0, cq_head: 0, cq_phase: 1, cid: 0 };

struct Nvme {
    ready: bool,
    bdf: Bdf,
    bar: u64,
    dstrd: u32,
    nsid: u32,
    ns_lbas: u64,
    lba_shift: u8,
    admin: Queue,
    io: Queue,
    irq_vec: u8,
    msix_bar: u64,
    resets: u32,
    timeouts: u32,
}
const NO_NVME: Nvme = Nvme { ready: false, bdf: Bdf { segment: 0, bus: 0, device: 0, function: 0 }, bar: 0, dstrd: 0, nsid: 0, ns_lbas: 0, lba_shift: 9, admin: NO_QUEUE, io: NO_QUEUE, irq_vec: 0, msix_bar: 0, resets: 0, timeouts: 0 };
static NVME: IrqSpinLock<Nvme> = IrqSpinLock::new(NO_NVME);
static CMD_LOCK: IrqSpinLock<()> = IrqSpinLock::new(());
static IRQ_FIRED: AtomicU32 = AtomicU32::new(0);

fn r32(bar: u64, off: u64) -> u32 { unsafe { (bar as *const u32).byte_add(off as usize).read_volatile() } }
fn w32(bar: u64, off: u64, v: u32) { unsafe { (bar as *mut u32).byte_add(off as usize).write_volatile(v) } }
fn r64(bar: u64, off: u64) -> u64 { (r32(bar, off) as u64) | ((r32(bar, off + 4) as u64) << 32) }
fn w64(bar: u64, off: u64, v: u64) {
    w32(bar, off, v as u32);
    w32(bar, off + 4, (v >> 32) as u32);
}

fn nvme_handler(vector: u8, dev_id: u64) -> bool {
    let _ = (vector, dev_id);
    IRQ_FIRED.fetch_add(1, Ordering::SeqCst);
    true
}

fn wait_csts(bar: u64, mask: u32, want: u32, timeout_ms: u64) -> Result<(), &'static str> {
    let deadline = crate::arch::x86_64::apic::ticks().wrapping_add(timeout_ms / 4 + 2);
    while r32(bar, R_CSTS) & mask != want {
        spin_pause();
        if crate::arch::x86_64::apic::ticks().wrapping_sub(deadline) < 0x8000_0000 {
            return Err("csts timeout");
        }
    }
    Ok(())
}

fn alloc_queue() -> Result<(u64, u64, u64, u64), &'static str> {
    let (sq_va, sq_pa) = crate::dma::alloc_coherent(4096).ok_or("sq alloc")?;
    let (cq_va, cq_pa) = crate::dma::alloc_coherent(4096).ok_or("cq alloc")?;
    unsafe {
        core::ptr::write_bytes(sq_va as *mut u8, 0, 4096);
        core::ptr::write_bytes(cq_va as *mut u8, 0, 4096);
    }
    Ok((sq_va, sq_pa, cq_va, cq_pa))
}

fn free_queue(sq_pa: u64, cq_pa: u64) {
    if sq_pa != 0 { crate::dma::free_coherent(0, sq_pa, 4096); }
    if cq_pa != 0 { crate::dma::free_coherent(0, cq_pa, 4096); }
}

fn sq_doorbell(bar: u64, dstrd: u32, qid: u32) -> u64 { 0x1000 + (2 * qid as u64) * ((1u64 << dstrd) * 4) }
fn cq_doorbell(bar: u64, dstrd: u32, qid: u32) -> u64 { 0x1000 + (2 * qid as u64 + 1) * ((1u64 << dstrd) * 4) }

fn bdf_pack(bdf: Bdf) -> u64 { ((bdf.bus as u64) << 16) | ((bdf.device as u64) << 8) | (bdf.function as u64) }

/* Submit one command; returns its CID. Caller must hold CMD_LOCK. */
fn submit(admin: bool, opcode: u8, nsid: u32, cdw10: u32, cdw11: u32, cdw12: u32, prp1: u64, prp2: u64) -> u16 {
    /* Doorbell values must stay below the queue size: the controller
       ignores (drops) out-of-range tails, which wedges the queue after
       QUEUE_DEPTH submissions. Wrap the tail; CIDs stay unique. */
    let (sq_va, slot, tail, bar, dstrd, qid) = {
        let mut n = NVME.lock();
        if admin {
            let slot = n.admin.sq_tail as usize;
            n.admin.sq_tail += 1;
            if n.admin.sq_tail as usize >= QUEUE_DEPTH { n.admin.sq_tail = 0; }
            (n.admin.sq_va, slot, n.admin.sq_tail as u32, n.bar, n.dstrd, 0u32)
        } else {
            let slot = n.io.sq_tail as usize;
            n.io.sq_tail += 1;
            if n.io.sq_tail as usize >= QUEUE_DEPTH { n.io.sq_tail = 0; }
            (n.io.sq_va, slot, n.io.sq_tail as u32, n.bar, n.dstrd, 1u32)
        }
    };
    let sqe = sq_va + (slot as u64) * (SQE_BYTES as u64);
    let cid = {
        let mut n = NVME.lock();
        if admin {
            n.admin.cid = n.admin.cid.wrapping_add(1);
            if n.admin.cid == 0 { n.admin.cid = 1; }
            n.admin.cid
        } else {
            n.io.cid = n.io.cid.wrapping_add(1);
            if n.io.cid == 0 { n.io.cid = 1; }
            n.io.cid
        }
    };
    unsafe {
        let p = sqe as *mut u32;
        p.write_volatile((opcode as u32) | ((cid as u32) << 16));
        p.add(1).write_volatile(nsid);
        p.add(2).write_volatile(0);
        p.add(3).write_volatile(0);
        p.add(4).write_volatile(0);
        p.add(5).write_volatile(0);
        (sqe as *mut u64).add(3).write_volatile(prp1);
        (sqe as *mut u64).add(4).write_volatile(prp2);
        p.add(10).write_volatile(cdw10);
        p.add(11).write_volatile(cdw11);
        p.add(12).write_volatile(cdw12);
        p.add(13).write_volatile(0);
        p.add(14).write_volatile(0);
        p.add(15).write_volatile(0);
    }
    core::sync::atomic::compiler_fence(Ordering::SeqCst);
    w32(bar, sq_doorbell(bar, dstrd, qid), tail);
    cid
}

/* Wait for `cid` on queue `qid`. Single outstanding, so the head slot
   carries our completion. Returns the status field (0 = ok). */
fn poll(qid: u32, cid: u16, timeout_ms: u64) -> Result<u16, i64> {
    let was_if = unsafe {
        let f: u64;
        core::arch::asm!("pushfq; pop {}", out(reg) f, options(nomem));
        f & (1 << 9) != 0
    };
    // Tick-based deadline (a tick is ~4ms here): robust against pause-cost variance under TCG/KVM.
    let deadline = crate::arch::x86_64::apic::ticks().wrapping_add(timeout_ms / 4 + 2);
    let mut waited = 0u64;
    loop {
        let (done, status) = {
            let n = NVME.lock();
            let (cq_va, cq_head, cq_phase) = if qid == 0 {
                (n.admin.cq_va, n.admin.cq_head, n.admin.cq_phase)
            } else {
                (n.io.cq_va, n.io.cq_head, n.io.cq_phase)
            };
            let slot = (cq_head as usize) % QUEUE_DEPTH;
            let cqe = cq_va + (slot as u64) * (CQE_BYTES as u64);
            let (ccid, p, st) = unsafe {
                let dw3 = (cqe as *const u32).add(3).read_volatile();
                (((dw3 & 0xffff) as u16), ((dw3 >> 16) & 1) as u8, (dw3 >> 17) as u16)
            };
            if p == cq_phase && ccid == cid { (true, st) } else { (false, 0) }
        };
        if done {
            let (bar, dstrd, head) = {
                let mut n = NVME.lock();
                if qid == 0 {
                    n.admin.cq_head += 1;
                    if n.admin.cq_head as usize >= QUEUE_DEPTH {
                        n.admin.cq_head = 0;
                        n.admin.cq_phase ^= 1;
                    }
                    (n.bar, n.dstrd, n.admin.cq_head as u32)
                } else {
                    n.io.cq_head += 1;
                    if n.io.cq_head as usize >= QUEUE_DEPTH {
                        n.io.cq_head = 0;
                        n.io.cq_phase ^= 1;
                    }
                    (n.bar, n.dstrd, n.io.cq_head as u32)
                }
            };
    w32(bar, cq_doorbell(bar, dstrd, qid), head);
            return Ok(status);
        }
        spin_pause();
        waited += 1;
        if waited % 100 == 99 {
            unsafe { core::arch::asm!("sti", options(nomem, nostack, preserves_flags)) };
            unsafe { core::arch::asm!("hlt", options(nomem, nostack, preserves_flags)) };
            if !was_if {
                unsafe { core::arch::asm!("cli", options(nomem, nostack, preserves_flags)) };
            }
        }
        if crate::arch::x86_64::apic::ticks().wrapping_sub(deadline) < 0x8000_0000 {
            crate::println!("nvme: poll timeout q={}", qid);
            return Err(crate::errno::ETIMEDOUT);
        }
    }
}

/* Admin command + poll. Returns the status field (0 = ok). */
fn admin_cmd(opcode: u8, nsid: u32, cdw10: u32, cdw11: u32, cdw12: u32, prp1: u64, prp2: u64, timeout_ms: u64) -> Result<u16, i64> {
    let _g = CMD_LOCK.lock();
    let cid = submit(true, opcode, nsid, cdw10, cdw11, cdw12, prp1, prp2);
    poll(0, cid, timeout_ms)
}

fn identify(nsid: u32, cns: u32, out_pa: u64) -> Result<(), i64> {
    let st = admin_cmd(OP_IDENTIFY, nsid, cns, 0, 0, out_pa, 0, 5000)?;
    if st != 0 { return Err(crate::errno::EIO); }
    Ok(())
}

/* ---- probe + controller bring-up ---- */
fn find_nvme() -> Option<Bdf> {
    let acc = Cam;
    for bus in 0..=255u8 {
        for dev in 0..32u8 {
            for fun in 0..8u8 {
                let bdf = Bdf { segment: 0, bus, device: dev, function: fun };
                if acc.read32(bdf, CFG_VENDOR) & 0xffff == 0xffff { continue; }
                let class = acc.read32(bdf, CFG_CLASS);
                if (class >> 24) as u8 == 0x01 && ((class >> 16) & 0xff) as u8 == 0x08 && ((class >> 8) & 0xff) as u8 == 0x02 {
                    if fun > 0 { continue; }
                    return Some(bdf);
                }
            }
        }
    }
    None
}

fn setup_controller(bdf: Bdf, bar: u64) -> Result<(), &'static str> {
    let mut cc = r32(bar, R_CC);
    if cc & CC_EN != 0 {
        cc &= !CC_EN;
        w32(bar, R_CC, cc);
        wait_csts(bar, CSTS_RDY, 0, 5000)?;
    }
    let cap = r64(bar, R_CAP);
    let dstrd = ((cap >> 32) & 0xf) as u32;
    crate::println!("nvme: cap={:#x} vs={:#x}", cap, r32(bar, R_VS));
    let (asq_va, asq_pa) = crate::dma::alloc_coherent(4096).ok_or("asq alloc")?;
    let (acq_va, acq_pa) = crate::dma::alloc_coherent(4096).ok_or("acq alloc")?;
    unsafe {
        core::ptr::write_bytes(asq_va as *mut u8, 0, 4096);
        core::ptr::write_bytes(acq_va as *mut u8, 0, 4096);
    }
    {
        let mut n = NVME.lock();
        n.admin = Queue { sq_va: asq_va, sq_pa: asq_pa, cq_va: acq_va, cq_pa: acq_pa, sq_tail: 0, cq_head: 0, cq_phase: 1, cid: 0 };
        n.bar = bar;
        n.bdf = bdf;
        n.dstrd = dstrd;
    }
    w32(bar, R_AQA, ((63u32) << 16) | 63);
    w64(bar, R_ASQ, asq_pa);
    w64(bar, R_ACQ, acq_pa);
    w32(bar, R_CC, CC_EN | CC_IOSQES_64 | CC_IOCQES_16);
    wait_csts(bar, CSTS_RDY, CSTS_RDY, 5000)?;
    Ok(())
}

/* ---- I/O queues + MSI-X ---- */
fn setup_io_queues() -> Result<(), &'static str> {
    let (sq_va, sq_pa, cq_va, cq_pa) = alloc_queue()?;
    {
        let mut n = NVME.lock();
        n.io = Queue { sq_va, sq_pa, cq_va, cq_pa, sq_tail: 0, cq_head: 0, cq_phase: 1, cid: 0 };
    }
    /* Drop any stale firmware queues first (duplicate QIDs fail). Ignore errors. */
    for q in 1..=4u32 {
        let _ = admin_cmd(0x00, 0, q, 0, 0, 0, 0, 1000);
    }
    for q in 1..=4u32 {
        let _ = admin_cmd(0x04, 0, q, 0, 0, 0, 0, 1000);
    }
    /* CCQ: QID=1, QSIZE=63. CDW11 = PC|IEN (IV set when MSI-X is up). */
    let st = admin_cmd(0x05, 0, 1 | (63 << 16), 3, 0, cq_pa, 0, 5000).map_err(|_| "create cq")?;
    if st != 0 {
        crate::println!("nvme: create cq rejected st={:#x}", st);
        return Err("create cq status");
    }
    /* CSQ: QID=1, QSIZE=63, CQID=1. */
    let st = admin_cmd(0x01, 0, 1 | (63 << 16), (1 << 16) | 1, 0, sq_pa, 0, 5000).map_err(|_| "create sq")?;
    if st != 0 {
        crate::println!("nvme: create sq rejected st={:#x}", st);
        return Err("create sq status");
    }
    Ok(())
}

fn setup_msix(bdf: Bdf, reg_bar: u64, reg_nr: u8) -> Result<u8, &'static str> {
    let info = pci::msix_enable(bdf).map_err(|_| "msix enable")?;
    crate::println!("nvme: msix table_bar={} off={:#x} size={}", info.table_bar, info.table_offset, info.table_size);
    /* The table usually lives in the registers BAR: reuse that mapping
       instead of mapping the same BAR twice (the second reservation
       would fail). Only map a different BAR. */
    let (tbar, owned) = if info.table_bar == reg_nr {
        (reg_bar, false)
    } else {
        let t = pci::ioremap_bar(bdf, info.table_bar);
        if t == 0 {
            pci::msix_disable(bdf);
            return Err("msix table map");
        }
        (t, true)
    };
    let vec = pci::msix_alloc_vector(bdf);
    if vec < 0 {
        if owned { pci::iounmap_bar(tbar); }
        pci::msix_disable(bdf);
        return Err("msix vector");
    }
    let vec = vec as u8;
    if crate::irq::register(vec, nvme_handler, bdf_pack(bdf), b"nvme0\0", false) != 0 {
        pci::msix_free_vector(bdf);
        if owned { pci::iounmap_bar(tbar); }
        pci::msix_disable(bdf);
        return Err("msix register");
    }
    let table = tbar + info.table_offset as u64;
    let me = crate::arch::x86_64::apic::apic_id() as u8;
    unsafe {
        (table as *mut u32).write_volatile(MSI_ADDR_BASE | ((me as u32) << 12));
        (table as *mut u32).add(1).write_volatile(0);
        (table as *mut u32).add(2).write_volatile(vec as u32);
        let c = (table as *mut u32).add(3).read_volatile();
        (table as *mut u32).add(3).write_volatile(c & !1);
    }
    {
        let mut n = NVME.lock();
        n.irq_vec = vec;
        n.msix_bar = if owned { tbar } else { 0 };
    }
    Ok(vec)
}

fn teardown_msix(bdf: Bdf) {
    let (vec, bar) = {
        let mut n = NVME.lock();
        let v = n.irq_vec;
        let b = n.msix_bar;
        n.irq_vec = 0;
        n.msix_bar = 0;
        (v, b)
    };
    if vec != 0 {
        crate::irq::unregister(vec, bdf_pack(bdf));
        pci::msix_free_vector(bdf);
    }
    if bar != 0 {
        pci::iounmap_bar(bar);
    }
    pci::msix_disable(bdf);
}

/* ---- init / identify ---- */
pub fn init() -> Result<u64, &'static str> {
    let bdf = find_nvme().ok_or("nvme not found")?;
    {
        let acc = Cam;
        let cmd = acc.read32(bdf, CFG_COMMAND);
        acc.write32(bdf, CFG_COMMAND, cmd | CMD_MEM as u32 | CMD_BUS_MASTER as u32 | PCI_CMD_INTX_OFF); 
    }
    let bar = pci::ioremap_bar(bdf, 0);
    if bar == 0 { return Err("nvme bar map"); }
    setup_controller(bdf, bar)?;
    /* Identify controller: namespace count (diagnostic). */
    {
        let (va, pa) = crate::dma::alloc_coherent(4096).ok_or("identify alloc")?;
        identify(0, CNS_CONTROLLER, pa).map_err(|_| "identify controller")?;
        let vid = unsafe { *(va as *const u16) };
        let ver = unsafe { *((va as *const u32).add(20)) };
        crate::println!("nvme: {:02x}:{:02x}.{} identify ok vid={:#x} ver={:#x}", bdf.bus, bdf.device, bdf.function, vid, ver);
        crate::dma::free_coherent(va, pa, 4096);
    }
    /* Identify namespace: probe NSIDs directly (robust to NN reporting
       capacity instead of attached namespaces). First success wins. */
    let (nsid, ns_lbas, lbads) = {
        let mut found = None;
        for id in 1..=8u32 {
            let (va, pa) = crate::dma::alloc_coherent(4096).ok_or("identify alloc")?;
            let r = identify(id, CNS_NAMESPACE, pa);
            if r.is_ok() {
                let (ns, flbas) = unsafe {
                    let b = va as *const u8;
                    let nsze = (b as *const u64).read_volatile();
                    let fl = *b.add(26);
                    (nsze, fl & 0xf)
                };
                let lb = unsafe { *((va as *const u8).add(128 + (flbas as usize) * 16 + 2) as *const u8) };
                crate::dma::free_coherent(va, pa, 4096);
                if ns != 0 {
                    found = Some((id, ns, lb));
                    break;
                }
            } else {
                crate::dma::free_coherent(va, pa, 4096);
            }
        }
        found.ok_or("no namespace")?
    };
    if ns_lbas == 0 { return Err("empty namespace"); }
    if lbads < 9 || lbads > 12 { return Err("bad lba size"); }
    {
        let mut n = NVME.lock();
        n.nsid = nsid;
        n.ns_lbas = ns_lbas;
        n.lba_shift = lbads;
    }
    /* MSI-X capability first: I/O queue creation with IEN=1 needs a
       live vector, and the table entry must exist before completions. */
    let reg_bar = NVME.lock().bar;
    match setup_msix(bdf, reg_bar, 0) {
        Ok(v) => crate::println!("nvme: MSI-X vec={:#x}", v),
        Err(e) => crate::println!("nvme: MSI-X unavailable ({})", e),
    }
    setup_io_queues()?;
    {
        let mut n = NVME.lock();
        n.ready = true;
    }
    let sectors = ns_lbas << (lbads - 9);
    crate::println!("nvme: ready nsid={} ns_lbas={} lbads={} ({} 512B sectors)", nsid, ns_lbas, lbads, sectors);
    Ok(sectors)
}

pub fn ready() -> bool { NVME.lock().ready }
pub fn capacity() -> u64 {
    let n = NVME.lock();
    if !n.ready { return 0; }
    n.ns_lbas << (n.lba_shift - 9)
}

fn lba_of(sector512: u64) -> Option<(u64, u64)> {
    let n = NVME.lock();
    if !n.ready { return None; }
    let per = 1u64 << (n.lba_shift - 9);
    if sector512 >= n.ns_lbas * per { return None; }
    Some((sector512 / per, sector512 % per))
}

/* PRP pair for identity-mapped RAM (up to two pages: covers any 512B
   sector and any single LBA up to 4K). */
fn prp_for(va: u64, len: usize) -> Result<(u64, u64), i64> {
    if va == 0 || len == 0 || len > 8192 {
        crate::println!("nvme: prp bad args va={:#x} len={}", va, len);
        return Err(crate::errno::EINVAL);
    }
    // NOTE: the direct map uses 1 GiB pages, which plain virt_to_phys
    // rejects; virt_to_phys_full walks huge pages too.
    let mut prev: Option<u64> = None;
    let mut first = 0u64;
    let mut off = va & !4095;
    let end = (va + len as u64 + 4095) & !4095;
    let mut pages = 0u32;
    while off < end {
        if pages >= 2 { return Err(crate::errno::EINVAL); }
        let pa = crate::arch::x86_64::phys::virt_to_phys_full(off);
        if pa == 0 {
            crate::println!("nvme: prp unmapped va={:#x}", off);
            return Err(crate::errno::EFAULT);
        }
        if off == va & !4095 { first = pa + (va & 4095); }
        if let Some(pp) = prev {
            if pa != pp + 4096 {
                crate::println!("nvme: prp noncontig {:#x} vs {:#x}", pa, pp);
                return Err(crate::errno::EFAULT);
            }
        }
        prev = Some(pa);
        off += 4096;
        pages += 1;
    }
    let prp1 = first;
    let prp2 = if pages > 1 {
        let second_va = (va & !4095) + 4096;
        crate::arch::x86_64::phys::virt_to_phys_full(second_va)
    } else { 0 };
    Ok((prp1, prp2))
}

/* Whole-LBA transfer. `va` points at a buffer holding exactly one LBA. */
fn xfer_lba(lba: u64, va: u64, is_write: bool, timeout_ms: u64) -> Result<(), i64> {
    let n = NVME.lock();
    let shift = n.lba_shift;
    drop(n);
    if shift < 9 || shift > 12 { return Err(crate::errno::EINVAL); }
    let (prp1, prp2) = prp_for(va, 1usize << shift)?;
    let opcode = if is_write { OP_WRITE } else { OP_READ };
    let _g = CMD_LOCK.lock();
    let cid = submit(false, opcode, 1, (lba & 0xffff_ffff) as u32, (lba >> 32) as u32, 0, prp1, prp2);
    let st = poll(1, cid, timeout_ms)?;
    if st != 0 { return Err(crate::errno::EIO); }
    Ok(())
}

/* 512-byte sector read/write for the block facade. Larger LBAs use a
   coherent scratch page for read-modify-write. */
pub fn read_sector(sector: u64, dst: &mut [u8]) -> Result<(), i64> {
    if dst.len() < 512 { return Err(crate::errno::EINVAL); }
    let (lba, off) = lba_of(sector).ok_or(crate::errno::ENODEV)?;
    let shift = NVME.lock().lba_shift;
    if shift == 9 {
        xfer_lba(lba, dst.as_ptr() as u64, false, 10000)?;
        return Ok(());
    }
    let (sva, spa) = crate::dma::alloc_coherent(4096).ok_or(crate::errno::ENOMEM)?;
    let r = xfer_lba(lba, sva, false, 10000);
    if r.is_ok() {
        unsafe { core::ptr::copy_nonoverlapping((sva + off) as *const u8, dst.as_mut_ptr(), 512) };
    }
    crate::dma::free_coherent(sva, spa, 4096);
    r
}

pub fn write_sector(sector: u64, src: &[u8]) -> Result<(), i64> {
    if src.len() < 512 { return Err(crate::errno::EINVAL); }
    let (lba, off) = lba_of(sector).ok_or(crate::errno::ENODEV)?;
    let shift = NVME.lock().lba_shift;
    if shift == 9 {
        xfer_lba(lba, src.as_ptr() as u64, true, 10000)?;
        return Ok(());
    }
    let (sva, spa) = crate::dma::alloc_coherent(4096).ok_or(crate::errno::ENOMEM)?;
    let r = xfer_lba(lba, sva, false, 10000).and_then(|_| {
        unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), (sva + off) as *mut u8, 512) };
        xfer_lba(lba, sva, true, 10000)
    });
    crate::dma::free_coherent(sva, spa, 4096);
    r
}

pub fn flush() -> Result<(), i64> {
    let _g = CMD_LOCK.lock();
    let cid = submit(false, OP_FLUSH, 1, 0, 0, 0, 0, 0);
    let st = poll(1, cid, 10000)?;
    if st != 0 { return Err(crate::errno::EIO); }
    Ok(())
}

pub fn shutdown_notify() -> Result<(), &'static str> {
    let bar = NVME.lock().bar;
    if bar == 0 { return Err("not ready"); }
    let mut cc = r32(bar, R_CC);
    cc = (cc & !CC_SHN_NORMAL) | CC_SHN_NORMAL;
    w32(bar, R_CC, cc);
    wait_csts(bar, CSTS_SHST_MASK, CSTS_SHST_DONE, 5000)?;
    Ok(())
}

/* Full controller reset + re-setup. Admin + I/O queues are freed and
   rebuilt, so a reset cycle is allocation-neutral (leak-checked). */
pub fn recover() -> Result<(), &'static str> {
    let (bdf, bar) = {
        let n = NVME.lock();
        (n.bdf, n.bar)
    };
    if bar == 0 { return Err("not ready"); }
    {
        let mut n = NVME.lock();
        free_queue(n.admin.sq_pa, n.admin.cq_pa);
        free_queue(n.io.sq_pa, n.io.cq_pa);
        n.admin = NO_QUEUE;
        n.io = NO_QUEUE;
        n.ready = false;
    }
    teardown_msix(bdf);
    let mut cc = r32(bar, R_CC);
    cc &= !CC_EN;
    w32(bar, R_CC, cc);
    wait_csts(bar, CSTS_RDY, 0, 5000)?;
    {
        let mut n = NVME.lock();
        n.resets += 1;
    }
    setup_controller(bdf, bar)?;
    {
        let nsid = NVME.lock().nsid;
        let (va, pa) = crate::dma::alloc_coherent(4096).ok_or("identify alloc")?;
        let r = identify(nsid, CNS_NAMESPACE, pa);
        crate::dma::free_coherent(va, pa, 4096);
        r.map_err(|_| "re-identify")?;
    }
    let reg_bar = NVME.lock().bar;
    let _ = setup_msix(bdf, reg_bar, 0);
    setup_io_queues()?;
    {
        let mut n = NVME.lock();
        n.ready = true;
    }
    Ok(())
}

/* ---- boot selftest (skips cleanly without NVMe hardware) ---- */
fn fill_pattern(buf: &mut [u8], seed: u64) {
    for (i, b) in buf.iter_mut().enumerate() {
        *b = ((i as u64).wrapping_mul(0x11).wrapping_add(seed) & 0xff) as u8;
    }
}

pub fn selftest() {
    if !ready() {
        crate::println!("nvme: selftest SKIP (no controller)");
        return;
    }
    let cap = capacity();
    crate::println!("nvme: selftest start sectors={}", cap);
    if cap < 16 {
        crate::println!("nvme: selftest FAIL tiny namespace");
        return;
    }
    let (free_base, _) = crate::memory::frame_stats();
    let mut wbuf = [0u8; 512];
    let mut rbuf = [0u8; 512];
    /* Test sectors live at the top of the namespace (away from filesystem
       metadata), and every byte is snapshotted first: the selftest must
       leave a live filesystem image bit-identical. */
    let tsecs = [cap - 8, cap - 7, cap - 6, cap - 5, cap - 1];
    let prp_sec = cap - 4;
    let (snap_va, snap_pa) = match crate::dma::alloc_coherent(4096) {
        Some(v) => v,
        None => {
            crate::println!("nvme: selftest FAIL snapshot alloc");
            return;
        }
    };
    let mut snap = unsafe { core::slice::from_raw_parts_mut(snap_va as *mut u8, 3072) };
    for (i, &sec) in tsecs.iter().chain(core::iter::once(&prp_sec)).enumerate() {
        if read_sector(sec, &mut snap[i * 512..(i + 1) * 512]).is_err() {
            crate::println!("nvme: selftest FAIL snapshot sec={}", sec);
            crate::dma::free_coherent(snap_va, snap_pa, 4096);
            return;
        }
    }
    /* 1. Write/read-back across several LBAs, incl. the last sector. */
    for (i, &lba_sec) in tsecs.iter().enumerate() {
        fill_pattern(&mut wbuf, 0xA0 + i as u64);
        for b in rbuf.iter_mut() { *b = 0; }
        if write_sector(lba_sec, &wbuf).is_err() {
            crate::println!("nvme: selftest FAIL write sec={}", lba_sec);
            return;
        }
        if read_sector(lba_sec, &mut rbuf).is_err() {
            crate::println!("nvme: selftest FAIL read sec={}", lba_sec);
            return;
        }
        if wbuf != rbuf {
            crate::println!("nvme: selftest FAIL mismatch sec={}", lba_sec);
            return;
        }
    }
    /* 2. Unaligned (page-crossing) buffer exercises the PRP2 path. */
    {
        let (va, pa) = match crate::dma::alloc_coherent(8192) {
            Some(v) => v,
            None => {
                crate::println!("nvme: selftest FAIL prp alloc");
                return;
            }
        };
        let cross_va = va + 4096 - 256;
        let mut cw = [0u8; 512];
        let mut cr = [0u8; 512];
        fill_pattern(&mut cw, 0x77);
        unsafe { core::ptr::copy_nonoverlapping(cw.as_ptr(), cross_va as *mut u8, 512) };
        let _ = pa;
        if xfer_lba(prp_sec, cross_va, true, 10000).is_err() {
            crate::println!("nvme: selftest FAIL prp2 write");
            crate::dma::free_coherent(va, pa, 8192);
            return;
        }
        unsafe { core::ptr::write_bytes(cross_va as *mut u8, 0, 512) };
        if xfer_lba(prp_sec, cross_va, false, 10000).is_err() {
            crate::println!("nvme: selftest FAIL prp2 read");
            crate::dma::free_coherent(va, pa, 8192);
            return;
        }
        unsafe { core::ptr::copy_nonoverlapping(cross_va as *const u8, cr.as_mut_ptr(), 512) };
        crate::dma::free_coherent(va, pa, 8192);
        if cw != cr {
            crate::println!("nvme: selftest FAIL prp2 mismatch cr[0..16]={:02x?}", &cr[..16]);
            return;
        }
    }
    /* 3. Flush works. */
    if flush().is_err() {
        crate::println!("nvme: selftest FAIL flush");
        return;
    }
    /* 4. MSI-X delivery happened (when MSI-X is up). */
    {
        let fired = IRQ_FIRED.load(Ordering::Relaxed);
        let vec = NVME.lock().irq_vec;
        crate::println!("nvme: selftest irqs fired={} vec={:#x}", fired, vec);
        if vec != 0 && fired == 0 {
            crate::println!("nvme: selftest FAIL no MSI-X delivery");
            return;
        }
    }
    /* 5. Timeout path: poll a never-submitted CID with a tiny budget. */
    {
        let _g = CMD_LOCK.lock();
        match poll(1, 0xBEEFu16, 1) {
            Err(crate::errno::ETIMEDOUT) => {},
            _ => {
                crate::println!("nvme: selftest FAIL timeout path");
                return;
            }
        }
        NVME.lock().timeouts += 1;
    }
    /* 6. Reset recovery on a live controller; data must survive. */
    {
        let (free_pre, _) = crate::memory::frame_stats();
        if recover().is_err() {
            crate::println!("nvme: selftest FAIL recover");
            return;
        }
        let (free_post, _) = crate::memory::frame_stats();
        if free_pre != free_post {
            crate::println!("nvme: selftest FAIL leak after recover ({} -> {})", free_pre, free_post);
            return;
        }
        for b in rbuf.iter_mut() { *b = 0; }
        fill_pattern(&mut wbuf, 0xA0);
        if read_sector(tsecs[0], &mut rbuf).is_err() || wbuf != rbuf {
            crate::println!("nvme: selftest FAIL data after recover");
            return;
        }
    }
    /* 7. Shutdown notification, then bring the controller back. */
    if shutdown_notify().is_err() {
        crate::println!("nvme: selftest FAIL shutdown");
        return;
    }
    if recover().is_err() {
        crate::println!("nvme: selftest FAIL post-shutdown recover");
        return;
    }
    /* Restore every test sector from the snapshot and verify. */
    for (i, &sec) in tsecs.iter().chain(core::iter::once(&prp_sec)).enumerate() {
        if write_sector(sec, &snap[i * 512..(i + 1) * 512]).is_err() {
            crate::println!("nvme: selftest FAIL restore sec={}", sec);
            return;
        }
        for b in rbuf.iter_mut() { *b = 0; }
        if read_sector(sec, &mut rbuf).is_err() || rbuf != snap[i * 512..(i + 1) * 512] {
            crate::println!("nvme: selftest FAIL restore verify sec={}", sec);
            return;
        }
    }
    crate::dma::free_coherent(snap_va, snap_pa, 4096);
    {
        let (free_end, _) = crate::memory::frame_stats();
        if free_base != free_end {
            crate::println!("nvme: selftest FAIL leak overall ({} -> {})", free_base, free_end);
            return;
        }
    }
    let n = NVME.lock();
    crate::println!("nvme: selftest PASS resets={} timeouts={}", n.resets, n.timeouts);
}
