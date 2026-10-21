//! virtio-pci transport.
//!
//! The transport covers both device interfaces a Linux guest meets: the
//! legacy interface (version 0.9.5, port I/O) and the modern interface
//! (virtio 1.0, memory mapped). The modern interface is what a plain
//! `virtio-blk-pci` or `virtio-net-pci` device offers, so a driver that only
//! speaks the legacy interface cannot use a default device model.
//!
//! The modern interface describes itself through vendor specific PCI
//! capabilities: the common configuration, the notification area and the
//! device specific configuration. All of them live in the first memory
//! mapped bar.
use crate::arch::x86_64::phys::phys_to_virt;
use crate::arch::x86_64::port;
use crate::driver::pci::{self,Bdf,Cam,ConfigAccess};

pub const VIRTIO_VENDOR:u16=0x1af4;
pub const LEGACY_NET:u16=0x1000;
pub const LEGACY_BLK:u16=0x1001;
pub const MODERN_NET:u16=0x1040;
pub const MODERN_BLK:u16=0x1041;

/* The legacy port I/O register set. */
const LEGACY_HOST_FEATURES:u16=0x00;
const LEGACY_GUEST_FEATURES:u16=0x04;
const LEGACY_QUEUE_ADDR:u16=0x08;
const LEGACY_QUEUE_SIZE:u16=0x0c;
const LEGACY_QUEUE_SELECT:u16=0x0e;
const LEGACY_QUEUE_NOTIFY:u16=0x10;
const LEGACY_STATUS:u16=0x12;
const LEGACY_CONFIG:u16=0x14;

/* The device status bits. */
pub const S_ACK:u8=1;
pub const S_DRIVER:u8=2;
pub const S_DRIVER_OK:u8=4;
pub const S_FEATURES_OK:u8=8;

/* The common configuration register offsets. */
const CFG_DEVICE_FEATURE_SELECT:u64=0x00;
const CFG_DEVICE_FEATURE:u64=0x04;
const CFG_DRIVER_FEATURE_SELECT:u64=0x08;
const CFG_DRIVER_FEATURE:u64=0x0c;
const CFG_NUM_QUEUES:u64=0x12;
const CFG_STATUS:u64=0x14;
const CFG_QUEUE_SELECT:u64=0x16;
const CFG_QUEUE_SIZE:u64=0x18;
const CFG_QUEUE_ENABLE:u64=0x1c;
const CFG_QUEUE_NOTIFY_OFF:u64=0x1e;
const CFG_QUEUE_DESC:u64=0x20;
const CFG_QUEUE_DRIVER:u64=0x24;
const CFG_QUEUE_DEVICE:u64=0x28;

/* The capability identifiers and the layout of the virtio capability. */
const PCI_CAP_ID_VENDOR:u8=0x09;
const CAP_COMMON_CFG:u8=1;
const CAP_NOTIFY_CFG:u8=2;
const CAP_DEVICE_CFG:u8=3;

/* The feature bit that selects the modern interface. */
pub const VIRTIO_F_VERSION_1:u32=32;

/// One split virtqueue: the descriptor table, the available ring and the
/// used ring. The legacy interface keeps the three parts in one buffer, the
/// modern interface names three separate addresses.
#[derive(Clone,Copy)]
pub struct Queue{pub desc:*mut u8,pub avail:*mut u8,pub used:*mut u8,pub size:usize}

/// The device transport.
#[derive(Clone,Copy)]
pub struct Transport{
 pub modern:bool,
 /// The legacy port I/O base.
 pub io:u16,
 /// The memory mapped base of the first bar.
 pub mmio:u64,
 /// The offset of the common configuration inside the bar.
 pub common:u64,
 /// The offset of the device configuration inside the bar.
 pub device_cfg:u64,
 /// The notification multiplier, the queue offset and the bar that carries
 /// the notification area.
 pub notify_mult:u32,
 pub notify_off:u32,
 pub notify_bar:u32,
 /// The number of queues the device offers.
 pub queues:u16,
 /// The physical address of the notification area.
 pub notify_base:u64,
}

