//! xAPIC and first IOAPIC support for the bootstrap processor.
use crate::acpi::InterruptInfo;
use core::sync::atomic::{AtomicU64, Ordering};

pub const TIMER_VECTOR: u8 = 0x40;
pub const SPURIOUS_VECTOR: u8 = 0xff;

static mut LAPIC: u64 = 0;
static TICKS: AtomicU64 = AtomicU64::new(0);

#[inline]
unsafe fn write(off: u64, v: u32) {
    unsafe { ((LAPIC + off) as *mut u32).write_volatile(v) }
}

#[inline]
unsafe fn read(off: u64) -> u32 {
    unsafe { ((LAPIC + off) as *const u32).read_volatile() }
}

unsafe fn io_write(base: u64, reg: u8, v: u32) {
    unsafe {
        (base as *mut u32).write_volatile(reg as u32);
        ((base + 0x10) as *mut u32).write_volatile(v);
    }
}

unsafe fn io_read(base: u64, reg: u8) -> u32 {
    unsafe {
        (base as *mut u32).write_volatile(reg as u32);
        ((base + 0x10) as *const u32).read_volatile()
    }
}

unsafe fn mask_pic() {
    unsafe {
        core::arch::asm!(
            "out dx, al",
            in("dx") 0x21u16,
            in("al") 0xffu8,
            options(nomem, nostack, preserves_flags)
        );
        core::arch::asm!(
            "out dx, al",
            in("dx") 0xa1u16,
            in("al") 0xffu8,
            options(nomem, nostack, preserves_flags)
        );
    }
}

pub unsafe fn init(info: InterruptInfo) -> Result<(), &'static str> {
    if info.lapic == 0 {
        return Err("LAPIC missing");
    }
    if info.ioapic == 0 {
        return Err("IOAPIC missing");
    }
    unsafe {
        mask_pic();
        LAPIC = super::phys::phys_to_virt(info.lapic);
        let old = super::msr::rdmsr(super::msr::IA32_APIC_BASE);
        let base = (info.lapic & 0xffff_f000) | (old & 0xfff) | (1 << 11);
        super::msr::wrmsr(super::msr::IA32_APIC_BASE, base);
        write(0xf0, SPURIOUS_VECTOR as u32 | (1 << 8));
        write(0x80, 0);
        write(0x350, 1 << 16);
        write(0x360, 1 << 16);
        let ioapic = super::phys::phys_to_virt(info.ioapic);
        let max = ((io_read(ioapic, 1) >> 16) & 0xff) as u8;
        for pin in 0..=max {
            io_write(ioapic, 0x10 + pin * 2, (1 << 16) | (0x20 + pin as u32));
            io_write(ioapic, 0x11 + pin * 2, 0);
        }
        // Periodic bootstrap tick. Frequency is intentionally uncalibrated until HPET/PIT calibration is added.
        write(0x3e0, 0x3);
        write(0x320,TIMER_VECTOR as u32|(1<<17));
        write(0x380, 100_000);
        TIMER_COUNT = 100_000;
        let _ = read(0x390);
    }
    Ok(())
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

/// The count the timer was started with. The length of a tick follows from
/// it and from the rate of the bus the timer counts at.
static mut TIMER_COUNT: u32 = 0;

/* The length of a tick is measured inside the interrupt, where the counter
   is read a few cycles after the interrupt arrives. The span of a run of
   ticks is the sum of the gaps between them, so the error of the delay of
   the interrupt cancels out. */
static LAST_TSC: AtomicU64 = AtomicU64::new(0);
static TSC_TOTAL: AtomicU64 = AtomicU64::new(0);
static TSC_SAMPLES: AtomicU64 = AtomicU64::new(0);

/// A reading of the counter that the timer interrupt keeps.
#[derive(Clone, Copy)]
pub struct TickSample {
    pub total: u64,
    pub samples: u64,
}

/// The current reading of the counter that the timer interrupt keeps.
pub fn tick_sample() -> TickSample {
    TickSample {
        total: TSC_TOTAL.load(Ordering::Relaxed),
        samples: TSC_SAMPLES.load(Ordering::Relaxed),
    }
}

