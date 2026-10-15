use core::fmt::{self,Write};
const COM1:u16=0x3f8;const LSR:u16=COM1+5;const THR_EMPTY:u8=1<<5;const TX_EMPTY:u8=1<<6;
struct Serial;
impl Write for Serial{fn write_str(&mut self,s:&str)->fmt::Result{for b in s.bytes(){write_byte(b)}Ok(())}}
#[inline]unsafe fn outb(port:u16,value:u8){unsafe{core::arch::asm!("out dx, al",in("dx")port,in("al")value,options(nomem,nostack,preserves_flags));}}
#[inline]unsafe fn inb(port:u16)->u8{let value:u8;unsafe{core::arch::asm!("in al, dx",out("al")value,in("dx")port,options(nomem,nostack,preserves_flags));}value}
fn wait(mask:u8){for _ in 0..1_000_000{if unsafe{inb(LSR)}&mask==mask{return}}}
fn write_byte(byte:u8){if byte==b'\n'{wait(THR_EMPTY);unsafe{outb(COM1,b'\r')}}wait(THR_EMPTY);unsafe{outb(COM1,byte)}}
pub fn init(){unsafe{outb(COM1+1,0);outb(COM1+3,0x80);outb(COM1,3);outb(COM1+1,0);outb(COM1+3,3);outb(COM1+2,0xc7);outb(COM1+4,0x0b);}}
pub fn flush(){wait(TX_EMPTY)}
pub fn print(args:fmt::Arguments){let _=Serial.write_fmt(args);}
#[macro_export]macro_rules! print{($($arg:tt)*)=>($crate::console::print(format_args!($($arg)*)));}
#[macro_export]macro_rules! println{()=>($crate::print!("\n"));($fmt:expr)=>($crate::print!(concat!($fmt,"\n")));($fmt:expr,$($arg:tt)*)=>($crate::print!(concat!($fmt,"\n"),$($arg)*));}
