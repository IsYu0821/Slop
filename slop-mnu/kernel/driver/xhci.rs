//! xHCI USB host controller (PCI class 0x0c/0x03/0x30, no fixed IDs).
//!
//! Generic-PC USB path (Profile B/C): QEMU qemu-xhci + usb-kbd + usb-mouse.
//! Reuses the Slop PCI/BAR/DMA/IRQ core. Staged bring-up so each layer is
//! testable: controller (reset/run/rings) -> ports -> enumeration ->
//! HID. Polling for completions (like AHCI); MSI-X/INTx is follow-up.
use crate::driver::pci::{self, Bdf, Cam, ConfigAccess, CFG_CLASS, CFG_COMMAND, CFG_VENDOR, CMD_BUS_MASTER, CMD_MEM};
use crate::sync::IrqSpinLock;
use core::sync::atomic::Ordering;

const MAX_SLOTS: usize = 8;
const MAX_PORTS: usize = 16;

/* Capability registers (BAR0+0). */
const R_CAPLENGTH: u64 = 0x00;
const R_HCIVERSION: u64 = 0x02;
const R_HCSPARAMS1: u64 = 0x04;
const R_HCSPARAMS2: u64 = 0x08;
const R_HCCPARAMS1: u64 = 0x10;
const R_DBOFF: u64 = 0x14;
const R_RTSOFF: u64 = 0x18;
/* Operational registers (BAR0+caplen). */
const R_USBCMD: u64 = 0x00;
const CMD_RS: u32 = 1 << 0;
const CMD_HCRST: u32 = 1 << 1;
const CMD_INTE: u32 = 1 << 2;
const R_USBSTS: u64 = 0x04;
const STS_HALT: u32 = 1 << 0;
const STS_CNR: u32 = 1 << 11;
const R_PAGESIZE: u64 = 0x08;
const R_DNCTRL: u64 = 0x14;
const R_CRCR: u64 = 0x18;
const CRCR_RC: u64 = 1;
const R_DCBAAP: u64 = 0x30;
const R_CONFIG: u64 = 0x38;
/* Port registers (BAR0+caplen+0x400+port*0x10). */
const PORT_BASE: u64 = 0x400;
const PORT_STRIDE: u64 = 0x10;
const R_P_SC: u64 = 0x00;
const P_CCS: u32 = 1 << 0;
const P_PED: u32 = 1 << 1;
const P_PR: u32 = 1 << 4;
const P_PP: u32 = 1 << 9;
const P_PRC: u32 = 1 << 21;
/* Runtime registers (BAR0+rtsoff): MFN at +0, interrupter 0 set at +0x20. */
const R_INTR: u64 = 0x20;
const R_IMAN: u64 = 0x20;
const R_IMOD: u64 = 0x24;
const R_ERSTSZ: u64 = 0x28;
const R_ERSTBA: u64 = 0x30;
const R_ERDP: u64 = 0x38;
const ERDP_EHB: u64 = 1 << 3;
/* Doorbell array (BAR0+dboff): db0 = host controller, dbN = doorbell N. */

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

struct Xhci {
    bdf: Bdf,
    bar: u64,
    op: u64,
    rt: u64,
    db: u64,
    n_ports: u32,
    cmd_va: u64,
    dcbaa_va: u64,
    erst_va: u64,
    ev_va: u64,
    ready: bool,
}
const NO_XHCI: Xhci = Xhci { bdf: Bdf { segment: 0, bus: 0, device: 0, function: 0 }, bar: 0, op: 0, rt: 0, db: 0, n_ports: 0, cmd_va: 0, dcbaa_va: 0, erst_va: 0, ev_va: 0, ready: false };
static XHCI: IrqSpinLock<Xhci> = IrqSpinLock::new(NO_XHCI);

fn find_xhci() -> Option<Bdf> {
    let acc = Cam;
    for bus in 0..=255u8 {
        for dev in 0..32u8 {
            for fun in 0..8u8 {
                let bdf = Bdf { segment: 0, bus, device: dev, function: fun };
                if acc.read32(bdf, CFG_VENDOR) & 0xffff == 0xffff { continue; }
                let class = acc.read32(bdf, CFG_CLASS);
                if (class >> 24) as u8 == 0x0c && ((class >> 16) & 0xff) as u8 == 0x03 && ((class >> 8) & 0xff) as u8 == 0x30 {
                    if fun > 0 { continue; }
                    return Some(bdf);
                }
            }
        }
    }
    None
}

fn was_if() -> bool {
    unsafe {
        let f: u64;
        core::arch::asm!("pushfq; pop {}", out(reg) f, options(nomem, preserves_flags));
        f & (1 << 9) != 0
    }
}
/* Spin with periodic sti+hlt so tick-based deadlines advance even when
   the caller runs with IF clear (early boot). Restores IF afterwards. */
fn wait_pause(waited: &mut u64, had_if: bool) {
    *waited += 1;
    if *waited % 100 == 99 {
        unsafe { core::arch::asm!("sti", options(nomem, nostack, preserves_flags)) };
        unsafe { core::arch::asm!("hlt", options(nomem, nostack, preserves_flags)) };
        if !had_if {
            unsafe { core::arch::asm!("cli", options(nomem, nostack, preserves_flags)) };
        }
    } else {
        core::hint::spin_loop();
    }
}
fn wait_op(bar: u64, op: u64, off: u64, mask: u32, want: u32, timeout_ms: u64) -> Result<(), &'static str> {
    let _ = bar;
    let deadline = crate::arch::x86_64::apic::ticks().wrapping_add(timeout_ms / 4 + 2);
    let had_if = was_if();
    let mut waited = 0u64;
    loop {
        if r32(op, off) & mask == want { return Ok(()); }
        wait_pause(&mut waited, had_if);
        if crate::arch::x86_64::apic::ticks().wrapping_sub(deadline) < 0x8000_0000 {
            return Err("xhci timeout");
        }
    }
}

pub fn ready() -> bool { XHCI.lock().ready }

