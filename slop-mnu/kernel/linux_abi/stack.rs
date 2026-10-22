//! System V AMD64/Linux initial userspace stack image builder.
use super::types::*;
pub const MAX_ARGS:usize=32; pub const MAX_ENVS:usize=64; pub const RANDOM_BYTES:usize=16;
#[derive(Clone,Copy)] pub struct ImageInfo { pub phdr:u64,pub phent:u64,pub phnum:u64,pub entry:u64,pub base:u64,pub vdso:u64 }
pub struct BuiltStack { pub rsp:u64,pub argc:usize,pub bytes_used:usize,pub random_addr:u64,pub execfn_addr:u64 }

/// Builds a complete argc/argv/envp/auxv stack in `page`, whose first byte is at `base`.
/// Strings are copied into the top; scalar vectors are written below and RSP is 16-byte aligned.
pub fn build(page:&mut[u8],base:u64,argv:&[&str],envp:&[&str],execfn:&str,random:[u8;RANDOM_BYTES],info:ImageInfo)->Result<BuiltStack,i64>{
 if argv.len()>MAX_ARGS||envp.len()>MAX_ENVS{return Err(crate::errno::E2BIG)}
 let top=base.checked_add(page.len() as u64).ok_or(crate::errno::EFAULT)?; let mut cursor=page.len();
 let random_addr=push_bytes(page,base,&mut cursor,&random)?;
 let execfn_addr=push_cstr(page,base,&mut cursor,execfn)?;
 let mut enva=[0u64;MAX_ENVS]; for (i,s) in envp.iter().enumerate().rev(){enva[i]=push_cstr(page,base,&mut cursor,s)?;}
 let mut arga=[0u64;MAX_ARGS]; for (i,s) in argv.iter().enumerate().rev(){arga[i]=push_cstr(page,base,&mut cursor,s)?;}
 cursor&=!7usize;
 let aux=[Auxv{kind:AT_PAGESZ,value:4096},Auxv{kind:AT_PHDR,value:info.phdr},Auxv{kind:AT_PHENT,value:info.phent},Auxv{kind:AT_PHNUM,value:info.phnum},Auxv{kind:AT_ENTRY,value:info.entry},Auxv{kind:AT_BASE,value:info.base},Auxv{kind:AT_UID,value:0},Auxv{kind:AT_EUID,value:0},Auxv{kind:AT_GID,value:0},Auxv{kind:AT_EGID,value:0},Auxv{kind:AT_SECURE,value:0},Auxv{kind:AT_RANDOM,value:random_addr},Auxv{kind:AT_EXECFN,value:execfn_addr},Auxv{kind:AT_SYSINFO_EHDR,value:info.vdso},Auxv{kind:AT_NULL,value:0}];
 let scalar_words=aux.len()*2 + 1+envp.len() + 1+argv.len() + 1;
 if (base + cursor as u64 - (scalar_words as u64)*8)&15!=0 { if cursor<8{return Err(crate::errno::E2BIG)} cursor-=8; }
 for a in aux.iter().rev(){push_u64(page,&mut cursor,a.value)?;push_u64(page,&mut cursor,a.kind)?;}
 push_u64(page,&mut cursor,0)?; for &p in enva[..envp.len()].iter().rev(){push_u64(page,&mut cursor,p)?;}
 push_u64(page,&mut cursor,0)?; for &p in arga[..argv.len()].iter().rev(){push_u64(page,&mut cursor,p)?;}
 push_u64(page,&mut cursor,argv.len() as u64)?;
 // AMD64 process entry requires a 16-byte aligned stack pointer.
 let rsp=base+cursor as u64; if rsp>=top{return Err(crate::errno::E2BIG)}
 Ok(BuiltStack{rsp,argc:argv.len(),bytes_used:page.len()-cursor,random_addr,execfn_addr})
}
fn push_bytes(page:&mut[u8],base:u64,cursor:&mut usize,data:&[u8])->Result<u64,i64>{if data.len()>*cursor{return Err(crate::errno::E2BIG)}*cursor-=data.len();page[*cursor..*cursor+data.len()].copy_from_slice(data);Ok(base+*cursor as u64)}
fn push_cstr(page:&mut[u8],base:u64,cursor:&mut usize,s:&str)->Result<u64,i64>{if s.as_bytes().contains(&0){return Err(crate::errno::EINVAL)}if *cursor==0{return Err(crate::errno::E2BIG)}*cursor-=1;page[*cursor]=0;push_bytes(page,base,cursor,s.as_bytes())}
fn push_u64(page:&mut[u8],cursor:&mut usize,v:u64)->Result<(),i64>{if *cursor<8{return Err(crate::errno::E2BIG)}*cursor-=8;page[*cursor..*cursor+8].copy_from_slice(&v.to_ne_bytes());Ok(())}
