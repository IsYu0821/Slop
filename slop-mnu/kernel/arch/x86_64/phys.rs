//! Bootstrap physical access for the first UEFI/QEMU acceptance path.
use super::paging::PhysAccess;
pub struct Identity;
impl PhysAccess for Identity {
    unsafe fn table(&self, pa:u64)->&mut[u64;512] {
        unsafe { &mut *(pa as *mut [u64;512]) }
    }
}
impl Identity {
    pub unsafe fn bytes(&self,pa:u64,len:usize)->&mut[u8]{unsafe{core::slice::from_raw_parts_mut(pa as *mut u8,len)}}
}