pub fn init() -> Result<(), &'static str> {
    let bdf = find_xhci().ok_or("xhci not found")?;
    {
        let acc = Cam;
        let cmd = acc.read32(bdf, CFG_COMMAND);
        acc.write32(bdf, CFG_COMMAND, cmd | CMD_MEM as u32 | CMD_BUS_MASTER as u32);
    }
    let bar = pci::ioremap_bar(bdf, 0);
    if bar == 0 { return Err("xbar map"); }

    let caplen = (r32(bar, R_CAPLENGTH) & 0xff) as u64;
    let op = bar + caplen;
    let rtsoff = r32(bar, R_RTSOFF) as u64;
    let rt = bar + (rtsoff & !0x1f);
    let dboff = r32(bar, R_DBOFF) as u64;
    let db = bar + (dboff & !0x3);
    let hcs1 = r32(bar, R_HCSPARAMS1);
    let n_ports = (hcs1 >> 24) & 0xff;
    let version = (r32(bar, R_CAPLENGTH & !3) >> 16) & 0xffff;
    crate::println!("xhci: {:02x}:{:02x}.{} ver={:#x} ports={} caplen={:#x}", bdf.bus, bdf.device, bdf.function, version, n_ports, caplen);
    if n_ports == 0 || n_ports > MAX_PORTS as u32 {
        return Err("bad port count");
    }
    /* Stop first (in case BIOS left it running), then reset. */
    let mut cmd = r32(op, R_USBCMD);
    cmd &= !CMD_RS;
    w32(op, R_USBCMD, cmd);
    wait_op(bar, op, R_USBSTS, STS_HALT, STS_HALT, 2000).map_err(|_| "stop")?;
    let mut cmd = r32(op, R_USBCMD);
    cmd |= CMD_HCRST;
    w32(op, R_USBCMD, cmd);
    wait_op(bar, op, R_USBCMD, CMD_HCRST, 0, 5000).map_err(|_| "reset")?;
    wait_op(bar, op, R_USBSTS, STS_CNR, 0, 5000).map_err(|_| "cnr")?;
    /* Rings + tables: command ring (4K), DCBAA (4K, 64 entries), ERST (4K),
       event ring (4K). All page-aligned, identity-mapped for DMA. */
    let (cmd_va, cmd_pa) = crate::dma::alloc_coherent(4096).ok_or("cmd ring alloc")?;
    let (dcbaa_va, dcbaa_pa) = crate::dma::alloc_coherent(4096).ok_or("dcbaa alloc")?;
    let (erst_va, erst_pa) = crate::dma::alloc_coherent(4096).ok_or("erst alloc")?;
    let (ev_va, ev_pa) = crate::dma::alloc_coherent(4096).ok_or("event ring alloc")?;
    unsafe {
        core::ptr::write_bytes(cmd_va as *mut u8, 0, 4096);
        core::ptr::write_bytes(dcbaa_va as *mut u8, 0, 4096);
        core::ptr::write_bytes(erst_va as *mut u8, 0, 4096);
        core::ptr::write_bytes(ev_va as *mut u8, 0, 4096);
        /* Command ring link TRB at the end (cycle bit handling simplified:
           single-segment ring, link back to start with toggle). */
        let link = (cmd_va + 4096 - 16) as *mut u64;
        link.write_volatile(cmd_pa | (6u64 << 10)); /* type Link */
        link.add(1).write_volatile(0);
        /* ERST entry 0: one 4K event segment. */
        let e = erst_va as *mut u64;
        e.write_volatile(ev_pa);
        e.add(1).write_volatile(256); /* 256 TRBs */
        e.add(2).write_volatile(0);
    }
    {
        let mut x = XHCI.lock();
        x.bdf = bdf;
        x.bar = bar;
        x.op = op;
        x.rt = rt;
        x.db = db;
        x.n_ports = n_ports;
        x.cmd_va = cmd_va;
        x.dcbaa_va = dcbaa_va;
        x.erst_va = erst_va;
        x.ev_va = ev_va;
    }
    /* Program HBA: DCBAA, command ring (RC=1), one event segment. */
    w64(op, R_DCBAAP, crate::arch::x86_64::phys::virt_to_phys_full(dcbaa_va));
    w64(op, R_CRCR, crate::arch::x86_64::phys::virt_to_phys_full(cmd_va) | CRCR_RC);
    w32(op, R_CONFIG, 8); /* enable up to 8 device slots */
    w32(rt, R_ERSTSZ, 1);
    w64(rt, R_ERSTBA, crate::arch::x86_64::phys::virt_to_phys_full(erst_va));
    w64(rt, R_ERDP, crate::arch::x86_64::phys::virt_to_phys_full(ev_va));
    w32(rt, R_IMOD, 0);
    w32(rt, R_IMAN, (1 << 1) | 1); /* IE + clear IP */
    /* Run (with INTE: QEMU may gate transfer-event posting on it). */
    let mut cmd = r32(op, R_USBCMD);
    cmd |= CMD_RS | CMD_INTE;
    w32(op, R_USBCMD, cmd);
    wait_op(bar, op, R_USBSTS, STS_HALT, 0, 5000).map_err(|_| "run")?;
    {
        let mut x = XHCI.lock();
        x.ready = true;
    }
    crate::println!("xhci: running, {} ports", n_ports);
    Ok(())
}

/* ---- minimal selftest: controller runs, ports enumerated ---- */
pub fn selftest() {
    if !ready() {
        crate::println!("xhci: selftest SKIP (no controller)");
        return;
    }
    let (op, n_ports) = {
        let x = XHCI.lock();
        (x.op, x.n_ports)
    };
    let mut found = 0u32;
    for port in 1..=n_ports {
        let sc = r32(op, PORT_BASE + ((port - 1) as u64) * PORT_STRIDE + R_P_SC);
        let connected = sc & P_CCS != 0;
        if connected { found += 1; }
        crate::println!("xhci: port{} sc={:#x} {}", port, sc, if connected { "device" } else { "empty" });
    }
    let mut enumerated = 0u32;
    for port in 1..=n_ports {
        let sc = r32(op, PORT_BASE + ((port - 1) as u64) * PORT_STRIDE + R_P_SC);
        if sc & P_CCS == 0 { continue; }
        match enumerate_port(port) {
            Ok(()) => enumerated += 1,
            Err(e) => crate::println!("xhci: port{} enumerate failed: {}", port, e),
        }
    }
    crate::println!("xhci: selftest PASS ports_with_devices={} enumerated={}", found, enumerated);
    /* On-demand polling via evdev read (no background thread: a kernel
       polling thread hung the CPU after 2 rounds; root cause TBD). */
    // hid_start_poller();

}

/* ================= Stage 2: rings, commands, enumeration ================= */
/* TRB types. */
const TRB_NORMAL: u32 = 1;
const TRB_SETUP: u32 = 2;
const TRB_DATA: u32 = 3;
const TRB_STATUS: u32 = 4;
const TRB_LINK: u32 = 6;
const TRB_ENABLE_SLOT: u32 = 9;
const TRB_ADDR_DEV: u32 = 11;
const TRB_CONFIG_EP: u32 = 12;
/* Event TRB types. */
const EVT_TRANSFER: u32 = 32;
const EVT_CMD_COMPLETE: u32 = 33;

/* Ring with producer/consumer state (4K page, last slot is a Link TRB). */
struct Ring {
    va: u64,
    n_trbs: usize, /* usable TRBs (excluding link) */
    prod: usize,
    prod_cycle: u8,
    cons: usize,
    cons_cycle: u8,
}
const NO_RING: Ring = Ring { va: 0, n_trbs: 0, prod: 0, prod_cycle: 1, cons: 0, cons_cycle: 1 };

fn trb_addr(va: u64, idx: usize) -> u64 { va + (idx as u64) * 16 }

/* Write a generic TRB (param, status, control with cycle bit). */
fn write_trb(addr: u64, param: u64, status: u32, ctrl: u32) {
    unsafe {
        let p = addr as *mut u64;
        p.write_volatile(param);
        (addr as *mut u32).add(2).write_volatile(status);
        (addr as *mut u32).add(3).write_volatile(ctrl);
    }
}
fn trb_ctrl(trb_type: u32, cycle: u8) -> u32 { (trb_type << 10) | (cycle as u32) }

/* Advance a producer; handles link wrap (toggle cycle). */
fn ring_produce(r: &mut Ring) -> (u64, u8) {
    let addr = trb_addr(r.va, r.prod);
    let cycle = r.prod_cycle;
    r.prod += 1;
    if r.prod >= r.n_trbs {
        /* Fill the link TRB to wrap (toggle). */
        let link = trb_addr(r.va, r.n_trbs);
        let pa = crate::arch::x86_64::phys::virt_to_phys_full(r.va);
        r.prod_cycle ^= 1;
        let tc: u8 = r.prod_cycle;
        unsafe {
            let p = link as *mut u64;
            p.write_volatile(pa);
            (link as *mut u32).add(3).write_volatile((TRB_LINK << 10) | ((tc as u32) << 1) | (tc as u32));
        }
        r.prod = 0;
    }
    (addr, cycle)
}

/* Command ring + event ring processing state. */
struct CmdState {
    ring: Ring,
    ev_cons: usize,
    ev_cycle: u8,
}
static mut CMDQ: CmdState = CmdState { ring: NO_RING, ev_cons: 0, ev_cycle: 1 };