/// Reads a capability sized byte range from the configuration space.
fn cfg_read(bdf:Bdf,offset:u16)->Option<[u8;16]>{
 let acc=Cam;
 let mut out=[0u8;16];
 let mut at=offset as usize;
 while at<offset as usize+16{
  let word=acc.read32(bdf,(at as u16)&!3);
  let shift=(at&3)*8;
  let bytes=word.to_le_bytes();
  let take=core::cmp::min(4-(at&3),16-(at-offset as usize));
  out[at-offset as usize..at-offset as usize+take].copy_from_slice(&bytes[shift..shift+take]);
  at+=take;
 }
 Some(out)
}

/// Finds the virtio capabilities of a device and returns the offsets of the
/// common configuration, the notification area and the device configuration.
fn modern_caps(bdf:Bdf)->Option<(u64,u64,u32,u32,u32)>{
 let acc=Cam;
 let mut ptr=(acc.read32(bdf,pci::CFG_CAP)&0xff)as u16;
 let mut common=0u64;let mut device=0u64;let mut mult=0u32;let mut noff=0u32;let mut nbar=0u32;
 let mut found=0u32;
 while ptr>=0x40&&ptr<0xfc{
  let cap=cfg_read(bdf,ptr)?;
  let id=cap[0];let next=cap[1];let len=cap[2]as usize;
  if id==PCI_CAP_ID_VENDOR&&len>=16{
   let cfg_type=cap[3];let bar=cap[4]as u32;
   let offset=u32::from_le_bytes([cap[8],cap[9],cap[10],cap[11]])as u64;
   match cfg_type{
    CAP_COMMON_CFG=>{common=offset;found|=1;}
    CAP_DEVICE_CFG=>{device=offset;found|=2;}
    CAP_NOTIFY_CFG=>{mult=u32::from_le_bytes([cap[8],cap[9],cap[10],cap[11]]);noff=u32::from_le_bytes([cap[12],cap[13],cap[14],cap[15]]);nbar=bar;found|=4;}
    _=>{}
   }
  }
  if next==0||u16::from(next)==ptr{break}
  ptr=u16::from(next);
 }
 if found&7==7{Some((common,device,mult,noff,nbar))}else{None}
}

/// Finds a virtio device of the given identifier and prepares the transport.
pub fn probe(device:u16)->Option<Transport>{
 let found=pci::find(VIRTIO_VENDOR,device)?;
 found.enable(&Cam);
 let bar0=found.bars[0];
 if device>=MODERN_NET{
  if bar0.io{return None}
  let (common,dev_cfg,mult,noff,nbar)=modern_caps(found.bdf)?;
  let mmio=bar0.base;
  let notify_base=if nbar==0{mmio}else{found.bars[nbar as usize].base};
  let mut t=Transport{modern:true,io:0,mmio,common,device_cfg:dev_cfg,notify_mult:mult,notify_off:noff,notify_bar:nbar,queues:0,notify_base:notify_base};
  t.queues=unsafe{core::ptr::read_volatile(t.common_ptr(CFG_NUM_QUEUES)as*const u16)};
  Some(t)
 }else{
  if !bar0.io{return None}
  Some(Transport{modern:false,io:bar0.base as u16,mmio:0,common:0,device_cfg:0,notify_mult:0,notify_off:0,notify_bar:0,queues:0,notify_base:0})
 }
}

