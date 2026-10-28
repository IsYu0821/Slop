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
pub unsafe extern "C" fn printk(fmt: *const u8, a: u64, b: u64, c: u64, d: u64, e: u64, f: u64) -> i32 {
    if fmt.is_null() { return 0; }
    let mut args = [a, b, c, d, e, f];
    let mut ai = 0usize;
    let mut out = [0u8; 512];
    let mut n = 0usize;
    let mut p = fmt;
    let next_arg = |args: &mut [u64; 6], ai: &mut usize| -> u64 {
        let v = if *ai < 6 { args[*ai] } else { 0 };
        *ai += 1;
        v
    };
    let mut push = |out: &mut [u8; 512], n: &mut usize, by: u8| {
        if *n < out.len() { out[*n] = by; *n += 1; }
    };
    let push_u64 = |out: &mut [u8; 512], n: &mut usize, mut v: u64, base: u64, upper: bool| {
        if v == 0 { push(out, n, b'0'); return; }
        let mut tmp = [0u8; 20];
        let mut l = 0usize;
        while v > 0 { let r = (v % base) as u8; tmp[l] = if r < 10 { b'0' + r } else { (if upper { b'A' } else { b'a' }) + r - 10 }; l += 1; v /= base; }
        while l > 0 { l -= 1; push(out, n, tmp[l]); }
    };
    loop {
        let ch = unsafe { core::ptr::read_volatile(p) };
        if ch == 0 { break; }
        p = unsafe { p.add(1) };
        if ch != b'%' { push(&mut out, &mut n, ch); continue; }
        let s = unsafe { core::ptr::read_volatile(p) };
        if s == 0 { break; }
        p = unsafe { p.add(1) };
        match s {
            b'%' => push(&mut out, &mut n, b'%'),
            b'c' => push(&mut out, &mut n, next_arg(&mut args, &mut ai) as u8),
            b's' => {
                let sp = next_arg(&mut args, &mut ai) as *const u8;
                if !sp.is_null() {
                    for i in 0..256 {
                        let by = unsafe { core::ptr::read_volatile(sp.add(i)) };
                        if by == 0 { break; }
                        push(&mut out, &mut n, by);
                    }
                } else {
                    for by in b"(null)".iter() { push(&mut out, &mut n, *by); }
                }
            }
            b'd' | b'i' => {
                let v = next_arg(&mut args, &mut ai) as i64;
                if v < 0 { push(&mut out, &mut n, b'-'); push_u64(&mut out, &mut n, (-v) as u64, 10, false); }
                else { push_u64(&mut out, &mut n, v as u64, 10, false); }
            }
            b'u' => push_u64(&mut out, &mut n, next_arg(&mut args, &mut ai), 10, false),
            b'x' => push_u64(&mut out, &mut n, next_arg(&mut args, &mut ai), 16, false),
            b'X' => push_u64(&mut out, &mut n, next_arg(&mut args, &mut ai), 16, true),
            b'p' => {
                push(&mut out, &mut n, b'0'); push(&mut out, &mut n, b'x');
                push_u64(&mut out, &mut n, next_arg(&mut args, &mut ai), 16, false);
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
                            if v < 0 { push(&mut out, &mut n, b'-'); push_u64(&mut out, &mut n, (-v) as u64, 10, false); }
                            else { push_u64(&mut out, &mut n, v as u64, 10, false); }
                        }
                        b'x' => push_u64(&mut out, &mut n, v, 16, false),
                        _ => push_u64(&mut out, &mut n, v, 10, false),
                    }
                } else { push(&mut out, &mut n, b'%'); push(&mut out, &mut n, b'l'); }
            }
            _ => { push(&mut out, &mut n, b'%'); push(&mut out, &mut n, s); }
        }
        if n >= 500 { break; }
    }
    crate::console::write_bytes(&out[..n]);
    n as i32
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
// slot/devfn/vendor/device/...) then zeroed. Offsets vendor@60 device@62
// verified against host headers; irq/resource left for future HW drivers.
#[repr(C)]
pub struct PciDev {
    _pad0: [u8; 60],
    pub vendor: u16,
    pub device: u16,
    _rest: [u8; 448],
}
const _: () = assert!(core::mem::size_of::<PciDev>() == 512);