/* Wait for the next event TRB; returns (type, param, status, control). */
fn wait_event(timeout_ms: u64) -> Result<(u32, u64, u32, u32), &'static str> {
    let (ev_va, rt) = {
        let x = XHCI.lock();
        if !x.ready { return Err("not ready"); }
        (x.ev_va, x.rt)
    };
    let deadline = crate::arch::x86_64::apic::ticks().wrapping_add(timeout_ms / 4 + 2);
    let had_if_ev = was_if();
    let mut waited_ev = 0u64;
    loop {
        let (cons, cycle) = unsafe {
            let q = &raw mut CMDQ;
            ((*q).ev_cons, (*q).ev_cycle)
        };
        let addr = trb_addr(ev_va, cons % 256);
        let (cyc, typ, param, status) = unsafe {
            let dw3 = (addr as *const u32).add(3).read_volatile();
            let c = (dw3 & 1) as u8;
            let t = (dw3 >> 10) & 0x3f;
            let p = (addr as *const u64).read_volatile();
            let s = (addr as *const u32).add(2).read_volatile();
            (c, t, p, s)
        };
        if cyc == cycle {
            /* Consume: advance, toggle on wrap, update ERDP + EHB. */
            unsafe {
                let q = &raw mut CMDQ;
                (*q).ev_cons += 1;
                if (*q).ev_cons >= 256 {
                    (*q).ev_cons = 0;
                    (*q).ev_cycle ^= 1;
                }
            }
            let ev_pa = crate::arch::x86_64::phys::virt_to_phys_full(trb_addr(ev_va, cons % 256));
            w64(rt, R_ERDP, (ev_pa & !0xf) | ERDP_EHB);
            let ctrl = unsafe { (addr as *const u32).add(3).read_volatile() };
            return Ok((typ, param, status, ctrl));
        }
        wait_pause(&mut waited_ev, had_if_ev);
        if crate::arch::x86_64::apic::ticks().wrapping_sub(deadline) < 0x8000_0000 {
            static TO: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
            let n = TO.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            if n % 20000 == 0 {
                crate::println!("xhci: event timeout #{} ticks={}", n, crate::arch::x86_64::apic::ticks());
            }
            return Err("xhci event timeout");
        }
    }
}

/* Submit a command TRB on the command ring and ring HC doorbell 0. */
fn submit_cmd(param: u64, ctrl_nocycle: u32) -> Result<(), &'static str> {
    let (db, cmd_pa_base) = {
        let x = XHCI.lock();
        if !x.ready { return Err("not ready"); }
        (x.db, crate::arch::x86_64::phys::virt_to_phys_full(x.cmd_va))
    };
    let _ = cmd_pa_base;
    let (addr, cycle) = unsafe {
        let q = &raw mut CMDQ;
        if (*q).ring.va == 0 {
            /* First use: carve the command ring from the preallocated page. */
            let x = XHCI.lock();
            (*q).ring = Ring { va: x.cmd_va, n_trbs: 255, prod: 0, prod_cycle: 1, cons: 0, cons_cycle: 1 };
            (*q).ev_cons = 0;
            (*q).ev_cycle = 1;
        }
        ring_produce(&mut (*q).ring)
    };
    write_trb(addr, param, 0, ctrl_nocycle | (cycle as u32));
    core::sync::atomic::compiler_fence(Ordering::SeqCst);
    w32(db, 0, 0); /* HC doorbell */
    Ok(())
}

/* Enable Slot; returns the assigned slot id (1..255). */
fn cmd_enable_slot() -> Result<u8, &'static str> {
    submit_cmd(0, trb_ctrl(TRB_ENABLE_SLOT, 0) & !1 | 1)?;
    /* First command: cycle starts at 1; submit_cmd handles it. */
    loop {
        let (typ, _param, status, ctrl) = wait_event(5000)?;
        if typ == EVT_CMD_COMPLETE {
            let cc = (status >> 24) & 0xff;
            if cc != 1 {
                crate::println!("xhci: enable slot failed cc={}", cc);
                return Err("enable slot");
            }
            let slot = ((ctrl >> 24) & 0xff) as u8;
            if slot == 0 {
                return Err("no slot");
            }
            return Ok(slot);
        }
    }
}

/* ---- USB enumeration (control transfers, descriptors) ---- */
/* Input Context (4K): control(32B) + slot(32B) + EP contexts. */
fn write_input_context(va: u64, add_flags: u32, speed: u8, mps: u16, tr_pa: u64, route: u32, rh_port: u8) {
    unsafe {
        let p = va as *mut u32;
        /* Input Control Context: DW0 = Drop (0), DW1 = Add flags. */
        p.write_volatile(0);
        p.add(1).write_volatile(add_flags);
        for i in 2..8 { p.add(i).write_volatile(0); }
        /* Slot Context (offset 32): DW0 = route + speed + entries,
           DW1 = root hub port. Context Entries=1 (Slot + EP0 only). */
        let s = (va + 32) as *mut u32;
        s.write_volatile((route & 0xfffff) | ((speed as u32) << 20) | (1 << 27));
        s.add(1).write_volatile((rh_port as u32) << 16);
        for i in 2..8 { s.add(i).write_volatile(0); }
        /* EP0 Context (offset 64): DW0 mostly zero (disabled state,
           Interval 0 for control), DW1 = CErr/Type/Burst/MPS, DW2-3 =
           TR dequeue pointer + DCS=1. */
        let e = (va + 64) as *mut u32;
        e.write_volatile(0);
        e.add(1).write_volatile((3 << 1) | (4 << 3) | ((mps as u32) << 16));
        let dp = (tr_pa & !0xf) | 1;
        e.add(2).write_volatile((dp & 0xffff_ffff) as u32);
        e.add(3).write_volatile((dp >> 32) as u32);
        for i in 4..8 { e.add(i).write_volatile(0); }
    }
}

/* Address Device (BSR=1: configure EP0 only; BSR=0: assign address). */
fn cmd_address(slot: u8, ctx_pa: u64, bsr: bool) -> Result<(), &'static str> {
    let mut ctrl = (TRB_ADDR_DEV << 10) | 1; /* cycle added by submit */
    if bsr { ctrl |= 1 << 9; }
    ctrl |= (slot as u32) << 24;
    submit_cmd(ctx_pa, ctrl & !1)?;
    loop {
        let (typ, _param, status, _ctrl) = wait_event(1000)?;
        if typ == EVT_CMD_COMPLETE {
            let cc = (status >> 24) & 0xff;
            if cc != 1 {
                crate::println!("xhci: address dev failed cc={}", cc);
                return Err("address device");
            }
            return Ok(());
        }
    }
}

/* Transfer ring per endpoint (4K page, last slot is Link). */
struct EpRing {
    va: u64,
    prod: usize,
    cycle: u8,
}

/* Queue a control transfer on EP0 (DCI=1) of `slot`: setup + optional data
   + status. Single outstanding; waits for the status-stage event. */
