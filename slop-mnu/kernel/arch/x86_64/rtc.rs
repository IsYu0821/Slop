//! CMOS/RTC wall-clock read used to seed CLOCK_REALTIME at boot.
use super::port::{inb,outb};

fn read(reg:u8)->u8{unsafe{outb(0x70,reg);inb(0x71)}}

fn bcd(v:u8)->u64{((v>>4)as u64)*10+(v&0xf)as u64}

fn status_ok()->bool{unsafe{outb(0x70,0x0D);inb(0x71)&0x80!=0}}

/* Reads the RTC in binary or BCD mode; returns UTC seconds since the epoch. */
pub fn epoch_seconds()->Option<u64>{
 for _ in 0..1000{unsafe{outb(0x70,0x0A);if inb(0x71)&0x80==0{break}}}
 let st=read(0x0B);
 let bcd_mode=st&0x04==0;
 let h12=st&0x02!=0;
 let conv=if bcd_mode{bcd}else{|v|v as u64};
 let mut sec=conv(read(0x00));
 let min=conv(read(0x02));
 let hraw=read(0x04);
 let mut hour=conv(hraw&0x7f);
 let day=conv(read(0x07));
 let mon=conv(read(0x08));
 let year=conv(read(0x09));
 let mut century=conv(read(0x32)&0x7f);
 if h12{
  let pm=hraw&0x80!=0;
  if pm&&hour<12{hour+=12}
  if !pm&&hour==12{hour=0}
 }
 if century==0{century=if year>=70{19}else{20}}
 if sec>=60||min>=60||hour>=24||day<1||day>31||mon<1||mon>12||year>199||century<19||century>21{return None}
 if !status_ok(){return None}
 sec=sec+min*60+hour*3600;
 let mut days=0u64;
 let y=century*100+year;
 for yy in 1970..y{
  days+=if leap(yy){366}else{365};
 }
 let mdays=[31u64,28,31,30,31,30,31,31,30,31,30,31];
 for mm in 0..(mon as usize-1){
  days+=mdays[mm];
  if mm==1&&leap(y){days+=1}
 }
 days+=day-1;
 Some(days*86400+sec)
}

fn leap(y:u64)->bool{y%4==0&&(y%100!=0||y%400==0)}
