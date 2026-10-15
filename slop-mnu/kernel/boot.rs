//! Stable hand-off ABI between a firmware loader and the kernel.
pub const BOOT_MAGIC: u64 = 0x534c4f50_4d4e5521; // "SLOPMNU!"
pub const BOOT_ABI_VERSION: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MemoryRegion { pub base: u64, pub pages: u64, pub kind: u32, pub attributes: u32 }

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Framebuffer { pub base: u64, pub size: u64, pub width: u32, pub height: u32, pub stride: u32, pub format: u32 }

#[repr(C)]
pub struct BootInfo {
    pub magic: u64, pub abi_version: u32, pub size: u32,
    pub memory_map: *const MemoryRegion, pub memory_map_len: usize,
    pub rsdp: u64, pub initrd_base: u64, pub initrd_size: u64,
    pub framebuffer: Framebuffer,
}
impl BootInfo {
    pub fn validate(&self) -> bool {
        self.magic == BOOT_MAGIC && self.abi_version == BOOT_ABI_VERSION &&
        self.size as usize >= core::mem::size_of::<BootInfo>() &&
        (self.memory_map_len == 0 || !self.memory_map.is_null())
    }
}