fn control_xfer(slot: u8, ring: &mut EpRing, setup: &[u8; 8], data_va: u64, data_len: usize, dir_in: bool) -> Result<(), &'static str> {
    let db = {
        let x = XHCI.lock();
        if !x.ready { return Err("not ready"); }
        x.db
    };
    /* Setup stage TRB. */
    let (saddr, scycle) = {
        let a = trb_addr(ring.va, ring.prod);
        let c = ring.cycle;
        ring.prod += 1;
        if ring.prod >= 255 {
            let link = trb_addr(ring.va, 255);
            let pa = crate::arch::x86_64::phys::virt_to_phys_full(ring.va);
            ring.cycle ^= 1;
            let tc = ring.cycle;
            unsafe {
                (link as *mut u64).write_volatile(pa);
                (link as *mut u32).add(3).write_volatile((TRB_LINK << 10) | ((tc as u32) << 1) | (tc as u32));
            }
            ring.prod = 0;
        }
        (a, c)
    };
    let trt: u32 = if data_len == 0 { 0 } else if dir_in { 3 } else { 2 };
    unsafe {
        let p = saddr as *mut u64;
        let sp = setup.as_ptr() as *const u64;
        p.write_volatile(sp.read_volatile());
        (saddr as *mut u32).add(2).write_volatile(8); /* length */
        /* IDT=1 (setup bytes are immediate, not a pointer), TRT per direction.
           No IOC here: the status stage carries the completion event. */
        (saddr as *mut u32).add(3).write_volatile((TRB_SETUP << 10) | (scycle as u32) | (1 << 6) | (trt << 16));
    }
    if data_len > 0 {
        let (daddr, dcycle) = {
            let a = trb_addr(ring.va, ring.prod);
            let c = ring.cycle;
            ring.prod += 1;
            if ring.prod >= 255 {
                let link = trb_addr(ring.va, 255);
                let pa = crate::arch::x86_64::phys::virt_to_phys_full(ring.va);
                ring.cycle ^= 1;
                let tc = ring.cycle;
                unsafe {
                    (link as *mut u64).write_volatile(pa);
                    (link as *mut u32).add(3).write_volatile((TRB_LINK << 10) | ((tc as u32) << 1) | (tc as u32));
                }
                ring.prod = 0;
            }
            (a, c)
        };
        let dpa = crate::arch::x86_64::phys::virt_to_phys_full(data_va);
        if dpa == 0 { return Err("unmapped data"); }
        /* Match OVMF/EDK2 bit-for-bit (ISP + IOC on data stage):
           QEMU's xhci model stalls without them. */
        unsafe {
            (daddr as *mut u64).write_volatile(dpa);
            (daddr as *mut u32).add(2).write_volatile(data_len as u32);
            let mut c = (TRB_DATA << 10) | (dcycle as u32) | (1 << 2) | (1 << 5);
            if dir_in { c |= 1 << 16; }
            (daddr as *mut u32).add(3).write_volatile(c);
        }
    }
    /* Status stage TRB (opposite direction, IOC). */
    let (taddr, tcycle) = {
        let a = trb_addr(ring.va, ring.prod);
        let c = ring.cycle;
        ring.prod += 1;
        if ring.prod >= 255 {
            let link = trb_addr(ring.va, 255);
            let pa = crate::arch::x86_64::phys::virt_to_phys_full(ring.va);
            ring.cycle ^= 1;
            let tc = ring.cycle;
            unsafe {
                (link as *mut u64).write_volatile(pa);
                (link as *mut u32).add(3).write_volatile((TRB_LINK << 10) | ((tc as u32) << 1) | (tc as u32));
            }
            ring.prod = 0;
        }
        (a, c)
    };
    unsafe {
        (taddr as *mut u64).write_volatile(0);
        (taddr as *mut u32).add(2).write_volatile(0);
        let mut c = (TRB_STATUS << 10) | (tcycle as u32) | (1 << 5); /* IOC */
        /* USB control status direction: IN (device sends zero packet) unless
           the data stage was IN (then status is OUT). No-data control ends
           with status IN. */
        if data_len>0 && dir_in {
            /* Status OUT (host-to-device) for IN data. */
        } else {
            c |= 1 << 16; /* Status IN */
        }
        (taddr as *mut u32).add(3).write_volatile(c);
    }
    core::sync::atomic::compiler_fence(Ordering::SeqCst);
    /* Doorbell: slot, DCI=1 (EP0 control). */
    w32(db, (slot as u64) * 4, 1);
    /* Wait for the STATUS-stage Transfer Event. The data stage also has IOC
       (QEMU needs it), so its event arrives first and must not be mistaken
       for completion; match by TRB pointer (identity-mapped DMA). A stale
       status event from a previous transfer would have been consumed already
       because every transfer drains exactly its own status event. */
    let status_pa = crate::arch::x86_64::phys::virt_to_phys_full(taddr);
    loop {
        let (typ, param, status, _ctrl) = wait_event(1000)?;
        if typ == EVT_TRANSFER {
            let cc = (status >> 24) & 0xff;
            if param == status_pa {
                if cc != 1 {
                    crate::println!("xhci: control xfer failed cc={}", cc);
                    return Err("control xfer");
                }
                return Ok(());
            } else if cc != 1 {
                crate::println!("xhci: control xfer data failed cc={}", cc);
                return Err("control xfer");
            }
            /* Data/setup stage success: keep waiting for status. */
        }
    }
}

/* GET_DESCRIPTOR via EP0. desc_type: 1=device, 2=config, 3=string. */
fn get_descriptor(slot: u8, ring: &mut EpRing, desc_type: u8, index: u8, out_va: u64, out_len: usize) -> Result<(), &'static str> {
    let setup: [u8; 8] = [0x80, 6, index, desc_type, 0, 0, (out_len & 0xff) as u8, ((out_len >> 8) & 0xff) as u8];
    /* Data stage buffer must be DMA-safe: caller passes coherent memory. */
    control_xfer(slot, ring, &setup, out_va, out_len, true)
}

/* ---- port reset + device enumeration ---- */
fn port_reset(op: u64, port: u32) -> Result<u8, &'static str> {
    let p = PORT_BASE + ((port - 1) as u64) * PORT_STRIDE;
    /* Power on, clear change bits. */
    let mut sc = r32(op, p + R_P_SC);
    sc |= P_PP;
    w32(op, p + R_P_SC, sc);
    w32(op, p + R_P_SC, r32(op, p + R_P_SC) | P_PRC);
    /* Assert reset. */
    let mut sc = r32(op, p + R_P_SC);
    sc |= P_PR;
    w32(op, p + R_P_SC, sc);
    /* Wait for reset complete + enabled (up to 2s; QEMU is instant). */
    let deadline = crate::arch::x86_64::apic::ticks().wrapping_add(500 + 2);
    let had_if = was_if();
    let mut waited = 0u64;
    loop {
        let sc = r32(op, p + R_P_SC);
        if sc & P_PR == 0 && sc & P_PED != 0 { break; }
        wait_pause(&mut waited, had_if);
        if crate::arch::x86_64::apic::ticks().wrapping_sub(deadline) < 0x8000_0000 {
            crate::println!("xhci: port{} reset timeout sc={:#x}", port, sc);
            return Err("port reset");
        }
    }
    w32(op, p + R_P_SC, r32(op, p + R_P_SC) | P_PRC);
    /* USB reset recovery: the device needs ~10ms+ before it answers at
       Default. Reset twice with settle time: the first reset clears any
       prior (firmware) configuration, the second guarantees Default. */
    for _ in 0..2 {
        w32(op, p + R_P_SC, r32(op, p + R_P_SC) | P_PR);
        let deadline = crate::arch::x86_64::apic::ticks().wrapping_add(500 + 2);
        let had_if = was_if();
        let mut waited = 0u64;
        loop {
            let sc = r32(op, p + R_P_SC);
            if sc & P_PR == 0 && sc & P_PED != 0 { break; }
            wait_pause(&mut waited, had_if);
            if crate::arch::x86_64::apic::ticks().wrapping_sub(deadline) < 0x8000_0000 {
                return Err("port reset");
            }
        }
        w32(op, p + R_P_SC, r32(op, p + R_P_SC) | P_PRC);
        let t0 = crate::arch::x86_64::tsc::rdtsc();
        let hz = crate::time::tsc_hz();
        let wait = if hz == 0 { 10_000_000 } else { hz / 5 };
        while crate::arch::x86_64::tsc::rdtsc().wrapping_sub(t0) < wait {
            core::hint::spin_loop();
        }
    }
    let sc = r32(op, p + R_P_SC);
    let speed = ((sc >> 10) & 0xf) as u8;
    crate::println!("xhci: port{} reset ok sc={:#x} speed={}", port, sc, speed);
    Ok(speed)
}

/* Enumerate one connected port: reset, Enable Slot, Address Device
   (BSR=1/MPS=8, then BSR=0/correct MPS), full device descriptor. */
