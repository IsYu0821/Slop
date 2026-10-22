//! The programmable interval timer, used to measure the rate of the time
//! stamp counter.
//!
//! The 8254 timer counts at a fixed rate that the processor does not know
//! about. Channel 2 is wired to the status port of the sound card, so the
//! line it drives can be watched while it counts down. That gives a window
//! of exactly known length, and the number of counter cycles inside the
//! window is the rate of the counter.

use crate::arch::x86_64::port::{inb,outb};
use crate::arch::x86_64::tsc::rdtsc;

/// The input rate of the 8254 timer, in counts per second.
pub const INPUT_HZ:u64=1_193_182;

/// The count register of channel 2 is sixteen bits wide, so the longest
/// window is 65536 counts, a little over 54 milliseconds.
pub const MAX_COUNTS:u16=0xffff;

/// Measures how many counter cycles pass while channel 2 counts down
/// `counts` ticks. Returns None when the timer does not finish, which
/// happens on a machine without the sound card status port.
pub fn measure_tsc(counts:u16)->Option<u64>{
 if counts==0{return None}
 unsafe{
  /* The gate of channel 2 drives the status line back to the status port.
     The line reads high while the channel is idle and low while it counts,
     so the start of the window is the moment the line falls. */
  let control=inb(0x61);
  outb(0x61,(control & !0x02)|0x01);
  /* Channel 2, low byte then high byte, mode 0, binary. */
  outb(0x43,0xb0);
  outb(0x42,(counts&0xff)as u8);
  outb(0x42,(counts>>8)as u8);
  let start=rdtsc();
  let mut spins=0u32;
  while inb(0x61)&0x20==0{
   spins+=1;
   /* The window cannot be longer than the longest count, so a counter that
      runs far past that means the status line never moved. */
   if spins>2_000_000{outb(0x61,control);return None}
  }
  let end=rdtsc();
  outb(0x61,control);
  Some(end-start)
 }
}

/// Measures the rate of the time stamp counter in cycles per second. The
/// result is rounded to the nearest cycle per second.
pub fn tsc_hz()->Option<u64>{
 let counts=MAX_COUNTS;
 let cycles=measure_tsc(counts)?;
 if cycles==0{return None}
 Some((cycles as u64*INPUT_HZ+(counts as u64)/2)/(counts as u64))
}
