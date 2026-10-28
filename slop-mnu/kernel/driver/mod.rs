pub mod pci;
pub mod virtio;
pub mod virtio_net;
pub mod virtio_blk;
pub trait Driver:Sync { fn name(&self)->&'static str; fn probe(&self,dev:&Device)->bool; fn attach(&self,dev:&Device)->Result<(),()>; }
pub enum Bus{Pci,Platform,Virtio} pub struct Device{pub bus:Bus,pub vendor:u32,pub device:u32,pub class:u32}
pub fn init(){
 let devices=pci::count();
 crate::println!("driver: PCI/platform/virtio contracts ready devices={}",devices);
 match virtio_net::init(){
  Ok(mac)=>crate::println!("net: virtio-net up mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",mac[0],mac[1],mac[2],mac[3],mac[4],mac[5]),
  Err(e)=>crate::println!("net: virtio-net unavailable: {}",e),
 }
 match virtio_blk::init(){
  Ok(cap)=>crate::println!("blk: virtio-blk up capacity={} sectors",cap),
  Err(e)=>crate::println!("blk: virtio-blk unavailable: {}",e),
 }
 // Slop IRQ core self-test: register → IPI → dispatch → handler → unregister.
 crate::irq::selftest_irq_dispatch();
 // Slop PCI core self-test: synthetic Linux driver to validate the
 // Linux-to-Slop adapter path (register/match/probe) without a module.
 crate::linux_compat::selftest_pci_adapter();
}