fn enumerate_port(port: u32) -> Result<(), &'static str> {
    let op = {
        let x = XHCI.lock();
        if !x.ready { return Err("not ready"); }
        x.op
    };
    let _p = PORT_BASE + ((port - 1) as u64) * PORT_STRIDE;
    let speed = port_reset(op, port)?;
    if speed != 1 && speed != 2 && speed != 3 {
        crate::println!("xhci: port{} unsupported speed {}", port, speed);
        return Err("bad speed");
    }
    let slot = cmd_enable_slot()?;
    crate::println!("xhci: port{} slot={}", port, slot);
    /* Input context + EP0 transfer ring. */
    let (ctx_va, ctx_pa) = crate::dma::alloc_coherent(4096).ok_or("ctx alloc")?;
    let (tr_va, _tr_pa) = crate::dma::alloc_coherent(4096).ok_or("tr alloc")?;
    let (buf_va, _buf_pa) = crate::dma::alloc_coherent(4096).ok_or("buf alloc")?;
    unsafe { core::ptr::write_bytes(tr_va as *mut u8, 0, 4096) };
    /* Link TRB at the end of the EP0 ring (cycle 0 initial; producer starts 1,
       first wrap toggles — matches the ring helper below). */
    unsafe {
        let link = (tr_va + 4096 - 16) as *mut u64;
        let pa = crate::arch::x86_64::phys::virt_to_phys_full(tr_va);
        link.write_volatile(pa);
        (link as *mut u32).add(3).write_volatile((TRB_LINK << 10) | (1 << 1) | 0);
    }
    let mut epring = EpRing { va: tr_va, prod: 0, cycle: 1 };
    /* Step 1: configure EP0 (BSR=1, no address assigned yet).
       Initial MPS from port speed: FS/LS use 8, HS uses 64. */
    let init_mps: u16 = if speed == 3 { 64 } else { 8 };
    write_input_context(ctx_va, (1 << 0) | (1 << 1), speed, init_mps, crate::arch::x86_64::phys::virt_to_phys_full(tr_va), 0, port as u8);
    cmd_address(slot, ctx_pa, true)?;
    /* Step 2: first 8 bytes of the device descriptor -> real MPS. */
    get_descriptor(slot, &mut epring, 1, 0, buf_va, 8)?;
    let mps = unsafe { *(buf_va as *const u8).add(7) };
    if mps != 8 && mps != 16 && mps != 32 && mps != 64 {
        crate::println!("xhci: port{} bad mps {}", port, mps);
        return Err("bad mps");
    }
    /* Step 3: assign address with the correct MPS (BSR=0). */
    write_input_context(ctx_va, (1 << 0) | (1 << 1), speed, mps as u16, crate::arch::x86_64::phys::virt_to_phys_full(tr_va), 0, port as u8);
    cmd_address(slot, ctx_pa, false)?;
    /* Step 4: full 18-byte device descriptor. */
    get_descriptor(slot, &mut epring, 1, 0, buf_va, 18)?;
    let (vid, pid, ncfg) = unsafe {
        let b = buf_va as *const u8;
        let v = u16::from_le_bytes([*b.add(8), *b.add(9)]);
        let p = u16::from_le_bytes([*b.add(10), *b.add(11)]);
        let c = *b.add(17);
        (v, p, c)
    };
    crate::println!("xhci: port{} slot={} usb {:04x}:{:04x} configs={}", port, slot, vid, pid, ncfg);
    /* Step 5: HID class setup (best-effort; failure leaves EP0 enumeration
       intact but without interrupt input). Rings/buffers are retained for
       the HID poller; ctx is single-use per command and freed. */
    let hid = hid_setup(port, slot, speed, &mut epring, buf_va, ncfg, vid, pid);
    crate::dma::free_coherent(ctx_va, ctx_pa, 4096);
    match hid {
        Ok(dev) => {
            let mut idx_opt: Option<usize> = None;
            unsafe {
                let hids = &raw mut HIDS;
                for (i, slot_opt) in (*hids).iter_mut().enumerate() {
                    if slot_opt.is_none() { *slot_opt = Some(dev); idx_opt = Some(i); break; }
                }
            }
            /* buf_va/tr_va retained by HidDev (no free). evdev nodes are
               pre-created by init_devtmpfs (which runs after us and would
               otherwise wipe dynamic nodes on initramfs mount). */
            if let Some(idx) = idx_opt {
                crate::println!("xhci: port{} evdev /dev/input/event{} ready", port, idx);
            }
        },
        Err(e) => {
            crate::println!("xhci: port{} hid setup skipped: {}", port, e);
            crate::dma::free_coherent(tr_va, crate::arch::x86_64::phys::virt_to_phys_full(tr_va), 4096);
            crate::dma::free_coherent(buf_va, crate::arch::x86_64::phys::virt_to_phys_full(buf_va), 4096);
        },
    }
    Ok(())
}

/* ================= Stage 3: HID (keyboard/mouse via interrupt IN) ========= */
/* Device state retained after enumeration. EP0 ring/buffers stay mapped for
   the poller's control needs; interrupt ring/buffer are dedicated. */
pub struct HidDev {
    pub slot: u8,
    pub port: u8,
    pub speed: u8,
    pub vid: u16,
    pub pid: u16,
    pub ep0_va: u64,
    pub ep0_ring: EpRing,
    pub buf_va: u64,
    pub int_ep: u8,
    pub int_dci: u8,
    pub int_va: u64,
    pub int_ring: EpRing,
    pub int_buf_va: u64,
    pub int_maxp: usize,
    pub is_mouse: bool,
    pub int_pending: bool,
    pub int_trb_pa: u64,
    pub prev_mods: u8,
    pub prev_keys: [u8; 6],
    pub prev_buttons: u8,
    pub evq: [u8; 1536],
    pub evq_len: usize,
}
const MAX_HID: usize = 4;
static mut HIDS: [Option<HidDev>; MAX_HID] = [None, None, None, None];

/* Configure Endpoint command (type 12): activates EPs after Address Device.
   Input context carries Slot + the NEW EP only (EP0 omitted so its live TR
   pointer is preserved). */
fn cmd_configure(slot: u8, ctx_pa: u64) -> Result<(), &'static str> {
    let ctrl = (TRB_CONFIG_EP << 10) | 1 | ((slot as u32) << 24);
    submit_cmd(ctx_pa, ctrl & !1)?;
    loop {
        let (typ, _param, status, _ctrl) = wait_event(1000)?;
        if typ == EVT_CMD_COMPLETE {
            let cc = (status >> 24) & 0xff;
            if cc != 1 {
                crate::println!("xhci: configure failed cc={}", cc);
                return Err("configure endpoint");
            }
            return Ok(());
        }
    }
}

/* SET_CONFIGURATION (no data; status IN). */
fn set_configuration(slot: u8, ring: &mut EpRing, config: u8) -> Result<(), &'static str> {
    let setup: [u8; 8] = [0x00, 9, config, 0, 0, 0, 0, 0];
    control_xfer(slot, ring, &setup, 0, 0, false)
}
/* HID SET_PROTOCOL (boot/report) and SET_IDLE. */
fn hid_set_protocol(slot: u8, ring: &mut EpRing, iface: u8, boot: bool) -> Result<(), &'static str> {
    let setup: [u8; 8] = [0x21, 0x0B, if boot { 0 } else { 1 }, 0, iface, 0, 0, 0];
    control_xfer(slot, ring, &setup, 0, 0, false)
}
fn hid_set_idle(slot: u8, ring: &mut EpRing, iface: u8) -> Result<(), &'static str> {
    let setup: [u8; 8] = [0x21, 0x0A, 0, 0, iface, 0, 0, 0];
    control_xfer(slot, ring, &setup, 0, 0, false)
}

/* Full HID setup after device-descriptor enumeration. Returns a retained
   device (rings/buffers stay mapped) or an error (caller frees EP0 ring). */
