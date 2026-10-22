//! The time stamp counter.
//!
//! The counter runs at a constant rate that does not depend on the load, so
//! it is the reference every other time source is measured against. The
//! rate itself is not known from the processor, it has to be measured
//! against a real time source; the programmable interval timer does that
//! (see `pit.rs`).

/// Reads the time stamp counter. The fences keep the read from being moved
/// across the instructions that measure the interval around it.
#[inline]pub fn rdtsc()->u64{
 let lo:u32;let hi:u32;
 unsafe{core::arch::asm!("lfence","rdtsc","lfence",out("eax")lo,out("edx")hi,options(nomem,preserves_flags));}
 ((hi as u64)<<32)|(lo as u64)
}

/// True when the processor reports an invariant counter, so the rate that was
/// measured once stays valid.
pub fn invariant()->bool{
 let max=unsafe{core::arch::x86_64::__cpuid_count(0x8000_0000,0)};
 if max.eax<0x8000_0007{return false}
 let r=unsafe{core::arch::x86_64::__cpuid(0x8000_0007)};
 r.edx&1!=0
}
