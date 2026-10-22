//! Monotonic time, the wall clock and the xorshift64 random source.
use core::sync::atomic::{AtomicU64,Ordering};
/* The length of one timer tick in nanoseconds. The timer of the machine
   does not run at a rate the processor reports, so the length is measured
   at boot against the time stamp counter. The default is the rate a program
   expects until the measurement replaces it. */
const DEFAULT_TICK_NS:u64=1_000_000;
static TICK_NS:AtomicU64=AtomicU64::new(DEFAULT_TICK_NS);
static TSC_HZ:AtomicU64=AtomicU64::new(0);
static RNG:AtomicU64=AtomicU64::new(0x6e6f_7274_6873_7461);
/// The length of one timer tick in nanoseconds.
pub fn tick_ns()->u64{TICK_NS.load(Ordering::Relaxed)}
/// Records the measured length of a timer tick.
pub fn set_tick_ns(ns:u64){if ns>0{TICK_NS.store(ns,Ordering::Relaxed)}}
/// The rate of the time stamp counter in cycles per second, zero when the
/// counter could not be measured.
pub fn tsc_hz()->u64{TSC_HZ.load(Ordering::Relaxed)}
/// Records the measured rate of the time stamp counter.
pub fn set_tsc_hz(hz:u64){TSC_HZ.store(hz,Ordering::Relaxed)}
pub fn monotonic_ns()->u64{crate::arch::x86_64::apic::ticks().saturating_mul(tick_ns())}
static REALTIME_OFFSET_NS:AtomicU64=AtomicU64::new(0);
pub fn realtime_ns()->u64{monotonic_ns().wrapping_add(REALTIME_OFFSET_NS.load(Ordering::Relaxed))}
pub fn set_realtime_ns(ns:u64){REALTIME_OFFSET_NS.store(ns.wrapping_sub(monotonic_ns()),Ordering::Relaxed)}
pub fn ticks_to_ns(t:u64)->u64{t.saturating_mul(tick_ns())}
pub fn ns_to_ticks_ceil(ns:u64)->u64{let unit=tick_ns();ns.saturating_add(unit-1)/unit}
pub fn ticks_after_ns(ns:u64)->u64{crate::arch::x86_64::apic::ticks().saturating_add(ns_to_ticks_ceil(ns))}
/// The tick at which a delay of `ns` nanoseconds is over. The delay is
/// rounded down, so the deadline never lies behind the delay that was asked
/// for. A delay shorter than a tick still waits for a tick.
pub fn ticks_after_ns_floor(ns:u64)->u64{let n=ns/tick_ns();crate::arch::x86_64::apic::ticks().saturating_add(if n==0{1}else{n})}
pub fn fill_random(out:&mut[u8]){let mut x=RNG.load(Ordering::Relaxed)^monotonic_ns().rotate_left(17);for b in out{x^=x<<13;x^=x>>7;x^=x<<17;*b=x as u8;}RNG.store(x,Ordering::Relaxed)}
pub fn self_test()->bool{let a=monotonic_ns();let mut b=[0u8;16];fill_random(&mut b);monotonic_ns()>=a&&b.iter().any(|x|*x!=0)}
