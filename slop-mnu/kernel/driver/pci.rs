#[derive(Clone,Copy)] pub struct Bdf{pub segment:u16,pub bus:u8,pub device:u8,pub function:u8}
pub trait ConfigAccess:Sync{fn read32(&self,bdf:Bdf,offset:u16)->u32;fn write32(&self,bdf:Bdf,offset:u16,value:u32);}
pub struct Bar{pub base:u64,pub size:u64,pub io:bool,pub prefetchable:bool}
// Linux driver compatibility belongs in generated adapters above this native API, never in core structs.
