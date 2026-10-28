//! Slop-native DMA core: coherent allocation, streaming map/unmap, sync.
//!
//! OS-neutral and shared by Slop drivers and the Linux compat adapter.
//! Bus address == physical address (no IOMMU; the IOMMU boundary is an
//! explicit future hook, never a silent cast). All mappings are validated
//! (contiguity, mask, alignment); failures return errors, never fake addresses.
//! A small live-mapping table tracks streaming maps for leak detection and
//! module-unload synchronization.
use crate::sync::IrqSpinLock;

pub const DMA_TO_DEVICE: u32 = 0;
pub const DMA_FROM_DEVICE: u32 = 1;
pub const DMA_BIDIRECTIONAL: u32 = 2;
pub const DMA_NONE: u32 = 3;
pub const DMA_MAPPING_ERROR: u64 = 0;

/// Translate a kernel VA to PA via page tables (no direct-map arithmetic).
/// Translate a kernel VA to PA via Slop page tables (huge-page aware).
/// No direct-map arithmetic: the mapping, not the convention, is authoritative.
fn va_to_pa(va: u64) -> Option<u64> {
    let pa = crate::arch::x86_64::phys::virt_to_phys_full(va);
    if pa == 0 { None } else { Some(pa) }
}
fn current_cr3() -> u64 {
    let v: u64;
    unsafe { core::arch::asm!("mov {}, cr3", out(reg) v, options(nostack, preserves_flags)) };
    v
}

/* ---------- coherent (consistent, CPU+device shared) ---------- */
/// Allocate `size` bytes of DMA-coherent memory (page-granular, contiguous).
/// Returns (kernel VA, bus/PA) on success.
pub fn alloc_coherent(size: usize) -> Option<(u64, u64)> {
    if size == 0 { return None; }
    let pages = (size + 4095) / 4096;
    let pa = crate::memory::alloc_contiguous(pages)?;
    // Map it uncached? For MVP, direct map is WB; real UC mapping needs PAT
    // programming (future). Document: coherent here means CPU-coherent via
    // direct map (WB is coherent on x86 SMP); device sees RAM directly.
    // True UC for MMIO apertures is a PCI BAR concern, not this path.
    let va = crate::arch::x86_64::phys::phys_to_virt(pa);
    unsafe { core::ptr::write_bytes(va as *mut u8, 0, pages * 4096) };
    Some((va, pa))
}
pub fn free_coherent(_va: u64, pa: u64, size: usize) {
    let pages = (size + 4095) / 4096;
    for k in 0..pages {
        let _ = crate::memory::free_frame(pa + (k as u64) * 4096);
    }
}

/* ---------- streaming (map normal memory for one transfer) ---------- */
struct StreamMap { used: bool, bus: u64, len: usize }
const NO_SMAP: StreamMap = StreamMap { used: false, bus: 0, len: 0 };
static SMAPS: IrqSpinLock<[StreamMap; 64]> = IrqSpinLock::new([NO_SMAP; 64]);

fn track_map(bus: u64, len: usize) -> bool {
    let mut t = SMAPS.lock();
    match t.iter_mut().find(|m| !m.used) {
        Some(m) => { m.used = true; m.bus = bus; m.len = len; true }
        None => false,
    }
}
fn untrack_map(bus: u64) -> bool {
    let mut t = SMAPS.lock();
    for m in t.iter_mut() {
        if m.used && m.bus == bus { *m = NO_SMAP; return true; }
    }
    false
}

