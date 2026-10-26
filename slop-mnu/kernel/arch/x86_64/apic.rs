//! xAPIC and first IOAPIC support for the bootstrap processor.
use crate::acpi::InterruptInfo;
use core::sync::atomic::{AtomicU64, Ordering};

pub const TIMER_VECTOR: u8 = 0x40;
/* Cross-CPU reschedule request (MS83): fixed-delivery IPI, no LVT entry. */
pub const RESCHED_VECTOR: u8 = 0x41;
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
    /* The clock is measured with the counter, so the tick moves it forward
       and the length of the tick is the time the counter has passed. */
    let tick_ns=crate::time::advance(now);
    if tick_ns>0{crate::time::set_tick_ns(tick_ns);}
    /* The vector system call entry point reads the time from a shared page
       that the kernel keeps up to date. */
    crate::vdso::update(now);
    crate::task::on_timer_tick(cs & 3 != 0,tick_ns);
    crate::net::poll();
    unsafe { write(0xb0,0) }
}

pub fn eoi() {
    unsafe { write(0xb0,0) }
}

/* Reads the caller's Local APIC ID (xAPIC ID register, top byte). */
pub fn apic_id() -> u32 {
    unsafe { read(0x20) >> 24 }
}

const ICR_LOW: u64 = 0x300;
const ICR_HIGH: u64 = 0x310;
/* Delivery-status bit: set while an IPI is in flight. */
const ICR_PENDING: u32 = 1 << 12;

/* Spins until no IPI is in flight, bounded so a dead APIC cannot hang
   the boot forever. Returns false on timeout. */
fn wait_icr_idle(mut spins: u64) -> bool {
    while unsafe { read(ICR_LOW) } & ICR_PENDING != 0 {
        core::hint::spin_loop();
        if spins == 0 {
            return false;
        }
        spins -= 1;
    }
    true
}

fn send_ipi(dest: u32, low: u32) -> bool {
    if !wait_icr_idle(20_000_000) {
        return false;
    }
    unsafe {
        write(ICR_HIGH, dest << 24);
        write(ICR_LOW, low);
    }
    wait_icr_idle(20_000_000)
}

/* Asserts INIT to one AP (level-triggered, level high). */
pub fn send_init(dest: u32) -> bool {
    send_ipi(dest, 0x0000_c500)
}

/* Deasserts a previously asserted INIT (level-triggered, level low). */
pub fn send_init_deassert(dest: u32) -> bool {
    send_ipi(dest, 0x0000_8500)
}

/* Sends a startup IPI naming the vector page (vector = page >> 12).
   Encoded exactly like Linux does: delivery mode STARTUP with no
   level/trigger bits, which QEMU and real hardware both accept. */
pub fn send_sipi(dest: u32, vector: u8) -> bool {
    send_ipi(dest, 0x0000_0600 | vector as u32)
}

/* Sends a fixed-delivery IPI for the given vector to one APIC ID.
   Used for cross-CPU reschedule requests (MS83); shorthand and
   trigger bits stay clear, destination is physical. */
pub fn send_fixed_ipi(dest: u32, vector: u8) -> bool {
    send_ipi(dest, 0x0000_0000 | vector as u32)
}

/* Requests a reschedule on one AP. Harmless when the target is idle:
   the flag is still global (only the BSP runs tasks), so it just
   marks a reschedule pending for the next yield point. */
pub fn send_resched_ipi(dest: u32) -> bool {
    send_fixed_ipi(dest, RESCHED_VECTOR)
}

#[unsafe(no_mangle)]
pub extern "sysv64" fn apic_resched_rust() {
    crate::task::request_resched();
    unsafe { write(0xb0, 0) }
}

/* Enables a secondary CPU's Local APIC for the parked state: the
   spurious vector is installed so stray vectors have somewhere to go
   and every LVT entry is masked so the parked CPU takes no interrupts
   (it runs with IF clear until the scheduler owns it). */
pub unsafe fn ap_enable() {
    unsafe {
        write(0xf0, SPURIOUS_VECTOR as u32 | (1 << 8));
        write(0x320, 1 << 16);
        write(0x350, 1 << 16);
        write(0x360, 1 << 16);
        write(0x370, 1 << 16);
        write(0x330, 1 << 16);
        write(0xb0, 0);
    }
}