impl Transport{
 #[inline]fn common_ptr(&self,off:u64)->*mut u8{unsafe{(phys_to_virt(self.mmio+self.common+off)as*mut u8)}}
 #[inline]fn device_ptr(&self,off:u64)->*mut u8{unsafe{(phys_to_virt(self.mmio+self.device_cfg+off)as*mut u8)}}
 /// Reads the device status.
 pub fn status(&self)->u8{if self.modern{unsafe{core::ptr::read_volatile(self.common_ptr(CFG_STATUS))}}else{unsafe{port::inb(self.io+LEGACY_STATUS)}}}
 /// Writes the device status.
 pub fn set_status(&self,value:u8){if self.modern{unsafe{core::ptr::write_volatile(self.common_ptr(CFG_STATUS),value)}}else{unsafe{port::outb(self.io+LEGACY_STATUS,value)}}}
 /// Reads one 32-bit feature word.
 pub fn read_features(&self,select:u32)->u32{
  if !self.modern{return unsafe{port::inl(self.io+LEGACY_HOST_FEATURES)};}
  unsafe{core::ptr::write_volatile(self.common_ptr(CFG_DEVICE_FEATURE_SELECT)as*mut u16,select as u16);
   core::ptr::read_volatile(self.common_ptr(CFG_DEVICE_FEATURE)as*const u32)}
 }
 /// Writes one 32-bit feature word.
 pub fn set_features(&self,select:u32,value:u32){
  if !self.modern{unsafe{port::outl(self.io+LEGACY_GUEST_FEATURES,value);}return;}
  unsafe{core::ptr::write_volatile(self.common_ptr(CFG_DRIVER_FEATURE_SELECT)as*mut u16,select as u16);
   core::ptr::write_volatile(self.common_ptr(CFG_DRIVER_FEATURE)as*mut u32,value);}
 }
 /// Negotiates the modern interface: the driver asks for the version 1
 /// feature and the device confirms the result.
 pub fn negotiate_modern(&self)->bool{
  let mut features=self.read_features(0);
  let mut high=self.read_features(1);
  if high&(1<<(VIRTIO_F_VERSION_1-32))==0{return false}
  features|=1;high|=1<<(VIRTIO_F_VERSION_1-32);
  self.set_features(0,features);
  self.set_features(1,high);
  self.set_status(self.status()|S_FEATURES_OK);
  (self.status()&S_FEATURES_OK)!=0
 }
 /// Selects a queue and returns its size.
 pub fn queue_select(&self,index:u16)->usize{
  if !self.modern{unsafe{port::outw(self.io+LEGACY_QUEUE_SELECT,index);return port::inw(self.io+LEGACY_QUEUE_SIZE)as usize}}
  unsafe{core::ptr::write_volatile(self.common_ptr(CFG_QUEUE_SELECT)as*mut u16,index);core::ptr::read_volatile(self.common_ptr(CFG_QUEUE_SIZE)as*const u16)as usize}
 }
 /// Enables a queue.
 pub fn queue_enable(&self,index:u16){if !self.modern{return}unsafe{core::ptr::write_volatile(self.common_ptr(CFG_QUEUE_SELECT)as*mut u16,index);core::ptr::write_volatile(self.common_ptr(CFG_QUEUE_ENABLE)as*mut u16,1u16);}}
 /// Installs the three ring parts of a queue.
 pub fn queue_setup(&self,index:u16,q:&Queue){
  if !self.modern{
   unsafe{port::outw(self.io+LEGACY_QUEUE_SELECT,index);
    port::outl(self.io+LEGACY_QUEUE_ADDR,(crate::arch::x86_64::phys::virt_to_phys(q.desc as u64)>>12)as u32);}
   return;
  }
  unsafe{
   core::ptr::write_volatile(self.common_ptr(CFG_QUEUE_SELECT)as*mut u16,index);
   /* The driver chooses the queue size of a modern device. */
   core::ptr::write_volatile(self.common_ptr(CFG_QUEUE_SIZE)as*mut u16,q.size as u16);
   core::ptr::write_volatile(self.common_ptr(CFG_QUEUE_DESC)as*mut u64,crate::arch::x86_64::phys::virt_to_phys(q.desc as u64));
   core::ptr::write_volatile(self.common_ptr(CFG_QUEUE_DRIVER)as*mut u64,crate::arch::x86_64::phys::virt_to_phys(q.avail as u64));
   core::ptr::write_volatile(self.common_ptr(CFG_QUEUE_DEVICE)as*mut u64,crate::arch::x86_64::phys::virt_to_phys(q.used as u64));
   core::ptr::write_volatile(self.common_ptr(CFG_QUEUE_ENABLE)as*mut u16,1u16);
  }
 }
 /// Tells the device that a queue has work.
 pub fn notify(&self,index:u16){
  if !self.modern{unsafe{port::outw(self.io+LEGACY_QUEUE_NOTIFY,index);}return}
  /* Every queue has its own notification register, two bytes apart. */
  let addr=self.notify_base+(self.notify_off as u64)*(self.notify_mult as u64)+(index as u64)*2;
  unsafe{core::ptr::write_volatile(phys_to_virt(addr)as*mut u16,index);}
 }
 /// Reads a device specific configuration value.
 pub fn device_config(&self,offset:u64)->u64{
  if !self.modern{return unsafe{port::inl(self.io+LEGACY_CONFIG as u16+(offset as u16))}as u64}
  unsafe{core::ptr::read_volatile(self.device_ptr(offset))as u64}
 }
}
