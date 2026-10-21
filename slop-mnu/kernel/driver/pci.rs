//! PCI configuration access and device discovery.
//!
//! The CAM pair at 0xcf8/0xcfc is used for configuration space; QEMU's q35
//! host bridge answers it for every bus. Discovery walks bus 0..=255, checking
//! function 0 first and only scanning further functions when the multifunction
//! bit in the header type is set.
use crate::arch::x86_64::port;

#[derive(Clone,Copy)] pub struct Bdf{pub segment:u16,pub bus:u8,pub device:u8,pub function:u8}
pub trait ConfigAccess:Sync{fn read32(&self,bdf:Bdf,offset:u16)->u32;fn write32(&self,bdf:Bdf,offset:u16,value:u32);}
#[derive(Clone,Copy)] pub struct Bar{pub base:u64,pub size:u64,pub io:bool,pub prefetchable:bool}
// Linux driver compatibility belongs in generated adapters above this native API, never in core structs.

pub const CFG_VENDOR:u16=0x00;pub const CFG_COMMAND:u16=0x04;pub const CFG_CLASS:u16=0x08;
pub const CFG_HEADER:u16=0x0c;pub const CFG_BAR0:u16=0x10;
pub const CMD_IO:u16=1;pub const CMD_MEM:u16=2;pub const CMD_BUS_MASTER:u16=4;

/* Configuration space via the CAM I/O ports (works on QEMU's q35 bridge). */
pub struct Cam;
fn cam_address(bdf:Bdf,offset:u16)->u32{(1<<31)|((bdf.bus as u32)<<16)|((bdf.device as u32)<<11)|((bdf.function as u32)<<8)|((offset as u32)&0xfc)}
impl ConfigAccess for Cam{
 fn read32(&self,bdf:Bdf,offset:u16)->u32{unsafe{port::outl(0xcf8,cam_address(bdf,offset));port::inl(0xcfc)}}
 fn write32(&self,bdf:Bdf,offset:u16,value:u32){unsafe{port::outl(0xcf8,cam_address(bdf,offset));port::outl(0xcfc,value)}}
}

pub struct Found{pub bdf:Bdf,pub vendor:u16,pub device:u16,pub class:u8,pub bars:[Bar;6]}
impl Found{
 pub fn enable(&self,acc:&dyn ConfigAccess){
  let base=acc.read32(self.bdf,CFG_COMMAND)as u16;
  acc.write32(self.bdf,CFG_COMMAND,(base|CMD_IO|CMD_MEM|CMD_BUS_MASTER)as u32);
 }
}
fn read_bar(acc:&dyn ConfigAccess,bdf:Bdf,index:u8)->Bar{
 let lo=acc.read32(bdf,CFG_BAR0+4*(index as u16));
 if lo&1!=0{return Bar{base:(lo&0xffff_fffc)as u64,size:0,io:true,prefetchable:false}}
 let kind=(lo>>1)&3;
 if kind==2{
  let hi=acc.read32(bdf,CFG_BAR0+4*(index as u16)+4);
  Bar{base:((hi as u64)<<32)|((lo&0xffff_fff0)as u64),size:0,io:false,prefetchable:lo&8!=0}
 }else{
  Bar{base:(lo&0xffff_fff0)as u64,size:0,io:false,prefetchable:lo&8!=0}
 }
}
fn read_bars(acc:&dyn ConfigAccess,bdf:Bdf)->[Bar;6]{
 let mut bars=[Bar{base:0,size:0,io:false,prefetchable:false};6];
 let mut i=0usize;
 while i<6{
  let bar=read_bar(acc,bdf,i as u8);
  let wide=!bar.io&&(acc.read32(bdf,CFG_BAR0+4*(i as u16))>>1)&3==2;
  bars[i]=bar;
  i+=if wide{2}else{1};
 }
 bars
}
fn probe(acc:&dyn ConfigAccess,bdf:Bdf)->Option<Found>{
 let word=acc.read32(bdf,CFG_VENDOR);
 if word&0xffff==0xffff{return None}
 let vendor=(word&0xffff)as u16;let device=(word>>16)as u16;
 let class=((acc.read32(bdf,CFG_CLASS)>>24)&0xff)as u8;
 Some(Found{bdf,vendor,device,class,bars:read_bars(acc,bdf)})
}
pub fn find(vendor:u16,device:u16)->Option<Found>{
 let acc=Cam;
 for bus in 0..=255u8{
  for dev in 0..32u8{
   let bdf=Bdf{segment:0,bus,device:dev,function:0};
   let word=acc.read32(bdf,CFG_VENDOR);
   if word&0xffff==0xffff{continue}
   let multifunction=(acc.read32(bdf,CFG_HEADER)>>16)&0x80!=0;
   let last=if multifunction{8u8}else{1u8};
   for func in 0..last{
    let bdf=Bdf{segment:0,bus,device:dev,function:func};
    if let Some(found)=probe(&acc,bdf){
     if found.vendor==vendor&&found.device==device{return Some(found)}
    }
   }
  }
 }
 None
}
/* Human-readable count for the boot log. */
pub fn count()->usize{
 let acc=Cam;let mut n=0;
 for bus in 0..=255u8{
  for dev in 0..32u8{
   let bdf=Bdf{segment:0,bus,device:dev,function:0};
   if acc.read32(bdf,CFG_VENDOR)&0xffff!=0xffff{n+=1}
  }
 }
 n
}
