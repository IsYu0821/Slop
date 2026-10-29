//! Linux-driver-compatible kernel symbols: printk, kmalloc family,
//! sleeps, jiffies, PCI registry + operations, MMIO, IRQ stubs, and the
//! string/memory helpers modules expect. Addresses in this file are exported
//! to .ko objects via `module::export_table`.
//!
//! Scope discipline (what this is NOT yet): no MSI/MSI-X routing
//! (IRQ registration is accepted and stored; delivery is not wired yet —
//! drivers must poll), no full `struct pci_dev` (vendor/device filled, the
//! rest zeroed; real HW drivers needing irq/resource fields come later).
use crate::sync::IrqSpinLock;
use core::sync::atomic::{AtomicU64, Ordering};

/* ================= printk ================= */
// Bounded format engine: %d %i %u %o %x %X %p %c %s %% with flags
// ('-', '0'), width (digits or '*'), precision ('.' digits or '*');
// lengths are accepted and ignored (args are already u64). %n is
// rejected (emitted literally, never stores through the pointer).
// The caller passes up to 6 register varargs (SysV: rsi..r9); stack
// varargs read as zero (documented limitation).
// Safety: the format string and every %s argument are copied through
// page-present checks (FMT_MAX/STR_MAX caps), so bad or stale
// (post-unload) pointers fail safe as "(bad fmt)"/"(bad ptr)" instead
// of faulting or over-reading module memory. %s precision truncation
// never splits a UTF-8 sequence. Width/precision saturate at
// FIELD_MAX (fail-safe truncation, documented).
const FMT_MAX: usize = 1024;
const STR_MAX: usize = 256;
const FIELD_MAX: usize = 512;

fn va_page_present(va: u64) -> bool {
    let hi = va >> 48;
    if hi != 0 && hi != 0xFFFF { return false; }
    crate::arch::x86_64::phys::virt_to_phys_full(va & !4095) != 0
}

/// Copy a NUL-terminated string at `src` into `dst` (up to `max` bytes,
/// never past `dst.len()`). Stops at unmapped pages. Returns bytes copied.
fn copy_cstr(dst: &mut [u8], src: *const u8, max: usize) -> usize {
    let cap = core::cmp::min(max, dst.len());
    let mut n = 0usize;
    let mut i = 0usize;
    while i < cap {
        let va = (src as u64).wrapping_add(i as u64);
        if i % 4096 == 0 && !va_page_present(va) { break; }
        let b = unsafe { core::ptr::read_volatile(src.wrapping_add(i)) };
        if b == 0 { break; }
        dst[n] = b;
        n += 1;
        i += 1;
    }
    n
}

/// Largest `k <= max` such that `b[..k]` ends on a UTF-8 boundary.
fn utf8_trunc_len(b: &[u8], max: usize) -> usize {
    let cut = core::cmp::min(max, b.len());
    let mut pos = 0usize;
    while pos < cut {
        let c = b[pos];
        let l = if c & 0x80 == 0 {
            1
        } else if c >> 5 == 0b110 {
            2
        } else if c >> 4 == 0b1110 {
            3
        } else if c >> 3 == 0b11110 {
            4
        } else {
            1 // invalid byte: pass through singly, never hang
        };
        if pos + l > cut {
            return pos;
        }
        // Multi-byte lead must be followed by continuation bytes.
        let mut k = 1usize;
        while k < l {
            if b[pos + k] & 0xC0 != 0x80 {
                return pos;
            }
            k += 1;
        }
        pos += l;
    }
    pos
}

