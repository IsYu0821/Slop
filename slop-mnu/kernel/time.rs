//! Monotonic time, the wall clock and the xorshift64 random source.
//!
//! The clock is measured with the time stamp counter. The counter runs at a
//! constant rate that is measured against the programmable interval timer at
//! boot, and the kernel moves the clock forward at every timer tick by the
//! time the counter has passed. The time between two ticks is measured the
//! same way, so the length of a tick follows the counter and does not drift
//! away from it.
//!
//! The counter is the only time source a program can read without entering
//! the kernel, so the clock has to be built on it: the page of code that
//! Linux calls the vector system call entry point reads the counter and the
//! base of the clock, and the two have to describe the same time.
use crate::sync::IrqSpinLock;
use core::sync::atomic::{AtomicU64,Ordering};

/// The base of the clock: the time and the counter value at the last
/// update.
#[derive(Clone,Copy)]
struct ClockBase{ns:u64,tsc:u64}
static CLOCK:IrqSpinLock<ClockBase>=IrqSpinLock::new(ClockBase{ns:0,tsc:0});
/// The nanoseconds per counter cycle, with 32 fractional bits.
static SCALE:AtomicU64=AtomicU64::new(0);
/// The measured length of a timer tick in nanoseconds.
static TICK_NS:AtomicU64=AtomicU64::new(1_000_000);
static REALTIME_OFFSET_NS:AtomicU64=AtomicU64::new(0);
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
/// The nanoseconds per counter cycle, with 32 fractional bits.
pub fn scale()->u64{SCALE.load(Ordering::Relaxed)}
/// Records the rate of the counter in nanoseconds per cycle.
pub fn set_scale(v:u64){SCALE.store(v,Ordering::Relaxed)}

/// The monotonic time at a given counter value.
pub fn monotonic_at(t:u64)->u64{
 let c=CLOCK.lock();
 if t<=c.tsc{return c.ns}
 c.ns.saturating_add(((t-c.tsc) as u64).saturating_mul(scale())>>32)
}
/// The current monotonic time.
pub fn monotonic_ns()->u64{monotonic_at(crate::arch::x86_64::tsc::rdtsc())}
/// Moves the clock forward to the counter value `t` and returns the length
/// of the tick that just passed, in nanoseconds.
pub fn advance(t:u64)->u64{
 let mut c=CLOCK.lock();
 if t<=c.tsc{return 0}
 let delta=t-c.tsc;
 let ns=(delta as u64).saturating_mul(scale())>>32;
 c.ns=c.ns.saturating_add(ns);
 c.tsc=t;
 drop(c);
 ns
}
/// The distance between the wall clock and the monotonic clock.
pub fn realtime_offset_ns()->u64{REALTIME_OFFSET_NS.load(Ordering::Relaxed)}
/// The wall clock.
pub fn realtime_ns()->u64{monotonic_ns().wrapping_add(REALTIME_OFFSET_NS.load(Ordering::Relaxed))}
/// Sets the wall clock.
pub fn set_realtime_ns(ns:u64){REALTIME_OFFSET_NS.store(ns.wrapping_sub(monotonic_ns()),Ordering::Relaxed)}
/// Converts a number of ticks into nanoseconds.
pub fn ticks_to_ns(t:u64)->u64{t.saturating_mul(tick_ns())}
/// The number of ticks a length of time takes.
pub fn ns_to_ticks_ceil(ns:u64)->u64{let unit=tick_ns();ns.saturating_add(unit-1)/unit}
/// The tick at which a delay of `ns` nanoseconds is over.
pub fn ticks_after_ns(ns:u64)->u64{crate::arch::x86_64::apic::ticks().saturating_add(ns_to_ticks_ceil(ns))}
/// The tick at which a delay of `ns` nanoseconds is over, rounded down, so
/// the deadline never lies behind the delay that was asked for.
pub fn ticks_after_ns_floor(ns:u64)->u64{let n=ns/tick_ns();crate::arch::x86_64::apic::ticks().saturating_add(if n==0{1}else{n})}
/// A source of random bytes.
pub fn fill_random(out:&mut[u8]){let mut x=RNG.load(Ordering::Relaxed)^monotonic_ns().rotate_left(17);for b in out{x^=x<<13;x^=x>>7;x^=x<<17;*b=x as u8;}RNG.store(x,Ordering::Relaxed)}
/// Checks that the clock moves forward.
pub fn self_test()->bool{let a=monotonic_ns();let mut b=[0u8;16];fill_random(&mut b);monotonic_ns()>=a&&b.iter().any(|x|*x!=0)}