fn hid_setup(port: u32, slot: u8, speed: u8, epring: &mut EpRing, buf_va: u64, _ncfg: u8, vid: u16, pid: u16) -> Result<HidDev, &'static str> {
    /* Config descriptor header -> total length. */
    get_descriptor(slot, epring, 2, 0, buf_va, 9)?;
    let total = unsafe {
        let b = buf_va as *const u8;
        u16::from_le_bytes([*b.add(2), *b.add(3)]) as usize
    };
    if total < 9 || total > 512 {
        return Err("bad config total");
    }
    get_descriptor(slot, epring, 2, 0, buf_va, total)?;
    /* Walk: interface (class 3) -> endpoint (interrupt IN). */
    let bytes = unsafe { core::slice::from_raw_parts(buf_va as *const u8, total) };
    let mut off = 0usize;
    let mut iface: Option<u8> = None;
    let mut proto: u8 = 0;
    let mut ep_addr: u8 = 0;
    let mut ep_maxp: u16 = 0;
    let mut ep_interval: u8 = 0;
    while off + 2 <= total {
        let len = bytes[off] as usize;
        let dtype = bytes[off + 1];
        if len < 2 || off + len > total { break; }
        if dtype == 4 && len >= 9 {
            /* Interface: class/sub/proto. */
            if bytes[off + 5] == 3 {
                iface = Some(bytes[off + 2]);
                proto = bytes[off + 7];
            }
        } else if dtype == 5 && len >= 7 && iface.is_some() {
            /* Endpoint: address/attrs/maxp/interval. */
            let addr = bytes[off + 2];
            let attrs = bytes[off + 3];
            if addr & 0x80 != 0 && attrs & 3 == 3 {
                ep_addr = addr & 0x0f;
                ep_maxp = u16::from_le_bytes([bytes[off + 4], bytes[off + 5]]);
                ep_interval = bytes[off + 6];
                break;
            }
        }
        if len == 0 { break; }
        off += len;
    }
    let iface = iface.ok_or("no hid iface")?;
    if ep_addr == 0 { return Err("no int endpoint"); }
    let is_mouse = proto == 2;
    crate::println!("xhci: port{} hid iface={} proto={} ep={} maxp={:#x} int={}",
        port, iface, proto, ep_addr, ep_maxp, ep_interval);
    /* Config descriptor's bConfigurationValue is at offset 5. */
    let cfg_val = bytes[5];
    set_configuration(slot, epring, cfg_val)?;
    /* Boot protocol + no idle repeat (change-only reports). Best-effort:
       QEMU HID accepts both; failure is non-fatal for polling. */
    let _ = hid_set_protocol(slot, epring, iface, true);
    let _ = hid_set_idle(slot, epring, iface);
    /* Interrupt IN ring + buffer (retained). */
    let (int_va, _int_pa) = crate::dma::alloc_coherent(4096).ok_or("int ring alloc")?;
    let (int_buf_va, _int_buf_pa) = crate::dma::alloc_coherent(4096).ok_or("int buf alloc")?;
    unsafe {
        let link = (int_va + 4096 - 16) as *mut u64;
        let pa = crate::arch::x86_64::phys::virt_to_phys_full(int_va);
        link.write_volatile(pa);
        (link as *mut u32).add(3).write_volatile((TRB_LINK << 10) | (1 << 1) | 0);
    }
    let int_ring = EpRing { va: int_va, prod: 0, cycle: 1 };
    let dci = ep_addr * 2 + 1;
    let maxp_size = (ep_maxp & 0x7ff) as u16;
    let maxp_size = if maxp_size == 0 || maxp_size > 64 { 8 } else { maxp_size };
    let mult = ((ep_maxp >> 11) & 3) as u32;
    let mut interval = ep_interval;
    if interval == 0 || interval > 16 { interval = 10; }
    /* Input context: Slot (Entries = highest DCI) + new EP (Type 7 = Int IN). */
    let (ctx2_va, ctx2_pa) = crate::dma::alloc_coherent(4096).ok_or("ctx2 alloc")?;
    unsafe {
        let p = ctx2_va as *mut u32;
        p.write_volatile(0);
        p.add(1).write_volatile((1 << 0) | (1 << dci));
        for i in 2..8 { p.add(i).write_volatile(0); }
        let s = (ctx2_va + 32) as *mut u32;
        s.write_volatile(((dci as u32) << 27) | ((speed as u32) << 20));
        s.add(1).write_volatile((port as u32) << 16);
        for i in 2..8 { s.add(i).write_volatile(0); }
        let e = (ctx2_va + 32 + (dci as u64) * 32) as *mut u32;
        e.write_volatile(((interval as u32) << 16) | (mult << 8));
        e.add(1).write_volatile((3 << 1) | (7 << 3) | ((maxp_size as u32) << 16));
        let dp = (crate::arch::x86_64::phys::virt_to_phys_full(int_va) & !0xf) | 1;
        e.add(2).write_volatile((dp & 0xffff_ffff) as u32);
        e.add(3).write_volatile((dp >> 32) as u32);
        for i in 4..8 { e.add(i).write_volatile(0); }
    }
    cmd_configure(slot, ctx2_pa)?;
    crate::dma::free_coherent(ctx2_va, ctx2_pa, 4096);
    crate::println!("xhci: port{} hid {} ready dci={}", port, if is_mouse { "mouse" } else { "kbd" }, dci);
    Ok(HidDev {
        slot, port: port as u8, speed,
        vid, pid,
        ep0_va: epring.va, ep0_ring: EpRing { va: epring.va, prod: epring.prod, cycle: epring.cycle },
        buf_va,
        int_ep: ep_addr, int_dci: dci,
        int_va, int_ring,
        int_buf_va, int_maxp: maxp_size as usize,
        is_mouse,
        int_pending: false, int_trb_pa: 0,
        prev_mods: 0, prev_keys: [0; 6], prev_buttons: 0,
        evq: [0; 1536], evq_len: 0,
    })
}

/* Submit one interrupt-IN Normal TRB and ring the endpoint doorbell.
   Returns the TRB's physical address for event matching. */
fn submit_int_trb(dev: &mut HidDev, db: u64) -> Result<u64, &'static str> {
    let ring = &mut dev.int_ring;
    let idx = ring.prod;
    let cyc = ring.cycle;
    ring.prod += 1;
    if ring.prod >= 255 {
        let link = trb_addr(ring.va, 255);
        let pa = crate::arch::x86_64::phys::virt_to_phys_full(ring.va);
        ring.cycle ^= 1;
        let tc = ring.cycle;
        unsafe {
            (link as *mut u64).write_volatile(pa);
            (link as *mut u32).add(3).write_volatile((TRB_LINK << 10) | ((tc as u32) << 1) | (tc as u32));
        }
        ring.prod = 0;
    }
    let taddr = trb_addr(ring.va, idx);
    let dpa = crate::arch::x86_64::phys::virt_to_phys_full(dev.int_buf_va);
    if dpa == 0 { return Err("unmapped int buf"); }
    unsafe {
        (taddr as *mut u64).write_volatile(dpa);
        (taddr as *mut u32).add(2).write_volatile(dev.int_maxp as u32);
        /* Normal IN, ISP + IOC. */
        (taddr as *mut u32).add(3).write_volatile((TRB_NORMAL << 10) | (cyc as u32) | (1 << 2) | (1 << 5) | (1 << 16));
    }
    core::sync::atomic::compiler_fence(Ordering::SeqCst);
    w32(db, (dev.slot as u64) * 4, dev.int_dci as u32);
    Ok(crate::arch::x86_64::phys::virt_to_phys_full(taddr))
}

/* One poll round for all HID devices: ensure every idle device has a TRB
   outstanding, then a single bounded wait whose event is dispatched by TRB
   pointer. A single shared event ring means per-device waits would consume
   each other's completions; dispatching avoids that loss. */