/// The number of counter cycles one tick takes, averaged over the ticks
/// between the two readings.
pub fn tick_cycles(a: TickSample, b: TickSample) -> Option<u64> {
    let n = b.samples.wrapping_sub(a.samples);
    if n == 0 {
        return None;
    }
    Some(b.total.wrapping_sub(a.total) / n)
}

/// Waits for the timer to run for a while and reports the length of a tick
/// in counter cycles. The interrupts have to be on for the timer to run.
pub fn measure_tick_cycles(ticks_to_watch: u64) -> Option<u64> {
    let start = ticks();
    let before = tick_sample();
    let mut spins = 0u32;
    while ticks().wrapping_sub(start) < ticks_to_watch {
        core::hint::spin_loop();
        spins += 1;
        if spins > 2_000_000_000 {
            return None;
        }
    }
    let after = tick_sample();
    tick_cycles(before, after)
}

/// Programs the timer so that a tick lasts `target_ns` nanoseconds. The
/// length of a tick follows the count of the timer, so the count is scaled
/// with the length that was measured. Returns the count that was
/// programmed.
pub fn program_tick_ns(target_ns: u64, measured_ns: u64) -> Option<u32> {
    if measured_ns == 0 {
        return None;
    }
    let old = unsafe { TIMER_COUNT } as u64;
    let count = (old.saturating_mul(target_ns) / measured_ns).clamp(1, 0x00ff_ffff) as u32;
    unsafe {
        TIMER_COUNT = count;
        write(0x380, count);
        let _ = read(0x390);
    }
    Some(count)
}

pub fn self_test() -> Result<u64, &'static str> {
    let before = ticks();
    for _ in 0..8 {
        unsafe { core::arch::asm!("sti", "hlt", "cli", options(nomem, nostack)); }
        let now = ticks();
        if now != before {
            return Ok(now);
        }
    }
    Err("Local APIC timer did not fire")
}

pub unsafe fn route_isa(info: InterruptInfo, irq: u8, vector: u8, masked: bool) -> Result<(), &'static str> {
    if irq >= 16 || vector < 0x20 {
        return Err("invalid IRQ route");
    }
    let gsi = info.iso_gsi[irq as usize];
    if gsi < info.ioapic_gsi {
        return Err("GSI below IOAPIC base");
    }
    let pin = gsi - info.ioapic_gsi;
    let ioapic = unsafe { super::phys::phys_to_virt(info.ioapic) };
    let max = ((unsafe { io_read(ioapic, 1) } >> 16) & 0xff) as u32;
    if pin > max {
        return Err("GSI outside IOAPIC");
    }
    let flags = info.iso_flags[irq as usize];
    let low = (vector as u32)
        | if flags & 2 != 0 { 1 << 13 } else { 0 }
        | if flags & 8 != 0 { 1 << 15 } else { 0 }
        | if masked { 1 << 16 } else { 0 };
    let dest = unsafe { read(0x20) } & 0xff00_0000;
    unsafe {
        io_write(ioapic, 0x10 + (pin as u8) * 2, low);
        io_write(ioapic, 0x11 + (pin as u8) * 2, dest);
    }
    Ok(())
}

#[unsafe(no_mangle)]
pub extern "sysv64" fn apic_timer_rust(cs:u64) {
    let now = super::tsc::rdtsc();
    let last = LAST_TSC.swap(now, Ordering::Relaxed);
    if last != 0 && now > last {
        TSC_TOTAL.fetch_add(now - last, Ordering::Relaxed);
        TSC_SAMPLES.fetch_add(1, Ordering::Relaxed);
    }
    TICKS.fetch_add(1, Ordering::Relaxed);
    /* The vector system call entry point reads the time from a shared page
       that the kernel keeps up to date. */
    crate::vdso::update();
    crate::task::on_timer_tick(cs & 3 != 0);
    crate::net::poll();
    unsafe { write(0xb0,0) }
}

pub fn eoi() {
    unsafe { write(0xb0,0) }
}
