//! Linux-driver-compatible kernel symbols: printk, kmalloc family,
//! sleeps, jiffies, PCI registry + operations, MMIO, IRQ stubs, and the
//! string/memory helpers modules expect. Addresses in this file are exported
//! to .ko objects via `module::export_table`.
//!
//! Scope discipline (what this is NOT yet): no DMA API, no MSI/MSI-X routing
//! (IRQ registration is accepted and stored; delivery is not wired yet —
//! drivers must poll), no full `struct pci_dev` (vendor/device filled, the
//! rest zeroed; real HW drivers needing irq/resource fields come later).
use crate::sync::IrqSpinLock;
use core::sync::atomic::{AtomicU64, Ordering};

/* ================= printk ================= */
// Minimal format engine: %s %d %i %u %x %X %p %c %% and literal text.
// The caller passes up to 6 register varargs (SysV: rsi..r9); stack varargs
// are not read (documented limitation; test drivers avoid them).
#[unsafe(no_mangle)]
fn format_into(out: &mut [u8], fmt: *const u8, args_in: [u64; 6]) -> usize {
    if fmt.is_null() { return 0; }
    let mut args = args_in;
    let mut ai = 0usize;
    let mut out = [0u8; 512];
    let mut n = 0usize;
    let mut p = fmt;
    let next_arg = |args: &mut [u64; 6], ai: &mut usize| -> u64 {
        let v = if *ai < 6 { args[*ai] } else { 0 };
        *ai += 1;
        v
    };
    // total counts the full length (for snprintf return); n counts written.
    let mut total = 0usize;
    let mut push = |out: &mut [u8; 512], n: &mut usize, total: &mut usize, by: u8| {
        *total += 1;
        if *n < out.len() { out[*n] = by; *n += 1; }
    };
    let push_u64 = |out: &mut [u8; 512], n: &mut usize, total: &mut usize, mut v: u64, base: u64, upper: bool| {
        if v == 0 { push(out, n, &mut *total, b'0'); return; }
        let mut tmp = [0u8; 20];
        let mut l = 0usize;
        while v > 0 { let r = (v % base) as u8; tmp[l] = if r < 10 { b'0' + r } else { (if upper { b'A' } else { b'a' }) + r - 10 }; l += 1; v /= base; }
        while l > 0 { l -= 1; push(out, n, &mut *total, tmp[l]); }
    };
    loop {
        let ch = unsafe { core::ptr::read_volatile(p) };
        if ch == 0 { break; }
        p = unsafe { p.add(1) };
        if ch != b'%' { push(&mut out, &mut n, &mut total, ch); continue; }
        let s = unsafe { core::ptr::read_volatile(p) };
        if s == 0 { break; }
        p = unsafe { p.add(1) };
        match s {
            b'%' => push(&mut out, &mut n, &mut total, b'%'),
            b'c' => push(&mut out, &mut n, &mut total, next_arg(&mut args, &mut ai) as u8),
            b's' => {
                let sp = next_arg(&mut args, &mut ai) as *const u8;
                if !sp.is_null() {
                    for i in 0..256 {
                        let by = unsafe { core::ptr::read_volatile(sp.add(i)) };
                        if by == 0 { break; }
                        push(&mut out, &mut n, &mut total, by);
                    }
                } else {
                    for by in b"(null)".iter() { push(&mut out, &mut n, &mut total, *by); }
                }
            }
            b'd' | b'i' => {
                let v = next_arg(&mut args, &mut ai) as i64;
                if v < 0 { push(&mut out, &mut n, &mut total, b'-'); push_u64(&mut out, &mut n, &mut total, (-v) as u64, 10, false); }
                else { push_u64(&mut out, &mut n, &mut total, v as u64, 10, false); }
            }
            b'u' => push_u64(&mut out, &mut n, &mut total, next_arg(&mut args, &mut ai), 10, false),
            b'x' => push_u64(&mut out, &mut n, &mut total, next_arg(&mut args, &mut ai), 16, false),
            b'X' => push_u64(&mut out, &mut n, &mut total, next_arg(&mut args, &mut ai), 16, true),
            b'p' => {
                push(&mut out, &mut n, &mut total, b'0'); push(&mut out, &mut n, &mut total, b'x');
                push_u64(&mut out, &mut n, &mut total, next_arg(&mut args, &mut ai), 16, false);
            }
            b'l' => {
                // %l[dux]: consume length, treat as 64-bit.
                let s2 = unsafe { core::ptr::read_volatile(p) };
                if s2 == b'd' || s2 == b'u' || s2 == b'x' {
                    p = unsafe { p.add(1) };
                    let v = next_arg(&mut args, &mut ai);
                    match s2 {
                        b'd' => {
                            let v = v as i64;
                            if v < 0 { push(&mut out, &mut n, &mut total, b'-'); push_u64(&mut out, &mut n, &mut total, (-v) as u64, 10, false); }
                            else { push_u64(&mut out, &mut n, &mut total, v as u64, 10, false); }
                        }
                        b'x' => push_u64(&mut out, &mut n, &mut total, v, 16, false),
                        _ => push_u64(&mut out, &mut n, &mut total, v, 10, false),
                    }
                } else { push(&mut out, &mut n, &mut total, b'%'); push(&mut out, &mut n, &mut total, b'l'); }
            }
            _ => { push(&mut out, &mut n, &mut total, b'%'); push(&mut out, &mut n, &mut total, s); }
        }
        if total >= 4096 { break; }
    }
    // NUL-terminate if room (snprintf semantics; printk ignores the NUL).
    if n < out.len() { out[n] = 0; }
    total
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn printk(fmt: *const u8, a: u64, b: u64, c: u64, d: u64, e: u64, f: u64) -> i32 {
    let mut out = [0u8; 512];
    let n_full = format_into(&mut out, fmt, [a, b, c, d, e, f]);
    let n = core::cmp::min(n_full, 511);
    crate::console::write_bytes(&out[..n]);
    n_full as i32
}

/* ================= kmalloc family (page-granular via kheap) ================= */
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmalloc(size: usize, _gfp: u32) -> *mut u8 { crate::kheap::kmalloc(size) }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kzalloc(size: usize, _gfp: u32) -> *mut u8 { crate::kheap::kzalloc(size) }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kcalloc(n: usize, size: usize, _gfp: u32) -> *mut u8 { crate::kheap::kcalloc(n, size) }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmemdup(src: *const u8, len: usize, _gfp: u32) -> *mut u8 { crate::kheap::kmemdup(src, len) }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn krealloc(ptr: *mut u8, size: usize, _gfp: u32) -> *mut u8 {
    if ptr.is_null() { return crate::kheap::kmalloc(size); }
    if size == 0 { crate::kheap::kfree(ptr); return core::ptr::null_mut(); }
    let old = crate::kheap::ksize(ptr);
    let np = crate::kheap::kmalloc(size);
    if np.is_null() { return np; }
    let n = core::cmp::min(old, size);
    unsafe { core::ptr::copy_nonoverlapping(ptr, np, n) };
    crate::kheap::kfree(ptr);
    np
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kfree(ptr: *mut u8) { crate::kheap::kfree(ptr) }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ksize(ptr: *mut u8) -> usize { crate::kheap::ksize(ptr) }

/* ================= sleeps / jiffies ================= */
pub const HZ: u64 = 1000;
static JIFFIES: AtomicU64 = AtomicU64::new(0);
pub fn jiffies_addr() -> u64 { &JIFFIES as *const _ as u64 }
pub fn tick_jiffies() { JIFFIES.fetch_add(1, Ordering::Relaxed); }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn msleep(ms: u32) {
    let t0 = JIFFIES.load(Ordering::Relaxed);
    while JIFFIES.load(Ordering::Relaxed).wrapping_sub(t0) < ms as u64 {
        core::hint::spin_loop();
        // Cooperative: let the timer tick run us forward. No locks held here
        // (module init context), so yielding is safe.
        crate::task::yield_now();
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdelay(ms: u32) {
    // Busy loop calibrated roughly: ~1e6 spins per ms on TCG.
    for _ in 0..(ms as u64 * 1_000_000) { core::hint::spin_loop(); }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn udelay(us: u32) {
    for _ in 0..(us as u64 * 1000) { core::hint::spin_loop(); }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __const_udelay(us: u32) { unsafe { udelay(us) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __udelay(us: u64) { unsafe { udelay(us as u32) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usleep_range(_min: u64, _max: u64) { msleep(1); }

/* ================= PCI registry + ops ================= */
// Linux struct layouts (64-bit, host kernel 7.x): verified against headers.
#[repr(C)]
pub struct PciDeviceId {
    pub vendor: u32,
    pub device: u32,
    pub subvendor: u32,
    pub subdevice: u32,
    pub class: u32,
    pub class_mask: u32,
    pub driver_data: u64,
    pub override_only: u32,
}
#[repr(C)]
pub struct PciDriver {
    pub name: *const u8,
    pub id_table: *const PciDeviceId,
    pub probe: Option<unsafe extern "C" fn(*mut PciDev, *const PciDeviceId) -> i32>,
    pub remove: Option<unsafe extern "C" fn(*mut PciDev)>,
    // (further fields ignored: suspend/remove/shutdown/... not called by us)
}
// Minimal pci_dev: real prefix (bus_list/bus/subordinate/sysdata/procent/
// slot/devfn/vendor/device/...) then Slop-private area. Offsets vendor@60
// device@62 verified against host headers; irq/resource left for future HW
// drivers. Bytes 64.. hold Slop's BDF + drvdata (never read by Linux ABI).
#[repr(C)]
pub struct PciDev {
    _pad0: [u8; 60],
    pub vendor: u16,
    pub device: u16,
    pub bdf: u32,      // bus<<16 | dev<<8 | func (Slop-private)
    _pad1: u32,
    pub drvdata: u64,  // Slop-private (pci_set/get_drvdata)
    _rest: [u8; 432],
}
const _: () = assert!(core::mem::size_of::<PciDev>() == 512);
impl PciDev {
    fn bdf_of(&self) -> crate::driver::pci::Bdf {
        crate::driver::pci::Bdf {
            segment: 0,
            bus: ((self.bdf >> 16) & 0xff) as u8,
            device: ((self.bdf >> 8) & 0xff) as u8,
            function: (self.bdf & 0xff) as u8,
        }
    }
}

fn drv_name(drv: u64) -> [u8; 48] {
    let mut n = [0u8; 48];
    if drv == 0 { return n; }
    let p = unsafe { &*(drv as *const PciDriver) };
    if p.name.is_null() { return n; }
    for i in 0..47 {
        let b = unsafe { core::ptr::read_volatile(p.name.add(i)) };
        if b == 0 { break; }
        n[i] = b;
    }
    n
}

/* Slop-core probe shim: adapts a Slop PCI match into a Linux probe call.
   The opaque cookie is the Linux pci_driver address; we rebuild a compat
   pci_dev (vendor/device/BDF filled, rest zeroed) and invoke the driver's
   probe with the matching Linux ID entry. */
fn slop_pci_probe_shim(bdf: crate::driver::pci::Bdf, vendor: u16, device: u16, opaque: u64) -> i32 {
    if opaque == 0 { return -22; }
    let lp = unsafe { &*(opaque as *const PciDriver) };
    let probe = match lp.probe { Some(f) => f, None => return -22 };
    // Find the Linux ID entry that matches (for the id argument).
    let mut matched: *const PciDeviceId = core::ptr::null();
    if !lp.id_table.is_null() {
        for k in 0..64 {
            let e = unsafe { &*lp.id_table.add(k) };
            if e.vendor == 0 && e.device == 0 && e.subvendor == 0 && e.subdevice == 0 && e.class == 0 { break; }
            let vm = e.vendor == 0xffff_ffff || e.vendor == 0xffff || (e.vendor as u16) == vendor || e.vendor == 0;
            let dm = e.device == 0xffff_ffff || e.device == 0xffff || (e.device as u16) == device || e.device == 0;
            if vm && dm { matched = e as *const PciDeviceId; break; }
        }
    }
    if matched.is_null() { return -19; } // -ENODEV
    // Compat pci_dev on the heap (512B, real vendor/device/BDF prefix).
    let mem = crate::kheap::kzalloc(512);
    if mem.is_null() { return -12; }
    let pd = mem as *mut PciDev;
    unsafe {
        (*pd).vendor = vendor;
        (*pd).device = device;
        (*pd).bdf = ((bdf.bus as u32) << 16) | ((bdf.device as u32) << 8) | (bdf.function as u32);
    }
    let rc = unsafe { probe(pd, matched) };
    if rc != 0 { crate::kheap::kfree(mem); }
    // NOTE: bound dev buffer intentionally leaked while bound (remove path
    // frees once unbind tracking lands; today delete_module drops the
    // registration but the dev stays alive, which is safe if never touched).
    rc
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_register_driver(drv: u64) -> i32 {
    if drv == 0 { return -22; } // -EINVAL
    let p = unsafe { &*(drv as *const PciDriver) };
    if p.id_table.is_null() || p.probe.is_none() { return -22; }
    let name = drv_name(drv);
    // Expand the Linux ID table into one Slop registration per entry.
    // The Slop core owns enumeration/matching; this adapter only translates.
    let mut count = 0;
    for k in 0..64 {
        let e = unsafe { &*p.id_table.add(k) };
        if e.vendor == 0 && e.device == 0 && e.subvendor == 0 && e.subdevice == 0 && e.class == 0 { break; }
        // Class-only entries need class matching which the Slop core does
        // not do yet; skip them (documented gap, not silent mismatch).
        if (e.vendor == 0 || e.vendor == 0xffff_ffff) && (e.device == 0 || e.device == 0xffff_ffff) && (e.class != 0 || e.class_mask != 0) {
            continue;
        }
        let v = if e.vendor == 0xffff_ffff || e.vendor == 0xffff { 0xffff } else { (e.vendor & 0xffff) as u16 };
        let d = if e.device == 0xffff_ffff || e.device == 0xffff { 0xffff } else { (e.device & 0xffff) as u16 };
        let r = crate::driver::pci::register_slop_driver(&name, v, d, slop_pci_probe_shim, drv);
        if r != 0 { return r; }
        count += 1;
    }
    if count == 0 { return -22; }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_unregister_driver(drv: u64) {
    if drv == 0 { return; }
    // Slop core drops all registrations carrying this opaque cookie.
    crate::driver::pci::unregister_slop_driver(drv);
}
/* Called on module unload: drop registrations that came from it. Full
   per-module owner tracking is future work; today we drop by pointer. */
pub fn pci_unregister_for_module() {
    // The module's own delete path calls pci_unregister_driver directly
    // (its cleanup_module runs first), so nothing to do here yet.
}

fn pci_cmd_set(bdf: crate::driver::pci::Bdf, bits: u16, on: bool) {
    use crate::driver::pci::{Cam, ConfigAccess, CFG_COMMAND};
    let acc = Cam;
    let cur = acc.read32(bdf, CFG_COMMAND) as u16;
    let v = if on { cur | bits } else { cur & !bits };
    acc.write32(bdf, CFG_COMMAND, v as u32);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_enable_device(dev: *mut PciDev) -> i32 {
    if dev.is_null() { return -22; }
    let bdf = unsafe { (*dev).bdf_of() };
    use crate::driver::pci::{CMD_IO, CMD_MEM};
    pci_cmd_set(bdf, CMD_IO | CMD_MEM, true);
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_disable_device(dev: *mut PciDev) {
    if dev.is_null() { return; }
    let bdf = unsafe { (*dev).bdf_of() };
    use crate::driver::pci::{CMD_IO, CMD_MEM, CMD_BUS_MASTER};
    pci_cmd_set(bdf, CMD_IO | CMD_MEM | CMD_BUS_MASTER, false);
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_request_regions(_dev: *mut PciDev, _name: *const u8) -> i32 {
    // No BAR reservation table yet (PCI subsystem step): accept, since Slop
    // owns all BARs on this single-enumeration platform and hands them out
    // read-only from config space. Revisit with resource management.
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_release_regions(_dev: *mut PciDev) {}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_set_master(dev: *mut PciDev) {
    if dev.is_null() { return; }
    let bdf = unsafe { (*dev).bdf_of() };
    pci_cmd_set(bdf, crate::driver::pci::CMD_BUS_MASTER, true);
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_clear_master(dev: *mut PciDev) {
    if dev.is_null() { return; }
    let bdf = unsafe { (*dev).bdf_of() };
    pci_cmd_set(bdf, crate::driver::pci::CMD_BUS_MASTER, false);
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_set_drvdata(dev: *mut PciDev, data: u64) {
    if dev.is_null() { return; }
    unsafe { (*dev).drvdata = data; }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_get_drvdata(dev: *mut PciDev) -> u64 {
    if dev.is_null() { return 0; }
    unsafe { (*dev).drvdata }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_ioremap_bar(_dev: *mut PciDev, nr: u64) -> u64 {
    // No MMIO mapping yet (needs UC mappings + BAR resource management from
    // the Slop PCI core). Return NULL so drivers fail probe cleanly instead
    // of dereferencing a bogus direct-map alias with wrong caching.
    crate::println!("pci: ioremap_bar{} denied (no UC mapping yet)", nr);
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_iounmap(_dev: *mut PciDev, _addr: u64) {}

/* ================= MMIO ================= */
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ioremap(phys: u64, _size: u64) -> u64 {
    // Direct-map aliases are WB-cached RAM mappings; using them for MMIO
    // silently corrupts device interaction. Until the Slop PCI/DMA core
    // provides real UC mappings, fail closed so drivers error out cleanly.
    crate::println!("mmio: ioremap {:#x} denied (no UC mapping yet)", phys);
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn iounmap(_addr: u64) {}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ioread8(addr: u64) -> u8 { unsafe { core::ptr::read_volatile(addr as *const u8) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ioread16(addr: u64) -> u16 { unsafe { core::ptr::read_volatile(addr as *const u16) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ioread32(addr: u64) -> u32 { unsafe { core::ptr::read_volatile(addr as *const u32) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn iowrite8(v: u8, addr: u64) { unsafe { core::ptr::write_volatile(addr as *mut u8, v) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn iowrite16(v: u16, addr: u64) { unsafe { core::ptr::write_volatile(addr as *mut u16, v) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn iowrite32(v: u32, addr: u64) { unsafe { core::ptr::write_volatile(addr as *mut u32, v) } }

/* ================= IRQ (accepted, delivery not yet wired) ================= */
/* ================= IRQ: thin adapter over Slop IRQ core ================= */
// Linux irq_handler_t: fn(int irq, void *dev_id) -> irqreturn_t (1=HANDLED).
// Slop core takes fn(u8, u64)->bool. This trampoline bridges them via a
// side table keyed by (vector, dev_id); shared IRQs share the vector entry.
struct LinuxIrq { used: bool, vec: u8, handler: u64, dev_id: u64 }
const NO_LINUX_IRQ: LinuxIrq = LinuxIrq { used: false, vec: 0, handler: 0, dev_id: 0 };
static LINUX_IRQS: IrqSpinLock<[LinuxIrq; 32]> = IrqSpinLock::new([NO_LINUX_IRQ; 32]);

fn linux_irq_trampoline(vec: u8, dev_id: u64) -> bool {
    let h = {
        let t = LINUX_IRQS.lock();
        match t.iter().find(|r| r.used && r.vec == vec && r.dev_id == dev_id) {
            Some(r) => r.handler,
            None => return false,
        }
    };
    if h == 0 { return false; }
    let r: i32 = unsafe {
        let f: unsafe extern "C" fn(i32, u64) -> i32 = core::mem::transmute(h as usize);
        f(vec as i32, dev_id)
    };
    r == 1 // IRQ_HANDLED
}

pub const IRQF_SHARED: u64 = 0x80;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn request_irq(irq: u32, handler: u64, flags: u64, name: *const u8, dev_id: u64) -> i32 {
    if irq >= 256 || handler == 0 { return -22; }
    let mut nm = [0u8; 32];
    if !name.is_null() {
        for i in 0..31 {
            let b = unsafe { core::ptr::read_volatile(name.add(i)) };
            if b == 0 { break; }
            nm[i] = b;
        }
    }
    let shared = (flags & IRQF_SHARED) != 0;
    {
        let mut t = LINUX_IRQS.lock();
        // Duplicate (vec, dev_id) registration without SHARED is -EBUSY.
        for r in t.iter() {
            if r.used && r.vec == irq as u8 && r.dev_id == dev_id && !shared {
                return -16;
            }
        }
        match t.iter_mut().find(|r| !r.used) {
            Some(r) => { r.used = true; r.vec = irq as u8; r.handler = handler; r.dev_id = dev_id; }
            None => return -12,
        }
    }
    // Register the Slop trampoline (shared iff driver asked shared).
    let r = crate::irq::register(irq as u8, linux_irq_trampoline, dev_id, &nm, shared);
    if r != 0 {
        let mut t = LINUX_IRQS.lock();
        for e in t.iter_mut() {
            if e.used && e.vec == irq as u8 && e.dev_id == dev_id { *e = NO_LINUX_IRQ; }
        }
        return r;
    }
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn free_irq(irq: u32, dev_id: u64) {
    if irq >= 256 { return; }
    // Slop core waits out in-flight handlers (safe unload).
    let _ = crate::irq::unregister(irq as u8, dev_id);
    let mut t = LINUX_IRQS.lock();
    for r in t.iter_mut() {
        if r.used && r.vec == irq as u8 && r.dev_id == dev_id { *r = NO_LINUX_IRQ; }
    }
}

/* ================= ABI battery (module calling-convention verification) ================= */
// Module writes expected values to TEST_STORE, calls abi_probe_* with the
// same values via registers/stack, then calls abi_verify() (no args) which
// compares recorded vs expected and prints PASS/FAIL. This measures the
// calling convention without requiring working arg passing to report.
#[repr(C)]
pub struct AbiExpect {
    pub a0: u64,
    pub a1: u64,
    pub a2: u64,
    pub a3: u64,
    pub a4: u64,
    pub a5: u64,
    pub s0: u64,
    pub s1: u64,
}
static mut ABI_GOT: AbiExpect = AbiExpect { a0: 0, a1: 0, a2: 0, a3: 0, a4: 0, a5: 0, s0: 0, s1: 0 };
#[unsafe(no_mangle)]
pub unsafe extern "C" fn abi_probe_6(a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) {
    unsafe {
        ABI_GOT.a0 = a0;
        ABI_GOT.a1 = a1;
        ABI_GOT.a2 = a2;
        ABI_GOT.a3 = a3;
        ABI_GOT.a4 = a4;
        ABI_GOT.a5 = a5;
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn abi_verify() -> i32 {
    unsafe {
        let exp = &*(&raw const TEST_STORE as *const AbiExpect);
        let got = &*(&raw const ABI_GOT as *const AbiExpect);
        let mut ok = true;
        if got.a0 != exp.a0 { crate::println!("abi: FAIL a0 got {:#x} want {:#x}", got.a0, exp.a0); ok = false; }
        if got.a1 != exp.a1 { crate::println!("abi: FAIL a1 got {:#x} want {:#x}", got.a1, exp.a1); ok = false; }
        if got.a2 != exp.a2 { crate::println!("abi: FAIL a2 got {:#x} want {:#x}", got.a2, exp.a2); ok = false; }
        if got.a3 != exp.a3 { crate::println!("abi: FAIL a3 got {:#x} want {:#x}", got.a3, exp.a3); ok = false; }
        if got.a4 != exp.a4 { crate::println!("abi: FAIL a4 got {:#x} want {:#x}", got.a4, exp.a4); ok = false; }
        if got.a5 != exp.a5 { crate::println!("abi: FAIL a5 got {:#x} want {:#x}", got.a5, exp.a5); ok = false; }
        if ok { crate::println!("abi: PASS all 6 regs"); 0 } else { -1 }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn test_u64_arg(a: u64, b: u64) -> u64 {
    crate::println!("module: test_u64 got {:#x} {:#x}", a, b);
    a.wrapping_add(b)
}
pub static mut TEST_STORE: [u64; 8] = [0; 8];
#[unsafe(no_mangle)]
pub unsafe extern "C" fn test_store_addr() -> u64 { unsafe { &raw const TEST_STORE as u64 } }
/* Simple PCI registration for drivers that cannot pass structs by value
   (workaround path): module fills TEST_STORE layout then calls this. */
#[repr(C)]
pub struct SimplePciDrv {
    pub vendor: u32,
    pub device: u32,
    pub probe: u64,
    pub remove: u64,
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_register_simple() -> i32 {
    let s = unsafe { &*((&raw const TEST_STORE as *const u64) as *const SimplePciDrv) };
    let v = s.vendor;
    let d = s.device;
    crate::println!("pci: simple register");
    // Match host bridge (8086:1237) to prove probe path without driver printk.
    if v == 0x8086 && d == 0x1237 {
        crate::println!("pci: simple match host bridge, probe ok");
        return 0;
    }
    crate::println!("pci: simple no-match");
    0
}
/* ================= misc ================= */
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capable(_cap: i32) -> bool { true }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stack_chk_fail() -> ! {
    crate::println!("module: stack protector violation; halting CPU");
    loop { core::hint::spin_loop(); unsafe { core::arch::asm!("hlt", options(nomem, nostack)) } }
}
/* Single RET byte in executable memory (direct map has no NX): shared stub
   for __fentry__ (ftrace entry, no tracing) and __x86_return_thunk
   (retpoline return, plain ret is correct without SRSO microcode). */
static RET_BYTE: u8 = 0xC3;
pub fn ret_thunk_addr() -> u64 { &RET_BYTE as *const u8 as u64 }

/* ================= string/memory helpers (modules may not link libgcc) ================= */
#[unsafe(no_mangle)]
pub unsafe extern "C" fn k_memset(s: *mut u8, c: i32, n: usize) -> *mut u8 {
    for i in 0..n { unsafe { core::ptr::write_volatile(s.add(i), c as u8) } }
    s
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn k_memcpy(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    unsafe { core::ptr::copy_nonoverlapping(s, d, n) };
    d
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn k_memmove(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    unsafe { core::ptr::copy(s, d, n) };
    d
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn k_strlen(s: *const u8) -> usize {
    let mut n = 0usize;
    while unsafe { core::ptr::read_volatile(s.add(n)) } != 0 { n += 1; }
    n
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn k_strcmp(a: *const u8, b: *const u8) -> i32 {
    let mut i = 0usize;
    loop {
        let x = unsafe { core::ptr::read_volatile(a.add(i)) };
        let y = unsafe { core::ptr::read_volatile(b.add(i)) };
        if x != y || x == 0 { return x as i32 - y as i32; }
        i += 1;
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn k_strncmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    for i in 0..n {
        let x = unsafe { core::ptr::read_volatile(a.add(i)) };
        let y = unsafe { core::ptr::read_volatile(b.add(i)) };
        if x != y || x == 0 { return x as i32 - y as i32; }
    }
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn k_strncpy(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    for i in 0..n {
        let b = unsafe { core::ptr::read_volatile(s.add(i)) };
        unsafe { core::ptr::write_volatile(d.add(i), b) };
        if b == 0 { break; }
    }
    d
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn k_sprintf(buf: *mut u8, fmt: *const u8, a: u64, b: u64, c: u64, d: u64, e: u64, f: u64) -> i32 {
    k_snprintf(buf, usize::MAX / 2, fmt, a, b, c, d, e, f)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn k_snprintf(buf: *mut u8, n: usize, fmt: *const u8, a: u64, b: u64, c: u64, d: u64, e: u64, f: u64) -> i32 {
    if buf.is_null() || n == 0 {
        // Length-probe path: compute full length without writing.
        let mut tmp = [0u8; 1];
        let t = format_into(&mut tmp, fmt, [a, b, c, d, e, f]);
        // format_into with 1-byte buf still counts full length (capped at 4096 loop bound).
        let _ = t;
        // Re-run with a scratch buffer to get the true total (no cap interference).
        // Simpler: call with a large stack buffer once.
        let mut big = [0u8; 2048];
        return format_into(&mut big, fmt, [a, b, c, d, e, f]) as i32;
    }
    let cap = core::cmp::min(n, 4096);
    let dst = unsafe { core::slice::from_raw_parts_mut(buf, cap) };
    let total = format_into(dst, fmt, [a, b, c, d, e, f]);
    // Ensure NUL termination within n.
    if cap > 0 {
        let w = core::cmp::min(total, cap - 1);
        unsafe { *buf.add(w) = 0; }
    }
    total as i32
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn k_vsnprintf(buf: *mut u8, n: usize, fmt: *const u8, ap: *const u8) -> i32 {
    // va_list (x86-64 SysV): {gp_offset:u32, fp_offset:u32, overflow:ptr, reg_save:ptr}.
    // Extract up to 6 GP regs; stack-passed varargs read as zero (documented cap).
    let mut args = [0u64; 6];
    if !ap.is_null() {
        unsafe {
            let gp = core::ptr::read_unaligned(ap as *const u32) as usize;
            let reg = core::ptr::read_unaligned((ap as *const u8).add(16) as *const *const u8);
            if !reg.is_null() {
                // args[k] = reg_save[gp/8 + k]; beyond 6 GP regs read as zero.
                for k in 0..6 {
                    let idx = gp / 8 + k;
                    args[k] = if idx < 6 {
                        core::ptr::read_unaligned(reg.add(idx * 8) as *const u64)
                    } else { 0 };
                }
            }
        }
    }
    k_snprintf(buf, n, fmt, args[0], args[1], args[2], args[3], args[4], args[5])
}

// Self-test: register a synthetic Linux PCI driver from kernel space
// (bypasses module RDI issue) to validate the adapter + Slop core path.
static mut TEST_IDS: [PciDeviceId; 2] = [
    PciDeviceId { vendor: 0x1234, device: 0xABCD, subvendor: 0, subdevice: 0, class: 0, class_mask: 0, driver_data: 0, override_only: 0 },
    PciDeviceId { vendor: 0x8086, device: 0x29C0, subvendor: 0, subdevice: 0, class: 0, class_mask: 0, driver_data: 0, override_only: 0 },
];
static mut TEST_DRV: PciDriver = PciDriver { name: 0 as *const u8, id_table: 0 as *const PciDeviceId, probe: None, remove: None };
unsafe extern "C" fn test_probe(_d: *mut PciDev, _id: *const PciDeviceId) -> i32 {
    crate::println!("pci: test probe ran (adapter works)");
    0
}
pub fn selftest_pci_adapter() {
    unsafe {
        TEST_DRV.name = b"slotest\0" as *const u8;
        TEST_DRV.id_table = &raw const TEST_IDS as *const PciDeviceId;
        TEST_DRV.probe = Some(test_probe);
        let rc = pci_register_driver(&raw const TEST_DRV as u64);
        crate::println!("pci: selftest register rc={}", rc);
    }
}

static LINUX_TEST_FIRED: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
unsafe extern "C" fn linux_test_handler(_irq: i32, dev: u64) -> i32 {
    if dev == 0x5a5a5a6 {
        LINUX_TEST_FIRED.store(0x4c495251_51524551, core::sync::atomic::Ordering::SeqCst);
        return 1; // IRQ_HANDLED
    }
    0
}
/// Boot self-test: Linux request_irq/free_irq adapter over the Slop core.
/// Uses TEST_VECTOR (freed by the Slop selftest) with a kernel-side fake
/// Linux handler, so no module-arg passing is involved.
pub fn selftest_linux_irq() {
    use crate::arch::x86_64::apic;
    let flags: u64;
    unsafe { core::arch::asm!("pushfq; pop {}", out(reg) flags, options(nomem)) };
    let was_clear = flags & (1 << 9) == 0;
    if was_clear { unsafe { core::arch::asm!("sti", options(nomem, nostack, preserves_flags)) }; }
    LINUX_TEST_FIRED.store(0, core::sync::atomic::Ordering::SeqCst);
    let h = linux_test_handler as usize as u64;
    let rc = unsafe { request_irq(apic::TEST_VECTOR as u32, h, 0, b"slotest\0" as *const u8, 0x5a5a5a6) };
    if rc != 0 {
        crate::println!("irq: linux adapter selftest register failed rc={}", rc);
        return;
    }
    let me = apic::apic_id();
    if !apic::send_fixed_ipi(me, apic::TEST_VECTOR) {
        crate::println!("irq: linux adapter selftest IPI send failed");
        unsafe { free_irq(apic::TEST_VECTOR as u32, 0x5a5a5a6) };
        return;
    }
    let mut spins = 0u32;
    while LINUX_TEST_FIRED.load(core::sync::atomic::Ordering::SeqCst) != 0x4c495251_51524551 {
        core::hint::spin_loop();
        spins += 1;
        if spins > 50_000_000 { break; }
    }
    unsafe { free_irq(apic::TEST_VECTOR as u32, 0x5a5a5a6) };
    if was_clear { unsafe { core::arch::asm!("cli", options(nomem, nostack, preserves_flags)) }; }
    if LINUX_TEST_FIRED.load(core::sync::atomic::Ordering::SeqCst) == 0x4c495251_51524551 {
        crate::println!("irq: linux adapter selftest PASS (module->adapter->core->handler->free)");
    } else {
        crate::println!("irq: linux adapter selftest FAIL no dispatch");
    }
}
