//! Monotonic bootstrap time and xorshift64 random source.
use core::sync::atomic::{AtomicU64,Ordering};
const NS_PER_TICK:u64=1_000_000;
static RNG:AtomicU64=AtomicU64::new(0x6e6f_7274_6873_7461);
pub fn monotonic_ns()->u64{crate::arch::x86_64::apic::ticks().saturating_mul(NS_PER_TICK)}
pub fn ticks_after_ns(ns:u64)->u64{crate::arch::x86_64::apic::ticks().saturating_add(ns.saturating_add(NS_PER_TICK-1)/NS_PER_TICK)}
pub fn fill_random(out:&mut[u8]){let mut x=RNG.load(Ordering::Relaxed)^monotonic_ns().rotate_left(17);for b in out{x^=x<<13;x^=x>>7;x^=x<<17;*b=x as u8;}RNG.store(x,Ordering::Relaxed)}
pub fn self_test()->bool{let a=monotonic_ns();let mut b=[0u8;16];fill_random(&mut b);monotonic_ns()>=a&&b.iter().any(|x|*x!=0)}