fn hid_poll_round(db: u64, timeout_ms: u64) {

    unsafe {
        let hids = &raw mut HIDS;
        for dev_opt in (*hids).iter_mut().flatten() {
            if !dev_opt.int_pending {
                match submit_int_trb(dev_opt, db) {
                    Ok(pa) => { dev_opt.int_trb_pa = pa; dev_opt.int_pending = true; },
                    Err(e) => { crate::println!("hid: submit err {}", e); },
                }
            }
        }
    }
    match wait_event(timeout_ms) {
        Ok((typ, param, status, _)) => {
            if typ != EVT_TRANSFER { return; }
            let cc = (status >> 24) & 0xff;
            unsafe {
                let hids = &raw mut HIDS;
                for dev_opt in (*hids).iter_mut().flatten() {
                    if dev_opt.int_pending && param == dev_opt.int_trb_pa {
                        dev_opt.int_pending = false;
                        if cc != 1 {
                            /* Stall/halt: re-arm next round (drop this TD). */
                            break;
                        }
                        let residual = (status & 0xffffff) as usize;
                        /* Transfer Event Length is RESIDUAL (bytes NOT moved);
                           transferred = TRB length - residual. */
                        let n = dev_opt.int_maxp.saturating_sub(residual);
                        hid_report(dev_opt, n.min(dev_opt.int_maxp));
                        break;
                    }
                }
                /* Unmatched transfer event (e.g., a control status event
                   already handled elsewhere, or a stale completion): ignore. */
            }
        },
        Err(_) => {},
    }
}

/* Boot-protocol report parser: logs keypresses and mouse motion. Real
   evdev feed is the next step; serial proof validates the IRQ-less path. */
fn hid_report(dev: &HidDev, n: usize) {
    if n == 0 { return; }
    let b = unsafe { core::slice::from_raw_parts(dev.int_buf_va as *const u8, n.min(8)) };
    if dev.is_mouse {
        if n >= 3 {
            crate::println!("hid mouse{} buttons={:#x} dx={} dy={}", dev.port, b[0], b[1] as i8, b[2] as i8);
        }
    } else if n >= 8 {
        let mods = b[0];
        let mut keys = [0u8; 6];
        keys.copy_from_slice(&b[2..8]);
        if keys.iter().any(|&k| k != 0) || mods != 0 {
            crate::println!("hid kbd{} mods={:#x} keys={:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
                dev.port, mods, keys[0], keys[1], keys[2], keys[3], keys[4], keys[5]);
        }
    }
}

/* Background poller: round-robins HID devices with short waits. Runs as a
   kernel thread so QMP-injected keys are harvested without guest help. */
fn hid_poller() {
    let db = {
        let x = XHCI.lock();
        if !x.ready { return; }
        x.db
    };
    crate::println!("hid: poller started");
    let mut rounds = 0u32;
    loop {
        hid_poll_round(db, 20);
        rounds += 1;
        if rounds % 200 == 0 {
            unsafe {
                let q = &raw mut CMDQ;
                crate::println!("hid: alive rounds={} ev_cons={} ev_cycle={}", rounds, (*q).ev_cons, (*q).ev_cycle);
            }
        }
        /* Brief yield so the poller does not starve the timer tick. */
        crate::task::yield_now();
    }
}

pub fn hid_start_poller() {
    /* Spawned once after enumeration; idempotent by thread count. */
    static STARTED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) { return; }
    match crate::task::spawn_kernel(hid_poller) {
        Ok(_) => crate::println!("hid: poller spawned"),
        Err(e) => crate::println!("hid: poller spawn failed {}", e),
    }
}

/* ================= Stage 4: evdev (/dev/input/eventN, on-demand) ========== */
/* Linux input_event (64-bit): time[16] + type[2] + code[2] + value[4]. */
const EV_SYN: u16 = 0; const EV_KEY: u16 = 1; const EV_REL: u16 = 2;
const SYN_REPORT: u16 = 0;
const REL_X: u16 = 0; const REL_Y: u16 = 1; const REL_WHEEL: u16 = 8;
const BTN_LEFT: u16 = 0x110; const BTN_RIGHT: u16 = 0x111; const BTN_MIDDLE: u16 = 0x112;
/* Modifier bit -> Linux KEY_*. */
const MOD_KEYS: [u16; 8] = [29, 42, 56, 125, 97, 54, 100, 126];
/* HID boot keycode -> Linux KEY_*. Letters are non-sequential on Linux. */
fn hid_to_linux(h: u8) -> Option<u16> {
    Some(match h {
        0x04=>30, 0x05=>48, 0x06=>46, 0x07=>32, 0x08=>18, 0x09=>33, 0x0a=>34, 0x0b=>35,
        0x0c=>23, 0x0d=>36, 0x0e=>37, 0x0f=>38, 0x10=>50, 0x11=>49, 0x12=>24, 0x13=>25,
        0x14=>16, 0x15=>19, 0x16=>31, 0x17=>20, 0x18=>22, 0x19=>47, 0x1a=>17, 0x1b=>45,
        0x1c=>21, 0x1d=>44,
        0x1e=>2, 0x1f=>3, 0x20=>4, 0x21=>5, 0x22=>6, 0x23=>7, 0x24=>8, 0x25=>9, 0x26=>10, 0x27=>11,
        0x28=>28, 0x29=>1, 0x2a=>14, 0x2b=>15, 0x2c=>57, 0x2d=>12, 0x2e=>13, 0x2f=>26,
        0x30=>27, 0x31=>43, 0x33=>39, 0x34=>40, 0x35=>41, 0x36=>51, 0x37=>52, 0x38=>53,
        0x39=>58,
        0x3a=>59, 0x3b=>60, 0x3c=>61, 0x3d=>62, 0x3e=>63, 0x3f=>64, 0x40=>65, 0x41=>66,
        0x42=>67, 0x43=>68, 0x44=>87, 0x45=>88,
        0x4f=>106, 0x50=>105, 0x51=>108, 0x52=>103,
        _=>return None,
    })
}
fn ev_time() -> (u64, u64) {
    let ns = crate::time::realtime_ns();
    (ns / 1_000_000_000, (ns % 1_000_000_000) / 1000)
}
fn push_event(dev: &mut HidDev, ty: u16, code: u16, val: i32) {
    if dev.evq_len + 24 > dev.evq.len() { return; } /* drop on overflow */
    let (sec, usec) = ev_time();
    let o = dev.evq_len;
    dev.evq[o..o+8].copy_from_slice(&sec.to_ne_bytes());
    dev.evq[o+8..o+16].copy_from_slice(&usec.to_ne_bytes());
    dev.evq[o+16..o+18].copy_from_slice(&ty.to_ne_bytes());
    dev.evq[o+18..o+20].copy_from_slice(&code.to_ne_bytes());
    dev.evq[o+20..o+24].copy_from_slice(&val.to_ne_bytes());
    dev.evq_len += 24;
}
/* Converts a boot-protocol report into queued input_events (press/release +
   SYN). Keyboard tracks modifiers + 6 keys; mouse tracks buttons + motion. */
fn hid_parse_report(dev: &mut HidDev, n: usize) {
    if n == 0 { return; }
    let b = unsafe { core::slice::from_raw_parts(dev.int_buf_va as *const u8, n.min(8)) };
    /* Diagnostic: log non-empty reports (proves the IRQ-less poll path). */
    if b.iter().any(|&x| x != 0) {
        crate::println!("hid rpt{} len={} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
            dev.port, n, b[0], if n>1{b[1]}else{0}, if n>2{b[2]}else{0}, if n>3{b[3]}else{0},
            if n>4{b[4]}else{0}, if n>5{b[5]}else{0}, if n>6{b[6]}else{0}, if n>7{b[7]}else{0});
    }
    if dev.is_mouse {
        if n < 3 { return; }
        let btn = b[0];
        let changed = btn ^ dev.prev_buttons;
        for i in 0..3 {
            if changed & (1 << i) != 0 {
                let code = [BTN_LEFT, BTN_RIGHT, BTN_MIDDLE][i];
                push_event(dev, EV_KEY, code, if btn & (1 << i) != 0 { 1 } else { 0 });
            }
        }
        dev.prev_buttons = btn;
        let mut moved = false;
        if b[1] != 0 { push_event(dev, EV_REL, REL_X, b[1] as i8 as i32); moved = true; }
        if b[2] != 0 { push_event(dev, EV_REL, REL_Y, b[2] as i8 as i32); moved = true; }
        if n >= 4 && b[3] != 0 { push_event(dev, EV_REL, REL_WHEEL, b[3] as i8 as i32); moved = true; }
        if changed != 0 || moved {
            push_event(dev, EV_SYN, SYN_REPORT, 0);
        }
    } else if n >= 8 {
        let mods = b[0];
        let mchg = mods ^ dev.prev_mods;
        for i in 0..8 {
            if mchg & (1 << i) != 0 {
                push_event(dev, EV_KEY, MOD_KEYS[i], if mods & (1 << i) != 0 { 1 } else { 0 });
            }
        }
        dev.prev_mods = mods;
        let mut keys = [0u8; 6];
        keys.copy_from_slice(&b[2..8]);
        let prev = dev.prev_keys;
        let mut any = mchg != 0;
        for &pk in prev.iter() {
            if pk != 0 && !keys.contains(&pk) {
                if let Some(lc) = hid_to_linux(pk) { push_event(dev, EV_KEY, lc, 0); any = true; }
            }
        }
        for &k in keys.iter() {
            if k != 0 && !prev.contains(&k) {
                if let Some(lc) = hid_to_linux(k) { push_event(dev, EV_KEY, lc, 1); any = true; }
            }
        }
        dev.prev_keys = keys;
        if any {
            push_event(dev, EV_SYN, SYN_REPORT, 0);
        }
    }
}
/* On-demand interrupt poll for evdev read: submit if idle, single bounded
   wait, dispatch by TRB pointer (shared ring). Returns true if a report was
   harvested (parsed into the device event queue). */
