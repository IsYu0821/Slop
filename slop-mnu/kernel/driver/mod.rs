pub mod pci;
pub trait Driver:Sync { fn name(&self)->&'static str; fn probe(&self,dev:&Device)->bool; fn attach(&self,dev:&Device)->Result<(),()>; }
pub enum Bus{Pci,Platform,Virtio} pub struct Device{pub bus:Bus,pub vendor:u32,pub device:u32,pub class:u32}
pub fn init(){crate::println!("driver: PCI/platform/virtio contracts ready");}
