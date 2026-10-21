//! Stable hand-off ABI between a firmware loader and the kernel.
pub const BOOT_MAGIC: u64 = 0x534c4f50_4d4e5521; // "SLOPMNU!"
pub const BOOT_ABI_VERSION: u32 = 2;

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
    pub image_base: u64, pub image_size: u64,
}
impl BootInfo {
    pub fn validate(&self) -> bool {
        self.magic == BOOT_MAGIC && self.abi_version == BOOT_ABI_VERSION &&
        self.size as usize >= core::mem::size_of::<BootInfo>() &&
        (self.memory_map_len == 0 || !self.memory_map.is_null())
    }
}

/* Applies PE32+ base relocations (`delta` = new base - original base) to a
   loaded copy of the kernel image so it can execute at its new base. Only
   DIR64 entries exist in images built by the LLVM toolchain. */
pub unsafe fn apply_relocations(image:u64,delta:i64)->Result<usize,&'static str>{
    let base=image as *const u8;
    let rd16=|off:u64|unsafe{core::ptr::read_unaligned(base.add(off as usize)as*const u16)};
    let rd32=|off:u64|unsafe{core::ptr::read_unaligned(base.add(off as usize)as*const u32)};
    if rd16(0)!=0x5a4d{return Err("image is not a PE")}
    let pe=rd32(0x3c)as u64;
    if rd32(pe)!=0x0000_4550{return Err("bad PE signature")}
    let opt=pe+24;
    if rd16(opt)!=0x20b{return Err("image is not PE32+")}
    let dirs=rd32(opt+108)as u64;
    if dirs<6{return Err("no base relocation directory")}
    let reloc_rva=rd32(opt+112+5*8)as u64;
    let reloc_size=rd32(opt+112+5*8+4)as u64;
    if reloc_rva==0||reloc_size<8{return Err("base relocations stripped")}
    let mut p=reloc_rva;
    let end=reloc_rva+reloc_size;
    let mut applied=0usize;
    while p+8<=end{
        let page=rd32(p)as u64;
        let block=rd32(p+4)as u64;
        if block<8||p+block>end{return Err("corrupt relocation block")}
        let mut q=p+8;
        while q+2<=p+block{
            let e=rd16(q);
            let typ=e>>12;
            let off=(e&0xfff)as u64;
            if typ==10{
                let at=base.add((page+off)as usize)as*mut u64;
                unsafe{let v=core::ptr::read_unaligned(at);core::ptr::write_unaligned(at,(v as i64+delta)as u64)};
                applied+=1;
            }else if typ!=0{return Err("unsupported relocation type")}
            q+=2;
        }
        p+=block;
    }
    Ok(applied)
}