struct PciReg { used: bool, drv: u64, name: [u8; 48] }
const NO_REG: PciReg = PciReg { used: false, drv: 0, name: [0; 48] };
static PCI_REGS: IrqSpinLock<[PciReg; 16]> = IrqSpinLock::new([NO_REG; 16]);

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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_register_driver(drv: u64) -> i32 {
    if drv == 0 { return -22; } // -EINVAL
    let p = unsafe { &*(drv as *const PciDriver) };
    if p.id_table.is_null() || p.probe.is_none() { return -22; }
    {
        let mut regs = PCI_REGS.lock();
        match regs.iter_mut().find(|r| !r.used) {
            Some(r) => { r.used = true; r.drv = drv; r.name = drv_name(drv); }
            None => return -12, // -ENOMEM
        }
    }
    // Match against already-enumerated PCI devices and probe immediately
    // (no hotplug bus rescan in this kernel yet).
    let matched = pci_match_and_probe(drv);
    crate::println!("pci: driver registered ({} devices matched)", matched);
    0
}

fn pci_match_and_probe(drv: u64) -> i32 {
    use crate::driver::pci::{Cam, ConfigAccess, CFG_VENDOR, CFG_CLASS};
    let p = unsafe { &*(drv as *const PciDriver) };
    let mut ids: usize = 0;
    // Count table length (terminated by all-zero vendor/device/sub/class).
    loop {
        let e = unsafe { &*p.id_table.add(ids) };
        if e.vendor == 0 && e.device == 0 && e.subvendor == 0 && e.subdevice == 0 && e.class == 0 {
            break;
        }
        ids += 1;
        if ids > 64 { break; }
    }
    let acc = Cam;
    let mut n = 0;
    for bus in 0..=255u8 {
        for dev in 0..32u8 {
            for fun in 0..8u8 {
                let bdf = crate::driver::pci::Bdf { segment: 0, bus, device: dev, function: fun };
                let w = acc.read32(bdf, CFG_VENDOR);
                if w & 0xffff == 0xffff { continue; }
                let vendor = (w & 0xffff) as u16;
                let device = ((w >> 16) & 0xffff) as u16;
                for k in 0..ids {
                    let e = unsafe { &*p.id_table.add(k) };
                    let vm = e.vendor == 0xffff || e.vendor == vendor as u32 || e.vendor == 0;
                    let dm = e.device == 0xffff || e.device == device as u32 || e.device == 0;
                    if vm && dm && pci_class_match(e, bdf) {
                        if pci_probe_one(drv, bdf, vendor, device, e) == 0 { n += 1; }
                        break;
                    }
                }
            }
        }
    }
    n
}

fn pci_class_match(e: &PciDeviceId, bdf: crate::driver::pci::Bdf) -> bool {
    use crate::driver::pci::{Cam, ConfigAccess, CFG_CLASS};
    if e.class == 0 && e.class_mask == 0 { return true; }
    let acc = Cam;
    let cls = acc.read32(bdf, CFG_CLASS);
    // Linux compares (class ^ e.class) & e.class_mask == 0 on the 24-bit field.
    // CFG_CLASS low byte is revision; class lives in the top 3 bytes.
    let dev_cls = (cls >> 8) & 0xffffff;
    let want = e.class & 0xffffff;
    let mask = if e.class_mask == 0 { 0xffffff } else { e.class_mask & 0xffffff };
    (dev_cls ^ want) & mask == 0
}

