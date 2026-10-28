//! Minimal kernel heap for loadable Linux drivers and the module loader.
//!
//! Page-granular bump+free allocator on top of the frame allocator: every
//! allocation occupies whole 4 KiB frames, with the byte length stored in an
//! 8-byte header so `kfree` needs no size argument. Single-page allocations
//! (<=4088 bytes) are supported; larger requests fail with null so callers
//! fall back or fail probe cleanly. A slab allocator for sub-page objects is
//! future work — correctness first, density later.
//!
//! All entry points are safe to call from module init/probe paths (they take
//! the same frame-allocator lock as the rest of the kernel; callers must not
//! hold that lock, which they never do through this API).
use crate::memory::{alloc_frame, free_frame};
use crate::arch::x86_64::phys::phys_to_virt;

pub const PAGE: usize = 4096;
const HDR: usize = 8;
pub const MAX_KMALLOC: usize = PAGE - HDR;

/* GFP flags (accepted and ignored: our pages are always sleepable). */
pub const GFP_KERNEL: u32 = 0;
pub const GFP_ATOMIC: u32 = 0;
pub const GFP_DMA: u32 = 0;
pub const GFP_DMA32: u32 = 0;

/* Allocate `size` bytes, page-granular. Returns null on OOM or oversize. */
pub fn kmalloc(size: usize) -> *mut u8 {
    if size == 0 || size > MAX_KMALLOC { return core::ptr::null_mut(); }
    match alloc_frame() {
        Some(pa) => {
            let base = phys_to_virt(pa) as *mut u8;
            unsafe {
                core::ptr::write_bytes(base, 0, PAGE);
                *(base as *mut usize) = size;
                base.add(HDR)
            }
        }
        None => core::ptr::null_mut(),
    }
}

/* Allocate zeroed memory. */
pub fn kzalloc(size: usize) -> *mut u8 {
    // kmalloc already zeroes the whole page.
    kmalloc(size)
}

/* Allocate an array (n * size, checked for overflow), zeroed. */
pub fn kcalloc(n: usize, size: usize) -> *mut u8 {
    match n.checked_mul(size) {
        Some(total) => kzalloc(total),
        None => core::ptr::null_mut(),
    }
}

/* Duplicate a buffer. `src` is a kernel address. */
pub fn kmemdup(src: *const u8, len: usize) -> *mut u8 {
    if src.is_null() || len == 0 || len > MAX_KMALLOC { return core::ptr::null_mut(); }
    let dst = kmalloc(len);
    if dst.is_null() { return dst; }
    unsafe { core::ptr::copy_nonoverlapping(src, dst, len); }
    dst
}

/* Free a pointer from kmalloc/kzalloc/kcalloc/kmemdup. Null is a no-op. */
pub fn kfree(ptr: *mut u8) {
    if ptr.is_null() { return; }
    let base = unsafe { ptr.sub(HDR) } as u64;
    let pa = base - crate::arch::x86_64::phys::PHYS_BASE;
    let _ = free_frame(pa);
}

/* Usable size of an allocation (for krealloc decisions). */
pub fn ksize(ptr: *mut u8) -> usize {
    if ptr.is_null() { return 0; }
    let base = unsafe { ptr.sub(HDR) } as *const usize;
    let want = unsafe { core::ptr::read_volatile(base) };
    if want <= MAX_KMALLOC { want } else { 0 }
}
