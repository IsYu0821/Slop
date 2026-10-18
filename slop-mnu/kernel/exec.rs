//! Transaction-friendly Linux ELF execution plan. It does not mutate an address space.
use crate::{elf,linux_abi::stack,vm::VmFlags};
pub const USER_STACK_TOP:u64=0x0000_7fff_ffff_f000; pub const USER_STACK_SIZE:usize=1024*1024;
#[derive(Clone,Copy,Default)] pub struct Mapping { pub start:u64,pub length:u64,pub file_offset:u64,pub file_length:u64,pub flags:VmFlags }
pub struct Plan { pub entry:u64,pub phdr:u64,pub phent:u64,pub phnum:u64,pub mappings:[Mapping;elf::MAX_LOAD_SEGMENTS],pub count:usize,pub stack_start:u64,pub stack_end:u64,pub needs_interpreter:bool,pub interp_base:u64 }
pub fn plan(image:&[u8])->Result<Plan,&'static str>{
 let e=elf::plan(image)?; let mut out=Plan{entry:e.header.entry,phdr:0,phent:e.header.phentsize as u64,phnum:e.header.phnum as u64,mappings:[Mapping::default();elf::MAX_LOAD_SEGMENTS],count:0,stack_start:USER_STACK_TOP-USER_STACK_SIZE as u64,stack_end:USER_STACK_TOP,needs_interpreter:e.has_interpreter,interp_base:0};
 for s in &e.segments[..e.segment_count]{let start=s.vaddr&!4095;let delta=s.vaddr-start;let end=align_up(s.vaddr.checked_add(s.memory_size).ok_or("segment overflow")?)?;let mut f=VmFlags::USER;if s.flags&elf::PF_R!=0{f.0|=VmFlags::READ.0}if s.flags&elf::PF_W!=0{f.0|=VmFlags::WRITE.0}if s.flags&elf::PF_X!=0{f.0|=VmFlags::EXEC.0}out.mappings[out.count]=Mapping{start,length:end-start,file_offset:s.file_offset.checked_sub(delta).ok_or("segment page prefix outside file")?,file_length:s.file_size.checked_add(delta).ok_or("segment file length overflow")?,flags:f};out.count+=1;}
 // AT_PHDR is valid when the program-header table resides in a loadable file range.
 for s in &e.segments[..e.segment_count]{if e.header.phoff>=s.file_offset && e.header.phoff<s.file_offset+s.file_size{out.phdr=s.vaddr+(e.header.phoff-s.file_offset);break}}
 if out.phdr==0{return Err("program headers are not mapped")}
 Ok(out)
}
pub fn stack_info(p:&Plan)->stack::ImageInfo{stack::ImageInfo{phdr:p.phdr,phent:p.phent,phnum:p.phnum,entry:p.entry,base:p.interp_base}}
fn align_up(v:u64)->Result<u64,&'static str>{v.checked_add(4095).map(|x|x&!4095).ok_or("alignment overflow")}