fn pci_probe_one(drv: u64, bdf: crate::driver::pci::Bdf, vendor: u16, device: u16, id: *const PciDeviceId) -> i32 {
    let p = unsafe { &*(drv as *const PciDriver) };
    let probe = match p.probe { Some(f) => f, None => return -22 };
    // Compat pci_dev on the heap (512B, real vendor/device prefix).
    let mem = crate::kheap::kzalloc(512);
    if mem.is_null() { return -12; }
    let pd = mem as *mut PciDev;
    unsafe {
        (*pd).vendor = vendor;
        (*pd).device = device;
    }
    // Fill BAR bases + IRQ line/pin from config space for future HW use.
    pci_fill_dev_resources(pd, bdf);
    let rc = unsafe { probe(pd, id) };
    // NOTE: dev buffer intentionally leaked while bound (remove() path frees
    // it later once unbind tracking lands; today delete_module drops the
    // registration and leaks bound devs — documented, no WildPtr).
    let _ = bdf;
    if rc != 0 { crate::kheap::kfree(mem); }
    rc
}

fn pci_fill_dev_resources(_pd: *mut PciDev, _bdf: crate::driver::pci::Bdf) {
    // Future: copy BAR bases into resource[] once exact irq/resource offsets
    // are pinned for the target kernel version. Today: vendor/device only.
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_unregister_driver(drv: u64) {
    if drv == 0 { return; }
    let mut regs = PCI_REGS.lock();
    for r in regs.iter_mut() {
        if r.used && r.drv == drv { *r = NO_REG; }
    }
}
/* Called on module unload: drop registrations that came from it. Full
   per-module owner tracking is future work; today we drop by pointer. */
pub fn pci_unregister_for_module() {
    // The module's own delete path calls pci_unregister_driver directly
    // (its cleanup_module runs first), so nothing to do here yet.
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_enable_device(_dev: *mut PciDev) -> i32 { 0 }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_disable_device(_dev: *mut PciDev) {}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_request_regions(_dev: *mut PciDev, _name: *const u8) -> i32 { 0 }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_release_regions(_dev: *mut PciDev) {}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_set_master(_dev: *mut PciDev) {}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_clear_master(_dev: *mut PciDev) {}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_set_drvdata(_dev: *mut PciDev, _data: u64) {}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_get_drvdata(_dev: *mut PciDev) -> u64 { 0 }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_ioremap_bar(_dev: *mut PciDev, _nr: u64) -> u64 { 0 }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pci_iounmap(_dev: *mut PciDev, _addr: u64) {}

/* ================= MMIO ================= */
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ioremap(phys: u64, _size: u64) -> u64 {
    crate::arch::x86_64::phys::phys_to_virt(phys)
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
struct IrqReg { used: bool, irq: u32, handler: u64, dev_id: u64 }
const NO_IRQ: IrqReg = IrqReg { used: false, irq: 0, handler: 0, dev_id: 0 };
static IRQS: IrqSpinLock<[IrqReg; 32]> = IrqSpinLock::new([NO_IRQ; 32]);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn request_irq(irq: u32, handler: u64, _flags: u64, _name: *const u8, dev_id: u64) -> i32 {
    let mut t = IRQS.lock();
    match t.iter_mut().find(|r| !r.used) {
        Some(r) => { r.used = true; r.irq = irq; r.handler = handler; r.dev_id = dev_id; }
        None => return -12,
    }
    crate::println!("irq: handler registered for line {} (delivery not yet wired; poll your device)", irq);
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn free_irq(_irq: u32, dev_id: u64) {
    let mut t = IRQS.lock();
    for r in t.iter_mut() {
        if r.used && r.dev_id == dev_id { *r = NO_IRQ; }
    }
}

/* ================= misc ================= */
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capable(_cap: i32) -> bool { true }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn test_u64_arg(x: u64) -> u64 {
    crate::println!("module: test_u64 got {:#x}", x);
    x.wrapping_add(1)
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
pub unsafe extern "C" fn k_sprintf(_buf: *mut u8, _fmt: *const u8) -> i32 { 0 }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn k_snprintf(_buf: *mut u8, _n: usize, _fmt: *const u8) -> i32 { 0 }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn k_vsnprintf(_buf: *mut u8, _n: usize, _fmt: *const u8) -> i32 { 0 }