fn hid_poll_once(dev: &mut HidDev, db: u64, timeout_ms: u64) -> bool {
    if !dev.int_pending {
        match submit_int_trb(dev, db) {
            Ok(pa) => { dev.int_trb_pa = pa; dev.int_pending = true; },
            Err(_) => return false,
        }
    }
    let want = dev.int_trb_pa;
    match wait_event(timeout_ms) {
        Ok((typ, param, status, _)) => {
            if typ != EVT_TRANSFER || param != want { return false; }
            dev.int_pending = false;
            let cc = (status >> 24) & 0xff;
            if cc != 1 { return false; }
            let residual = (status & 0xffffff) as usize;
            let n = dev.int_maxp.saturating_sub(residual);
            hid_parse_report(dev, n.min(dev.int_maxp));
            true
        },
        Err(_) => false,
    }
}
/* evdev read: drains queued input_events (whole 24-byte structs only);
   blocks (submit + wait loop) unless nonblock. EINTR on pending signal. */
pub fn evdev_read(idx: usize, dst: &mut [u8], nonblock: bool) -> Result<usize, i64> {
    if dst.len() < 24 { return Err(crate::errno::EINVAL); }
    let db = {
        let x = XHCI.lock();
        if !x.ready { return Err(crate::errno::ENODEV); }
        x.db
    };
    loop {
        let drained = unsafe {
            let hids = &raw mut HIDS;
            let dev = match (*hids).iter_mut().flatten().nth(idx) {
                Some(d) => d,
                None => return Err(crate::errno::ENODEV),
            };
            let n = core::cmp::min(dst.len() / 24 * 24, dev.evq_len);
            if n > 0 {
                dst[..n].copy_from_slice(&dev.evq[..n]);
                dev.evq.copy_within(n.., 0);
                dev.evq_len -= n;
            }
            n
        };
        if drained > 0 { return Ok(drained); }
        if nonblock { return Err(crate::errno::EAGAIN); }
        if crate::task::has_deliverable_signal() { return Err(crate::errno::EINTR); }
        let got = unsafe {
            let hids = &raw mut HIDS;
            match (*hids).iter_mut().flatten().nth(idx) {
                Some(dev) => hid_poll_once(dev, db, 100),
                None => return Err(crate::errno::ENODEV),
            }
        };
        if !got {
            /* Timeout with nothing harvested: re-check signal, else keep
               waiting (blocking read is indefinite like Linux). */
            if crate::task::has_deliverable_signal() { return Err(crate::errno::EINTR); }
        }
    }
}
/* Minimal evdev ioctls for libevdev/X: VERSION, ID, NAME, GBIT, GKEY. */
pub fn evdev_ioctl(idx: usize, cmd: u64, arg: u64) -> i64 {
    let is_mouse = unsafe {
        match (*&raw const HIDS).iter().flatten().nth(idx) {
            Some(d) => d.is_mouse,
            None => return crate::errno::neg(crate::errno::ENODEV),
        }
    };
    let vidpid = unsafe {
        match (*&raw const HIDS).iter().flatten().nth(idx) {
            Some(d) => (d.vid, d.pid),
            None => return crate::errno::neg(crate::errno::ENODEV),
        }
    };
    match cmd {
        0x80044501 => { /* EVIOCGVERSION */
            if arg == 0 { return crate::errno::neg(crate::errno::EFAULT); }
            let v = 0x010001u32;
            if crate::usercopy::copy_to_user(arg as usize, &v.to_ne_bytes()).is_err() { return crate::errno::neg(crate::errno::EFAULT); }
            0
        },
        0x80084502 => { /* EVIOCGID: bustype/vendor/product/version */
            if arg == 0 { return crate::errno::neg(crate::errno::EFAULT); }
            let mut id = [0u8; 8];
            id[0..2].copy_from_slice(&3u16.to_ne_bytes()); /* BUS_USB */
            id[2..4].copy_from_slice(&vidpid.0.to_ne_bytes());
            id[4..6].copy_from_slice(&vidpid.1.to_ne_bytes());
            id[6..8].copy_from_slice(&0x0100u16.to_ne_bytes());
            if crate::usercopy::copy_to_user(arg as usize, &id).is_err() { return crate::errno::neg(crate::errno::EFAULT); }
            0
        },
        c if (c >> 8) as u8 == b'E' && (c & 0xff) == 0x06 => { /* EVIOCGNAME(len) */
            let len = ((c >> 16) & 0x3fff) as usize;
            if arg == 0 || len == 0 { return crate::errno::neg(crate::errno::EFAULT); }
            let name: &[u8] = if is_mouse { b"Slop HID Mouse\0" } else { b"Slop HID Keyboard\0" };
            let n = core::cmp::min(len, name.len());
            if crate::usercopy::copy_to_user(arg as usize, &name[..n]).is_err() { return crate::errno::neg(crate::errno::EFAULT); }
            n as i64
        },
        c if (c >> 8) as u8 == b'E' && (c & 0xff) >= 0x20 && (c & 0xff) < 0x40 => { /* EVIOCGBIT(ev,len) */
            let ev = (c & 0xff) as u8 - 0x20;
            let len = ((c >> 16) & 0x3fff) as usize;
            if arg == 0 || len == 0 { return crate::errno::neg(crate::errno::EFAULT); }
            let mut bits = [0u8; 96];
            let blen = match ev {
                0 => { /* EV types: SYN + KEY (+ REL for mouse) */
                    bits[0] = 0x02 | if is_mouse { 0x04 } else { 0 };
                    8
                },
                1 => { /* KEY: modifiers + mapped keys (kbd) or buttons (mouse) */
                    if is_mouse {
                        bits[0x110 / 8] |= 0x07; /* BTN_LEFT/RIGHT/MIDDLE */
                    } else {
                        for &k in MOD_KEYS.iter() { bits[(k / 8) as usize] |= 1 << (k % 8); }
                        for h in 0x04..=0x52u8 {
                            if let Some(lc) = hid_to_linux(h) { bits[(lc / 8) as usize] |= 1 << (lc % 8); }
                        }
                    }
                    96
                },
                2 => { /* REL: X/Y/WHEEL (mouse only) */
                    if is_mouse { bits[0] = 0x0b | 0x01 | 0x02; }
                    8
                },
                _ => 0,
            };
            if blen == 0 { return crate::errno::neg(crate::errno::EINVAL); }
            let n = core::cmp::min(len, blen);
            if crate::usercopy::copy_to_user(arg as usize, &bits[..n]).is_err() { return crate::errno::neg(crate::errno::EFAULT); }
            n as i64
        },
        _ => 0,
    }
}