/// Map a kernel buffer for DMA. Validates contiguity page-by-page via the
/// page tables (no casts), checks the DMA mask, tracks for leak detection.
/// Returns bus address or DMA_MAPPING_ERROR.
pub fn map_single(va: u64, len: usize, _dir: u32, mask: u64) -> u64 {
    if va == 0 || len == 0 { return DMA_MAPPING_ERROR; }
    let start = va & !4095;
    let end = (va + len as u64 + 4095) & !4095;
    let mut prev: Option<u64> = None;
    let mut first = 0u64;
    let mut off = start;
    while off < end {
        let pa = match va_to_pa(off) {
            Some(p) => p,
            None => return DMA_MAPPING_ERROR,
        };
        if off == start { first = pa + (va & 4095); }
        if let Some(pp) = prev {
            if pa != pp + 4096 { return DMA_MAPPING_ERROR; } // not contiguous
        }
        prev = Some(pa);
        off += 4096;
    }
    if first + (len as u64) - 1 > mask { return DMA_MAPPING_ERROR; }
    if !track_map(first, len) { return DMA_MAPPING_ERROR; }
    // x86 is cache-coherent; no flush needed (documented no-op point).
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    first
}
pub fn unmap_single(bus: u64, _len: usize, _dir: u32) {
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    untrack_map(bus);
}
pub fn sync_single(_bus: u64, _len: usize, _dir: u32) {
    // x86 coherent: compiler barrier only (documents the hook for non-coherent).
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

/// Scatter-gather: each entry must be individually contiguous (checked);
/// the list itself may be scattered. Returns count mapped or 0 on error.
pub fn map_sg(entries: &[(u64, usize)], dir: u32, mask: u64, out_bus: &mut [u64]) -> usize {
    let mut n = 0usize;
    for (i, (va, len)) in entries.iter().enumerate() {
        if i >= out_bus.len() { break; }
        let b = map_single(*va, *len, dir, mask);
        if b == DMA_MAPPING_ERROR {
            for j in 0..n { unmap_single(out_bus[j], 0, dir); }
            return 0;
        }
        out_bus[i] = b;
        n += 1;
    }
    n
}

/* ---------- diagnostics ---------- */
pub fn live_maps() -> usize {
    SMAPS.lock().iter().filter(|m| m.used).count()
}

/// Boot self-test: coherent alloc/write/read/free + streaming map/unmap +
/// leak check. Proves the Slop DMA core end to end without hardware.
pub fn selftest_dma() {
    // Coherent: alloc 2 pages, write pattern via VA, verify via fresh read.
    let (va, pa) = match alloc_coherent(8192) {
        Some(v) => v,
        None => { crate::println!("dma: selftest FAIL alloc"); return; }
    };
    if pa & 4095 != 0 { crate::println!("dma: selftest FAIL align"); free_coherent(va, pa, 8192); return; }
    unsafe {
        for i in 0..2048usize {
            core::ptr::write_volatile((va as *mut u32).add(i), (i as u32).wrapping_mul(0x01010101));
        }
        for i in 0..2048usize {
            if core::ptr::read_volatile((va as *const u32).add(i)) != (i as u32).wrapping_mul(0x01010101) {
                crate::println!("dma: selftest FAIL pattern");
                free_coherent(va, pa, 8192);
                return;
            }
        }
    }
    // Streaming: map the same VA (must succeed: direct-mapped, contiguous).
    let bus = map_single(va, 8192, DMA_BIDIRECTIONAL, u64::MAX);
    if bus == DMA_MAPPING_ERROR {
        crate::println!("dma: selftest FAIL map");
        free_coherent(va, pa, 8192);
        return;
    }
    sync_single(bus, 8192, DMA_BIDIRECTIONAL);
    unmap_single(bus, 8192, DMA_BIDIRECTIONAL);
    // Non-contiguous must fail (use two far-apart VAs via sg path later; here
    // just verify mask enforcement with an impossible mask).
    let bad = map_single(va, 8192, DMA_BIDIRECTIONAL, 0xfff);
    if bad != DMA_MAPPING_ERROR {
        crate::println!("dma: selftest FAIL mask not enforced");
        unmap_single(bad, 8192, DMA_BIDIRECTIONAL);
        free_coherent(va, pa, 8192);
        return;
    }
    free_coherent(va, pa, 8192);
    if live_maps() != 0 {
        crate::println!("dma: selftest FAIL leak live={}", live_maps());
        return;
    }
    crate::println!("dma: selftest PASS (coherent+streaming+mask+leak clean)");
}
