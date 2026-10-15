pub mod x86_64;

#[inline(always)]
pub fn halt() -> ! { loop { unsafe { core::arch::asm!("cli; hlt", options(nomem, nostack)); } } }