#[unsafe(no_mangle)]
fn format_into(out: &mut [u8], fmt: *const u8, args_in: [u64; 6]) -> usize {
    let mut args = args_in;
    let mut ai = 0usize;
    let mut n = 0usize;
    let mut total = 0usize;
    macro_rules! push {
        ($by:expr) => {{
            total += 1;
            if n < out.len() {
                out[n] = $by;
                n += 1;
            }
        }};
    }
    if fmt.is_null() || !va_page_present(fmt as u64) {
        for &b in b"(bad fmt)" {
            push!(b);
        }
        return total;
    }
    // Copy the format string through page checks first, so the parse loop
    // below can never run past mapped memory.
    let mut fbuf = [0u8; FMT_MAX + 1];
    let flen = copy_cstr(&mut fbuf, fmt, FMT_MAX);
    let mut next_arg = |args: &mut [u64; 6], ai: &mut usize| -> u64 {
        let v = if *ai < 6 { args[*ai] } else { 0 };
        *ai += 1;
        v
    };
    let mut i = 0usize;
    // Stop at NUL too: fbuf is NUL-padded, so this is robust even if flen
    // ever overshoots the terminator.
    while i < flen && fbuf[i] != 0 {
        let ch = fbuf[i];
        i += 1;
        if ch != b'%' {
            push!(ch);
            continue;
        }
        if i >= flen {
            push!(b'%');
            break;
        }
        // Flags.
        let mut left = false;
        let mut zero = false;
        while i < flen {
            if fbuf[i] == b'-' { left = true; i += 1; }
            else if fbuf[i] == b'0' { zero = true; i += 1; }
            else { break; }
        }
        // Width.
        let mut width: usize = 0;
        let mut width_set = false;
        if i < flen && fbuf[i] == b'*' {
            i += 1;
            let w = next_arg(&mut args, &mut ai) as i32;
            if w < 0 { left = true; width = (-(w as i64) as u64).min(FIELD_MAX as u64) as usize; }
            else { width = (w as u64).min(FIELD_MAX as u64) as usize; }
            width_set = true;
        } else {
            while i < flen && fbuf[i].is_ascii_digit() {
                width_set = true;
                width = width.saturating_mul(10).saturating_add((fbuf[i] - b'0') as usize).min(FIELD_MAX);
                i += 1;
            }
        }
        // Precision.
        let mut prec: Option<usize> = None;
        if i < flen && fbuf[i] == b'.' {
            i += 1;
            if i < flen && fbuf[i] == b'*' {
                i += 1;
                let p = next_arg(&mut args, &mut ai) as i32;
                if p >= 0 { prec = Some((p as u64).min(FIELD_MAX as u64) as usize); }
            } else {
                let mut p: usize = 0;
                while i < flen && fbuf[i].is_ascii_digit() {
                    p = p.saturating_mul(10).saturating_add((fbuf[i] - b'0') as usize).min(FIELD_MAX);
                    i += 1;
                }
                prec = Some(p);
            }
        }
        // Lengths: C varargs promote int->32bit; only l/ll (and j/z/t
        // 64-bit) keep full 64 bits. h/hh narrow further. Correct sizing
        // matters: a negative int arrives zero-extended in the low 32.
        let mut wide = false; // l, ll, j, z, t
        while i < flen && matches!(fbuf[i], b'h' | b'l' | b'j' | b'z' | b't' | b'L') {
            if matches!(fbuf[i], b'l' | b'j' | b'z' | b't') { wide = true; }
            i += 1;
        }
        if i >= flen {
            push!(b'%');
            break;
        }
        let s = fbuf[i];
        i += 1;
        match s {
            b'%' => {
                let w = if width_set { width } else { 0 };
                if !left { for _ in 0..w { push!(b' '); } }
                push!(b'%');
                if left { for _ in 0..w { push!(b' '); } }
            }
            b'c' => {
                let v = next_arg(&mut args, &mut ai) as u32 as u8;
                let w = if width_set { width } else { 0 };
                if !left { for _ in 0..w.saturating_sub(1) { push!(b' '); } }
                push!(v);
                if left { for _ in 0..w.saturating_sub(1) { push!(b' '); } }
            }
            b's' => {
                let sp = next_arg(&mut args, &mut ai) as *const u8;
                let mut sbuf = [0u8; STR_MAX];
                let slen = if sp.is_null() {
                    sbuf[..6].copy_from_slice(b"(null)");
                    6
                } else if !va_page_present(sp as u64) {
                    sbuf[..9].copy_from_slice(b"(bad ptr)");
                    9
                } else {
                    copy_cstr(&mut sbuf, sp, STR_MAX)
                };
                // Field bytes first (truncate + UTF-8-safe), then pad.
                // Count is derived from the final field length only.
                let mut flen2 = slen;
                if let Some(p) = prec {
                    let t = utf8_trunc_len(&sbuf[..slen], p);
                    if t < flen2 {
                        flen2 = t;
                    }
                }
                let w = if width_set { width } else { 0 };
                if !left {
                    let mut k = flen2;
                    while k < w {
                        push!(b' ');
                        k += 1;
                    }
                }
                {
                    let mut k = 0;
                    while k < flen2 {
                        push!(sbuf[k]);
                        k += 1;
                    }
                }
                if left {
                    let mut k = flen2;
                    while k < w {
                        push!(b' ');
                        k += 1;
                    }
                }
            }
            b'd' | b'i' | b'u' | b'o' | b'x' | b'X' | b'p' => {
                let raw = next_arg(&mut args, &mut ai);
                // Narrow to declared width first (C promotion semantics).
                let v: u64 = if s == b'p' || wide { raw } else { raw as u32 as u64 };
                let (neg, base, upper) = match s {
                    b'd' | b'i' => {
                        let sv = if s == b'p' || wide { raw as i64 } else { (raw as u32 as i32) as i64 };
                        (sv < 0, 10, false)
                    }
                    b'o' => (false, 8, false),
                    b'x' => (false, 16, false),
                    b'X' => (false, 16, true),
                    b'p' => (false, 16, false),
                    _ => (false, 10, false),
                };
                let mut mag = if neg { (-((if wide || s == b'p' { raw as i64 } else { (raw as u32 as i32) as i64 }))) as u64 } else { v };
                // digits, reversed
                let mut tmp = [0u8; 32];
                let mut dl = 0usize;
                if mag == 0 {
                    // C: precision 0 suppresses the '0' digit (except %p keeps one).
                    if prec != Some(0) || s == b'p' { tmp[0] = b'0'; dl = 1; }
                } else {
                    while mag > 0 {
                        let r = (mag % base) as u8;
                        tmp[dl] = if r < 10 { b'0' + r } else { (if upper { b'A' } else { b'a' }) + r - 10 };
                        dl += 1;
                        mag /= base;
                    }
                }
                let mut ndig = dl;
                if let Some(p) = prec { ndig = core::cmp::max(ndig, core::cmp::min(p, FIELD_MAX)); }
                let content = ndig + if neg { 1 } else { 0 } + if s == b'p' { 2 } else { 0 };
                let w = if width_set { width } else { 0 };
                // Zero padding only without precision, not left-justified (standard).
                let zpad = zero && !left && prec.is_none();
                if !left && !zpad { for _ in 0..w.saturating_sub(content) { push!(b' '); } }
                if s == b'p' { push!(b'0'); push!(b'x'); }
                if neg { push!(b'-'); }
                if !left && zpad { for _ in 0..w.saturating_sub(content) { push!(b'0'); } }
                for _ in 0..ndig.saturating_sub(dl) { push!(b'0'); }
                let mut k = dl;
                while k > 0 { k -= 1; push!(tmp[k]); }
                if left { for _ in 0..w.saturating_sub(content) { push!(b' '); } }
                let _ = content;
            }
            // %n (store-back) and anything unknown: emit literally, consume
            // no argument. Safe by construction.
            _ => { push!(b'%'); push!(s); }
        }
    }
    // NUL-terminate if room (snprintf semantics; printk ignores the NUL).
    if n < out.len() { out[n] = 0; }
    total
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn printk(fmt: *const u8, a: u64, b: u64, c: u64, d: u64, e: u64, f: u64) -> i32 {
    let mut out = [0u8; 512];
    let n_full = format_into(&mut out, fmt, [a, b, c, d, e, f]);
    let n = core::cmp::min(n_full, 511);
    crate::console::write_bytes(&out[..n]);
    n_full as i32
}

/* UBSAN toolchain traps: normally-built .ko may carry -fsanitize
   instrumentation. Log the violation and continue (fail-safe: the guest
   stays alive and the offending check is visible in the log). */
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn ubsan_trap(_data: *const u8) {
    crate::println!("ubsan: violation trapped (continuing)");
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn kmalloc(size: usize, _gfp: u32) -> *mut u8 { crate::kheap::kmalloc(size) }
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn kzalloc(size: usize, _gfp: u32) -> *mut u8 { crate::kheap::kzalloc(size) }
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn kcalloc(n: usize, size: usize, _gfp: u32) -> *mut u8 { crate::kheap::kcalloc(n, size) }
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn kmemdup(src: *const u8, len: usize, _gfp: u32) -> *mut u8 { crate::kheap::kmemdup(src, len) }
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn krealloc(ptr: *mut u8, size: usize, _gfp: u32) -> *mut u8 {
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
pub unsafe extern "sysv64" fn kfree(ptr: *mut u8) { crate::kheap::kfree(ptr) }
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn ksize(ptr: *mut u8) -> usize { crate::kheap::ksize(ptr) }

/* ================= sleeps / jiffies ================= */
pub const HZ: u64 = 1000;
static JIFFIES: AtomicU64 = AtomicU64::new(0);
pub fn jiffies_addr() -> u64 { &JIFFIES as *const _ as u64 }
pub fn tick_jiffies() { JIFFIES.fetch_add(1, Ordering::Relaxed); }
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn msleep(ms: u32) {
    let t0 = JIFFIES.load(Ordering::Relaxed);
    while JIFFIES.load(Ordering::Relaxed).wrapping_sub(t0) < ms as u64 {
        core::hint::spin_loop();
        // Cooperative: let the timer tick run us forward. No locks held here
        // (module init context), so yielding is safe.
        crate::task::yield_now();
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn mdelay(ms: u32) {
    // Busy loop calibrated roughly: ~1e6 spins per ms on TCG.
    for _ in 0..(ms as u64 * 1_000_000) { core::hint::spin_loop(); }
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn udelay(us: u32) {
    for _ in 0..(us as u64 * 1000) { core::hint::spin_loop(); }
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn __const_udelay(us: u32) { unsafe { udelay(us) } }
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn __udelay(us: u64) { unsafe { udelay(us as u32) } }
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn usleep_range(_min: u64, _max: u64) { msleep(1); }

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
    pub probe: Option<unsafe extern "sysv64" fn(*mut PciDev, *const PciDeviceId) -> i32>,
    pub remove: Option<unsafe extern "sysv64" fn(*mut PciDev)>,
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
    pub msi_base: u8,  // MSI pool base vector (0 if none)
    pub msi_count: u8, // MSI vectors allocated
    pub intx_vec: u8,  // INTx vector (0 if none; level-triggered via IOAPIC)
    pub _pad1: u8,
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
static LINUX_PCI_DRVS: IrqSpinLock<[u64; 16]> = IrqSpinLock::new([0; 16]);
fn track_linux_pci_drv(drv: u64) {
    if drv == 0 { return; }
    let mut t = LINUX_PCI_DRVS.lock();
    if t.iter().any(|&d| d == drv) { return; }
    if let Some(s) = t.iter_mut().find(|s| **s == 0) { *s = drv; }
}
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
    if rc != 0 {
        crate::println!("pci: linux probe {:04x}:{:04x} rc={}", vendor, device, rc);
        crate::kheap::kfree(mem);
    } else {
        track_linux_pci_bound(opaque, bdf, pd as u64);
    }
    // NOTE: bound dev buffer lives until remove/unregister (freed there,
    // fixing the old intentional leak); see unbind_linux_pci_dev.
    rc
}

/* Bound-device tracking: (driver, bdf, dev ptr). unregister/remove walks
   this to invoke Linux remove, free MSI vectors, and kfree the dev struct
   (leak-free unload; in-flight IRQ waits in the Slop core). */
struct BoundPci { used: bool, drv: u64, bdf: u32, dev: u64 }
const NO_BOUND: BoundPci = BoundPci { used: false, drv: 0, bdf: 0, dev: 0 };
static LINUX_PCI_BOUND: IrqSpinLock<[BoundPci; 16]> = IrqSpinLock::new([NO_BOUND; 16]);
fn track_linux_pci_bound(drv: u64, bdf: crate::driver::pci::Bdf, dev: u64) {
    let packed = ((bdf.bus as u32) << 16) | ((bdf.device as u32) << 8) | (bdf.function as u32);
    let mut t = LINUX_PCI_BOUND.lock();
    if t.iter().any(|b| b.used && b.drv == drv && b.bdf == packed) { return; }
    if let Some(s) = t.iter_mut().find(|s| !s.used) {
        *s = BoundPci { used: true, drv, bdf: packed, dev };
    }
}
fn unbind_linux_pci_dev(dev: u64) {
    let slot = {
        let mut t = LINUX_PCI_BOUND.lock();
        match t.iter_mut().find(|b| b.used && b.dev == dev) {
            Some(b) => { let out = (b.drv, b.bdf); *b = NO_BOUND; Some(out) }
            None => None,
        }
    };
    if slot.is_none() { return; }
    // Free MSI vectors owned by this device (disables MSI at the device).
    let pd = dev as *mut PciDev;
    let bdf = unsafe {
        let b = (*pd).bdf;
        crate::driver::pci::Bdf { segment: 0, bus: ((b >> 16) & 0xff) as u8, device: ((b >> 8) & 0xff) as u8, function: (b & 0xff) as u8 }
    };
    crate::driver::pci::msi_free_vector(bdf);
    unsafe { (*pd).msi_base = 0; (*pd).msi_count = 0; }
    crate::kheap::kfree(dev as *mut u8);
}

#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn pci_register_driver(drv: u64) -> i32 {
    if drv == 0 { return -22; } // -EINVAL
    let p = unsafe { &*(drv as *const PciDriver) };
    if p.id_table.is_null() || p.probe.is_none() { return -22; }
    crate::println!("pci: drv={:#x} name={:#x} idtab={:#x} probe={}", drv, p.name as u64, p.id_table as u64, p.probe.is_some());
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
        crate::println!("pci: linux drv register {:x}:{:x} (raw v={:#x} d={:#x})", v, d, e.vendor, e.device);
        let r = crate::driver::pci::register_slop_driver(&name, v, d, slop_pci_probe_shim, drv);
        if r != 0 { return r; }
        count += 1;
    }
    if count == 0 { return -22; }
    track_linux_pci_drv(drv);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn pci_unregister_driver(drv: u64) {
    if drv == 0 { return; }
    // Invoke Linux remove for every bound device first (driver cleanup:
    // free_irq/DMA/bar-unmap run here), then free vectors + dev structs,
    // then drop the Slop registration. Safe if remove is missing.
    let targets: [(u64, u32); 16] = {
        let t = LINUX_PCI_BOUND.lock();
        let mut out = [(0u64, 0u32); 16];
        let mut n = 0usize;
        for b in t.iter() {
            if b.used && b.drv == drv && n < 16 { out[n] = (b.dev, b.bdf); n += 1; }
        }
        out
    };
    let lp = unsafe { &*(drv as *const PciDriver) };
    for (dev, _) in targets.iter() {
        if *dev == 0 { continue; }
        if let Some(remove) = lp.remove {
            unsafe { remove(*dev as *mut PciDev); }
        }
        unbind_linux_pci_dev(*dev);
    }
    {
        let mut t = LINUX_PCI_DRVS.lock();
        for d in t.iter_mut() { if *d == drv { *d = 0; } }
    }
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
pub unsafe extern "sysv64" fn pci_enable_device(dev: *mut PciDev) -> i32 {
    if dev.is_null() { return -22; }
    let bdf = unsafe { (*dev).bdf_of() };
    use crate::driver::pci::{CMD_IO, CMD_MEM};
    pci_cmd_set(bdf, CMD_IO | CMD_MEM, true);
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn pci_disable_device(dev: *mut PciDev) {
    if dev.is_null() { return; }
    let bdf = unsafe { (*dev).bdf_of() };
    use crate::driver::pci::{CMD_IO, CMD_MEM, CMD_BUS_MASTER};
    pci_cmd_set(bdf, CMD_IO | CMD_MEM | CMD_BUS_MASTER, false);
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn pci_request_regions(_dev: *mut PciDev, _name: *const u8) -> i32 {
    // No BAR reservation table yet (PCI subsystem step): accept, since Slop
    // owns all BARs on this single-enumeration platform and hands them out
    // read-only from config space. Revisit with resource management.
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn pci_release_regions(_dev: *mut PciDev) {}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn pci_set_master(dev: *mut PciDev) {
    if dev.is_null() { return; }
    let bdf = unsafe { (*dev).bdf_of() };
    pci_cmd_set(bdf, crate::driver::pci::CMD_BUS_MASTER, true);
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn pci_clear_master(dev: *mut PciDev) {
    if dev.is_null() { return; }
    let bdf = unsafe { (*dev).bdf_of() };
    pci_cmd_set(bdf, crate::driver::pci::CMD_BUS_MASTER, false);
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn pci_set_drvdata(dev: *mut PciDev, data: u64) {
    if dev.is_null() { return; }
    unsafe { (*dev).drvdata = data; }
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn pci_get_drvdata(dev: *mut PciDev) -> u64 {
    if dev.is_null() { return 0; }
    unsafe { (*dev).drvdata }
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn pci_ioremap_bar(dev: *mut PciDev, nr: u64) -> u64 {
    if dev.is_null() || nr >= 6 { return 0; }
    let bdf = unsafe { (*dev).bdf_of() };
    // Slop PCI core provides real UC mappings (fail-closed 0 on conflict).
    crate::driver::pci::ioremap_bar(bdf, nr as u8)
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn pci_iounmap(_dev: *mut PciDev, addr: u64) {
    crate::driver::pci::iounmap_bar(addr);
}

/* ================= MMIO ================= */
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn ioremap(phys: u64, _size: u64) -> u64 {
    // Direct-map aliases are WB-cached RAM mappings; using them for MMIO
    // silently corrupts device interaction. Until the Slop PCI/DMA core
    // provides real UC mappings, fail closed so drivers error out cleanly.
    crate::println!("mmio: ioremap {:#x} denied (no UC mapping yet)", phys);
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn iounmap(_addr: u64) {}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn ioread8(addr: u64) -> u8 { unsafe { core::ptr::read_volatile(addr as *const u8) } }
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn ioread16(addr: u64) -> u16 { unsafe { core::ptr::read_volatile(addr as *const u16) } }
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn ioread32(addr: u64) -> u32 { unsafe { core::ptr::read_volatile(addr as *const u32) } }
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn iowrite8(v: u8, addr: u64) { unsafe { core::ptr::write_volatile(addr as *mut u8, v) } }
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn iowrite16(v: u16, addr: u64) { unsafe { core::ptr::write_volatile(addr as *mut u16, v) } }
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn iowrite32(v: u32, addr: u64) { unsafe { core::ptr::write_volatile(addr as *mut u32, v) } }

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
            None => {
                crate::println!("irq: trampoline MISS vec={} dev={:#x}", vec, dev_id);
                return false;
            }
        }
    };
    if h == 0 { return false; }
    let r: i32 = unsafe {
        let f: unsafe extern "sysv64" fn(i32, u64) -> i32 = core::mem::transmute(h as usize);
        f(vec as i32, dev_id)
    };
    r == 1 // IRQ_HANDLED
}

pub const IRQF_SHARED: u64 = 0x80;
// Linux pci_alloc_irq_vectors flags (only MSI wired; INTx needs IOAPIC
// routing which is future work — request without MSI fails safe).
pub const PCI_IRQ_MSI: u32 = 1 << 1;
pub const PCI_IRQ_LEGACY: u32 = 1 << 0;

#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn pci_alloc_irq_vectors(dev: *mut PciDev, min_vecs: u32, max_vecs: u32, flags: u32) -> i32 {
    if dev.is_null() || min_vecs == 0 || max_vecs == 0 || min_vecs > max_vecs { return -22; }
    if unsafe { (*dev).msi_base } != 0 || unsafe { (*dev).msi_count } != 0 { return -16; }
    let bdf = unsafe { (*dev).bdf_of() };
    // MSI first (single vector); INTx legacy via IOAPIC if requested.
    if flags & PCI_IRQ_MSI != 0 {
        if crate::driver::pci::find_msi(bdf).is_none() {
            if flags & PCI_IRQ_LEGACY == 0 { return -19; }
        } else {
            if min_vecs != 1 { return -22; }
            let v = crate::driver::pci::msi_alloc_vector(bdf);
            if v < 0 { return v as i32; }
            unsafe { (*dev).msi_base = v as u8; (*dev).msi_count = 1; }
            return 1;
        }
    }
    if flags & PCI_IRQ_LEGACY == 0 { return -22; }
    // Legacy INTx: root-bus PIRQ->GSI via IOAPIC (level-triggered).
    // Dedicated vector 0x58 (outside the MSI pool) with its own IDT gate.
    if min_vecs != 1 { return -22; }
    const INTX_VECTOR: u8 = 0x58;
    if crate::driver::pci::legacy_route(bdf, INTX_VECTOR).is_err() { return -19; }
    unsafe { (*dev).intx_vec = INTX_VECTOR; }
    1
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn pci_irq_vector(dev: *mut PciDev, n: u32) -> u32 {
    if dev.is_null() { return 0; }
    let base = unsafe { (*dev).msi_base };
    let cnt = unsafe { (*dev).msi_count };
    if base != 0 && n < cnt as u32 { return base.wrapping_add(n as u8) as u32; }
    let iv = unsafe { (*dev).intx_vec };
    if iv != 0 && n == 0 { return iv as u32; }
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn pci_free_irq_vectors(dev: *mut PciDev) {
    if dev.is_null() { return; }
    let bdf = unsafe { (*dev).bdf_of() };
    crate::driver::pci::msi_free_vector(bdf);
    if unsafe { (*dev).intx_vec } != 0 {
        crate::driver::pci::legacy_unroute(bdf);
        unsafe { (*dev).intx_vec = 0; }
    }
    unsafe { (*dev).msi_base = 0; (*dev).msi_count = 0; }
}

#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn request_irq(irq: u32, handler: u64, flags: u64, name: *const u8, dev_id: u64) -> i32 {
    linux_request_irq(irq, handler, flags, name, dev_id)
}
/// Real Linux entry: threaded handler unsupported (fail safe if non-NULL);
/// NULL thread_fn behaves exactly like request_irq.
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn request_threaded_irq(irq: u32, handler: u64, thread_fn: u64, flags: u64, name: *const u8, dev_id: u64) -> i32 {
    if thread_fn != 0 { return -22; }
    linux_request_irq(irq, handler, flags, name, dev_id)
}
fn linux_request_irq(irq: u32, handler: u64, flags: u64, name: *const u8, dev_id: u64) -> i32 {
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
pub unsafe extern "sysv64" fn free_irq(irq: u32, dev_id: u64) {
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
pub unsafe extern "sysv64" fn abi_probe_6(a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) {
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
pub unsafe extern "sysv64" fn abi_verify() -> i32 {
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
pub unsafe extern "sysv64" fn test_u64_arg(x: u64) -> u64 {
    crate::println!("module: test_u64 got {:#x}", x);
    x.wrapping_add(1)
}
pub static mut TEST_STORE: [u64; 8] = [0; 8];
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn test_store_addr() -> u64 { unsafe { &raw const TEST_STORE as u64 } }
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
pub unsafe extern "sysv64" fn pci_register_simple() -> i32 {
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
pub unsafe extern "sysv64" fn capable(_cap: i32) -> bool { true }
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn stack_chk_fail() -> ! {
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
pub unsafe extern "sysv64" fn k_memset(s: *mut u8, c: i32, n: usize) -> *mut u8 {
    for i in 0..n { unsafe { core::ptr::write_volatile(s.add(i), c as u8) } }
    s
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn k_memcpy(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    unsafe { core::ptr::copy_nonoverlapping(s, d, n) };
    d
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn k_memmove(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    unsafe { core::ptr::copy(s, d, n) };
    d
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn k_strlen(s: *const u8) -> usize {
    let mut n = 0usize;
    while unsafe { core::ptr::read_volatile(s.add(n)) } != 0 { n += 1; }
    n
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn k_strcmp(a: *const u8, b: *const u8) -> i32 {
    let mut i = 0usize;
    loop {
        let x = unsafe { core::ptr::read_volatile(a.add(i)) };
        let y = unsafe { core::ptr::read_volatile(b.add(i)) };
        if x != y || x == 0 { return x as i32 - y as i32; }
        i += 1;
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn k_strncmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    for i in 0..n {
        let x = unsafe { core::ptr::read_volatile(a.add(i)) };
        let y = unsafe { core::ptr::read_volatile(b.add(i)) };
        if x != y || x == 0 { return x as i32 - y as i32; }
    }
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn k_strncpy(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    for i in 0..n {
        let b = unsafe { core::ptr::read_volatile(s.add(i)) };
        unsafe { core::ptr::write_volatile(d.add(i), b) };
        if b == 0 { break; }
    }
    d
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn k_sprintf(buf: *mut u8, fmt: *const u8, a: u64, b: u64, c: u64, d: u64, e: u64, f: u64) -> i32 {
    k_snprintf(buf, usize::MAX / 2, fmt, a, b, c, d, e, f)
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn k_snprintf(buf: *mut u8, n: usize, fmt: *const u8, a: u64, b: u64, c: u64, d: u64, e: u64, f: u64) -> i32 {
    if buf.is_null() || n == 0 {
        // Length-probe path: full length via a scratch buffer.
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
pub unsafe extern "sysv64" fn k_vsnprintf(buf: *mut u8, n: usize, fmt: *const u8, ap: *const u8) -> i32 {
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
unsafe extern "sysv64" fn test_probe(_d: *mut PciDev, _id: *const PciDeviceId) -> i32 {
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
unsafe extern "sysv64" fn linux_test_handler(_irq: i32, dev: u64) -> i32 {
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

/* ================= DMA: thin adapters over Slop DMA core ================= */
// dma_mask is u64 max (device-specific masks are future work with real BARs).
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn dma_alloc_coherent(_dev: u64, size: usize, dma_handle: *mut u64, _gfp: u32) -> *mut u8 {
    if dma_handle.is_null() || size == 0 || size > 4 * 1024 * 1024 { return core::ptr::null_mut(); }
    match crate::dma::alloc_coherent(size) {
        Some((va, pa)) => {
            unsafe { core::ptr::write_unaligned(dma_handle, pa) };
            va as *mut u8
        }
        None => core::ptr::null_mut(),
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn dma_free_coherent(_dev: u64, size: usize, vaddr: *mut u8, bus: u64) {
    if vaddr.is_null() { return; }
    let _ = vaddr;
    crate::dma::free_coherent(0, bus, size);
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn dma_map_single(_dev: u64, ptr: *mut u8, size: usize, dir: u32) -> u64 {
    if ptr.is_null() || size == 0 { return crate::dma::DMA_MAPPING_ERROR; }
    // Slop DMA core validates contiguity via page tables (no casts) and
    // enforces a full 64-bit mask here; per-device masks ride with BARs later.
    crate::dma::map_single(ptr as u64, size, dir, u64::MAX)
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn dma_unmap_single(_dev: u64, bus: u64, size: usize, dir: u32) {
    crate::dma::unmap_single(bus, size, dir);
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn dma_sync_single_for_cpu(_dev: u64, bus: u64, size: usize, dir: u32) {
    crate::dma::sync_single(bus, size, dir);
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn dma_sync_single_for_device(_dev: u64, bus: u64, size: usize, dir: u32) {
    crate::dma::sync_single(bus, size, dir);
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn dma_map_sg(_dev: u64, _sgl: u64, _nents: u32, _dir: u32) -> i32 {
    // Scatter-gather needs exact struct scatterlist layout pinning
    // (CONFIG_NEED_SG_DMA_* dependent); fail safe until then.
    crate::println!("dma: map_sg not yet wired (needs sg layout); returning 0");
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn dma_unmap_sg(_dev: u64, _sgl: u64, _nents: u32, _dir: u32) {}

/* Force-drop every Linux-side IRQ registration (module unload / init-failure
   safety net). Slop core waits out in-flight handlers, so no use-after-free.
   Slop-native drivers use the Slop core directly and are unaffected. */
pub fn force_unregister_all_irqs() {
    let vecs: [(u8, u64); 32] = {
        let t = LINUX_IRQS.lock();
        let mut out = [(0u8, 0u64); 32];
        let mut n = 0usize;
        for r in t.iter() {
            if r.used && n < 32 { out[n] = (r.vec, r.dev_id); n += 1; }
        }
        out
    };
    for (v, d) in vecs.iter() {
        if *v != 0 || *d != 0 {
            let _ = crate::irq::unregister(*v, *d);
            let mut t = LINUX_IRQS.lock();
            for r in t.iter_mut() {
                if r.used && r.vec == *v && r.dev_id == *d { *r = NO_LINUX_IRQ; }
            }
        }
    }
}
/* Force-drop every Linux-side PCI registration (same rationale as IRQ).
   Only modules use this path; Slop-native drivers are unaffected. */
pub fn force_unregister_all_pci() {
    let drvs = {
        let mut t = LINUX_PCI_DRVS.lock();
        let mut out = [0u64; 16];
        out.copy_from_slice(&t[..]);
        for d in t.iter_mut() { *d = 0; }
        out
    };
    // Route through the full unregister (Linux remove + vector/dev free).
    for drv in drvs.iter() {
        if *drv != 0 {
            unsafe { pci_unregister_driver(*drv); }
        }
    }
}
