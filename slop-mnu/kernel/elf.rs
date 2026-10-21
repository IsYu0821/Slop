//! Bounds-checked ELF64 parser and PT_LOAD planning. No borrowed packed structs.
pub const ELF_MAGIC:[u8;4]=*b"\x7fELF";
pub const ET_EXEC:u16=2; pub const ET_DYN:u16=3; pub const EM_X86_64:u16=62;
pub const PT_LOAD:u32=1; pub const PT_INTERP:u32=3; pub const PF_X:u32=1; pub const PF_W:u32=2; pub const PF_R:u32=4;
pub const MAX_LOAD_SEGMENTS:usize=16;

#[derive(Clone,Copy,Default)]
pub struct Header { pub kind:u16,pub machine:u16,pub entry:u64,pub phoff:u64,pub phentsize:u16,pub phnum:u16 }
#[derive(Clone,Copy,Default)]
pub struct ProgramHeader { pub kind:u32,pub flags:u32,pub offset:u64,pub vaddr:u64,pub filesz:u64,pub memsz:u64,pub align:u64 }
#[derive(Clone,Copy,Default)]
pub struct LoadSegment { pub file_offset:u64,pub vaddr:u64,pub file_size:u64,pub memory_size:u64,pub flags:u32 }
#[derive(Clone,Copy)]
pub struct LoadPlan { pub header:Header,pub segments:[LoadSegment;MAX_LOAD_SEGMENTS],pub segment_count:usize,pub has_interpreter:bool,pub interp_offset:u64,pub interp_len:u64,pub image_start:u64,pub image_end:u64 }
impl Default for LoadPlan { fn default()->Self{Self{header:Header::default(),segments:[LoadSegment::default();MAX_LOAD_SEGMENTS],segment_count:0,has_interpreter:false,interp_offset:0,interp_len:0,image_start:u64::MAX,image_end:0}} }

pub fn plan(image:&[u8])->Result<LoadPlan,&'static str>{
    if image.len()<64{return Err("ELF header truncated")}
    if image[0..4]!=ELF_MAGIC{return Err("bad ELF magic")}
    if image[4]!=2 || image[5]!=1 || image[6]!=1{return Err("not ELF64 little-endian v1")}
    let h=Header{kind:u16at(image,16)?,machine:u16at(image,18)?,entry:u64at(image,24)?,phoff:u64at(image,32)?,phentsize:u16at(image,54)?,phnum:u16at(image,56)?};
    if h.machine!=EM_X86_64{return Err("wrong machine")}
    if h.kind!=ET_EXEC && h.kind!=ET_DYN{return Err("unsupported ELF type")}
    if (h.phentsize as usize)<56{return Err("program header too small")}
    let table_size=(h.phentsize as u64).checked_mul(h.phnum as u64).ok_or("program table overflow")?;
    let table_end=h.phoff.checked_add(table_size).ok_or("program table overflow")?;
    if table_end>image.len() as u64{return Err("program table truncated")}
    let mut out=LoadPlan{header:h,..LoadPlan::default()};
    for i in 0..h.phnum as u64 {
        let off=h.phoff.checked_add(i*(h.phentsize as u64)).ok_or("program header overflow")? as usize;
        let p=ProgramHeader{kind:u32at(image,off)?,flags:u32at(image,off+4)?,offset:u64at(image,off+8)?,vaddr:u64at(image,off+16)?,filesz:u64at(image,off+32)?,memsz:u64at(image,off+40)?,align:u64at(image,off+48)?};
        if p.kind==PT_INTERP{out.has_interpreter=true;out.interp_offset=p.offset;out.interp_len=p.filesz;}
        if p.kind!=PT_LOAD{continue}
        if out.segment_count==MAX_LOAD_SEGMENTS{return Err("too many load segments")}
        if p.filesz>p.memsz{return Err("segment filesz exceeds memsz")}
        if p.offset.checked_add(p.filesz).ok_or("segment file overflow")?>image.len() as u64{return Err("segment truncated")}
        if p.vaddr.checked_add(p.memsz).ok_or("segment address overflow")?>crate::usercopy::USER_MAX as u64{return Err("segment outside user range")}
        if p.align!=0 && !p.align.is_power_of_two(){return Err("bad segment alignment")}
        if p.align>1 && (p.vaddr%p.align)!=(p.offset%p.align){return Err("incongruent segment alignment")}
        let seg=LoadSegment{file_offset:p.offset,vaddr:p.vaddr,file_size:p.filesz,memory_size:p.memsz,flags:p.flags};
        out.image_start=core::cmp::min(out.image_start,p.vaddr); out.image_end=core::cmp::max(out.image_end,p.vaddr+p.memsz);
        out.segments[out.segment_count]=seg; out.segment_count+=1;
    }
    if out.segment_count==0{return Err("ELF has no load segments")}
    if h.entry<out.image_start || h.entry>=out.image_end{return Err("entry outside image")}
    Ok(out)
}
/* Reads the NUL-terminated PT_INTERP path recorded by plan(). */
pub fn interp_path<'a>(image:&'a [u8],plan:&LoadPlan)->Result<&'a str,&'static str>{
 if plan.interp_len==0{return Err("no PT_INTERP segment")}
 let seg=image.get(plan.interp_offset as usize..(plan.interp_offset+plan.interp_len)as usize).ok_or("interp truncated")?;
 let end=seg.iter().position(|&b|b==0).ok_or("interp not NUL-terminated")?;
 core::str::from_utf8(&seg[..end]).map_err(|_|"interp not utf8")
}
fn get<const N:usize>(b:&[u8],o:usize)->Result<[u8;N],&'static str>{b.get(o..o+N).ok_or("field truncated")?.try_into().map_err(|_|"field truncated")}
fn u16at(b:&[u8],o:usize)->Result<u16,&'static str>{Ok(u16::from_le_bytes(get(b,o)?))}
fn u32at(b:&[u8],o:usize)->Result<u32,&'static str>{Ok(u32::from_le_bytes(get(b,o)?))}
fn u64at(b:&[u8],o:usize)->Result<u64,&'static str>{Ok(u64::from_le_bytes(get(b,o)?))}
