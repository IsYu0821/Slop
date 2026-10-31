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
/* Runtime registers (BAR0+rtsoff): interrupter 0. */
const R_IMAN: u64 = 0x00;
const R_IMOD: u64 = 0x04;
const R_ERSTSZ: u64 = 0x08;
const R_ERSTBA: u64 = 0x10;
const R_ERDP: u64 = 0x18;
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

fn wait_op(bar: u64, op: u64, off: u64, mask: u32, want: u32, timeout_ms: u64) -> Result<(), &'static str> {
    let _ = bar;
    let deadline = crate::arch::x86_64::apic::ticks().wrapping_add(timeout_ms / 4 + 2);
    loop {
        if r32(op, off) & mask == want { return Ok(()); }
        core::hint::spin_loop();
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
    /* Run. */
    let mut cmd = r32(op, R_USBCMD);
    cmd |= CMD_RS;
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
    crate::println!("xhci: selftest PASS ports_with_devices={}", found);
}
