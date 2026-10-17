//! Architecture-neutral virtual-memory area ledger. Hardware PTE work stays in arch.
pub const PAGE_SIZE:usize=4096; pub const MAX_AREAS:usize=64;
// Anonymous mappings live in PML4 slot 129: the 64 GiB firmware identity
// map in slot 0 and the ELF/stack slots stay untouched.
pub const MMAP_BASE:usize=0x0000_40c0_0000_0000;
#[derive(Clone,Copy,Default,PartialEq,Eq)] pub struct VmFlags(pub u8);
impl VmFlags { pub const READ:Self=Self(1);pub const WRITE:Self=Self(2);pub const EXEC:Self=Self(4);pub const USER:Self=Self(8);pub const ANON:Self=Self(16);pub fn contains(self,x:Self)->bool{self.0&x.0==x.0} }
#[derive(Clone,Copy,Default)] pub struct Area{pub start:usize,pub end:usize,pub flags:VmFlags}
#[derive(Clone)] pub struct Map{areas:[Area;MAX_AREAS],len:usize,next_hint:usize}
impl Map{
 pub const fn new()->Self{Self{areas:[Area{start:0,end:0,flags:VmFlags(0)};MAX_AREAS],len:0,next_hint:MMAP_BASE}}
 pub fn insert(&mut self,start:usize,len:usize,flags:VmFlags)->Result<(),i64>{
  if start%PAGE_SIZE!=0||len==0{return Err(crate::errno::EINVAL)} let end=start.checked_add(align_up(len)?).ok_or(crate::errno::ENOMEM)?;
  if start<crate::usercopy::USER_MIN||end>crate::usercopy::USER_MAX{return Err(crate::errno::ENOMEM)}
  if self.len==MAX_AREAS{return Err(crate::errno::ENOMEM)}
  if self.areas[..self.len].iter().any(|a|start<a.end&&a.start<end){return Err(crate::errno::EEXIST)}
  let mut at=self.len;for i in 0..self.len{if start<self.areas[i].start{at=i;break}}
  for i in (at..self.len).rev(){self.areas[i+1]=self.areas[i]} self.areas[at]=Area{start,end,flags};self.len+=1;Ok(())
 }
 pub fn allocate(&mut self,len:usize,flags:VmFlags)->Result<usize,i64>{let size=align_up(len)?;let mut p=self.next_hint;loop{let candidate_end=p.checked_add(size).ok_or(crate::errno::ENOMEM)?;let mut collision=None;for a in &self.areas[..self.len]{if p<a.end&&a.start<candidate_end{collision=Some(a.end);break}}match collision{Some(n)=>{p=align_up(n)?;if p.checked_add(size).filter(|e|*e<=crate::usercopy::USER_MAX).is_none(){return Err(crate::errno::ENOMEM)}},None=>break}}self.insert(p,size,flags)?;self.next_hint=p+size;Ok(p)}
 pub fn remove(&mut self,start:usize,len:usize)->Result<(),i64>{let end=start.checked_add(align_up(len)?).ok_or(crate::errno::EINVAL)?;let i=self.areas[..self.len].iter().position(|a|a.start==start&&a.end==end).ok_or(crate::errno::EINVAL)?;for j in i+1..self.len{self.areas[j-1]=self.areas[j]}self.len-=1;Ok(())}
 pub fn protect(&mut self,start:usize,len:usize,flags:VmFlags)->Result<(),i64>{let end=start.checked_add(align_up(len)?).ok_or(crate::errno::EINVAL)?;let a=self.areas[..self.len].iter_mut().find(|a|a.start==start&&a.end==end).ok_or(crate::errno::EINVAL)?;a.flags=flags;Ok(())}
 pub fn validate(&self,ptr:usize,len:usize,write:bool)->bool{let end=match ptr.checked_add(len){Some(v)=>v,None=>return false};self.areas[..self.len].iter().any(|a|ptr>=a.start&&end<=a.end&&a.flags.contains(VmFlags::USER)&&(!write||a.flags.contains(VmFlags::WRITE)))}
}
fn align_up(v:usize)->Result<usize,i64>{if v==0{return Err(crate::errno::EINVAL)}v.checked_add(PAGE_SIZE-1).map(|x|x&!(PAGE_SIZE-1)).ok_or(crate::errno::ENOMEM)}
