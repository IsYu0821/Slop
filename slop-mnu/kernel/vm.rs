//! Architecture-neutral virtual-memory area ledger. Hardware PTE work stays in arch.
pub const PAGE_SIZE:usize=4096; pub const MAX_AREAS:usize=64;
// Anonymous mappings live in PML4 slot 129: the 64 GiB firmware identity
// map in slot 0 and the ELF/stack slots stay untouched.
pub const MMAP_BASE:usize=0x0000_40c0_0000_0000;
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub struct ResourceStats{pub areas:usize,pub bytes:usize,pub pages:usize}
#[derive(Clone,Copy,Default,PartialEq,Eq)] pub struct VmFlags(pub u8);
impl VmFlags { pub const READ:Self=Self(1);pub const WRITE:Self=Self(2);pub const EXEC:Self=Self(4);pub const USER:Self=Self(8);pub const ANON:Self=Self(16);pub const EXTERNAL:Self=Self(32);pub const SHARED:Self=Self(64);pub fn contains(self,x:Self)->bool{self.0&x.0==x.0} }
#[derive(Clone,Copy)] pub enum VmKind { Anon, File { inode: crate::vfs::Inode, offset: u64 }, Stack, Brk }
impl Default for VmKind { fn default()->Self { VmKind::Anon } }
#[derive(Clone,Copy,Default)] pub struct Area{pub start:usize,pub end:usize,pub flags:VmFlags,pub kind:VmKind,pub cow:bool,pub locked:bool}
#[derive(Clone)] pub struct Map{areas:[Area;MAX_AREAS],len:usize,next_hint:usize}
impl Map{
 pub const fn new()->Self{Self{areas:[Area{start:0,end:0,flags:VmFlags(0),kind:VmKind::Anon,cow:false,locked:false};MAX_AREAS],len:0,next_hint:MMAP_BASE}}
  pub fn insert(&mut self,start:usize,len:usize,flags:VmFlags,kind:VmKind,cow:bool)->Result<(),i64>{
  if start%PAGE_SIZE!=0||len==0{return Err(crate::errno::EINVAL)} let end=start.checked_add(align_up(len)?).ok_or(crate::errno::ENOMEM)?;
  if start<crate::usercopy::USER_MIN||end>crate::usercopy::USER_MAX{return Err(crate::errno::ENOMEM)}
  if self.len==MAX_AREAS{return Err(crate::errno::ENOMEM)}
  if self.areas[..self.len].iter().any(|a|start<a.end&&a.start<end){return Err(crate::errno::EEXIST)}
  let mut at=self.len;for i in 0..self.len{if start<self.areas[i].start{at=i;break}}
  for i in (at..self.len).rev(){self.areas[i+1]=self.areas[i]} self.areas[at]=Area{start,end,flags,kind,cow,locked:false};self.len+=1;Ok(())
 }
  pub fn allocate(&mut self,len:usize,flags:VmFlags,kind:VmKind,cow:bool)->Result<usize,i64>{let size=align_up(len)?;let mut p=self.next_hint;loop{let candidate_end=p.checked_add(size).ok_or(crate::errno::ENOMEM)?;let mut collision=None;for a in &self.areas[..self.len]{if p<a.end&&a.start<candidate_end{collision=Some(a.end);break}}match collision{Some(n)=>{p=align_up(n)?;if p.checked_add(size).filter(|e|*e<=crate::usercopy::USER_MAX).is_none(){return Err(crate::errno::ENOMEM)}},None=>break}}self.insert(p,size,flags,kind,cow)?;self.next_hint=p+size;Ok(p)}
 pub fn remove(&mut self,start:usize,len:usize)->Result<(),i64>{
  if start%PAGE_SIZE!=0{return Err(crate::errno::EINVAL)}
  let end=start.checked_add(align_up(len)?).ok_or(crate::errno::EINVAL)?;
  let mut out=[Area::default();MAX_AREAS];
  let mut n=0usize;
  for i in 0..self.len{
   let a=self.areas[i];
   if a.end<=start||a.start>=end{out[n]=a;n+=1;continue}
   if a.start<start{out[n]=Area{end:start,..a};n+=1}
   if a.end>end{out[n]=Area{start:end,..a};n+=1}
  }
  self.areas=out;self.len=n;Ok(())
 }
  pub fn protect(&mut self,start:usize,len:usize,flags:VmFlags)->Result<(),i64>{
  if start%PAGE_SIZE!=0{return Err(crate::errno::EINVAL)}
  let end=start.checked_add(align_up(len)?).ok_or(crate::errno::EINVAL)?;
  let mut cursor=start;
  for a in &self.areas[..self.len]{
   if a.end<=cursor{continue}
   if a.start>cursor{return Err(crate::errno::ENOMEM)}
   cursor=a.end;
   if cursor>=end{break}
  }
  if cursor<end{return Err(crate::errno::ENOMEM)}
  let mut out=[Area::default();MAX_AREAS];
  let mut n=0usize;
  for i in 0..self.len{
   let a=self.areas[i];
   if a.end<=start||a.start>=end{out[n]=a;n+=1;continue}
   if n+3>MAX_AREAS{return Err(crate::errno::ENOMEM)}
   if a.start<start{out[n]=Area{end:start,..a};n+=1}
   out[n]=Area{start:core::cmp::max(a.start,start),end:core::cmp::min(a.end,end),flags,..a};n+=1;
   if a.end>end{out[n]=Area{start:end,..a};n+=1}
  }
  self.areas=out;self.len=n;Ok(())
 }
  pub fn validate(&self,ptr:usize,len:usize,write:bool)->bool{let end=match ptr.checked_add(len){Some(v)=>v,None=>return false};self.areas[..self.len].iter().any(|a|ptr>=a.start&&end<=a.end&&a.flags.contains(VmFlags::USER)&&(!write||a.flags.contains(VmFlags::WRITE)))}
  pub fn resource_stats(&self)->ResourceStats{
   let mut stats=ResourceStats{areas:self.len,bytes:0,pages:0};
   for area in &self.areas[..self.len]{let bytes=area.end-area.start;stats.bytes=stats.bytes.saturating_add(bytes);stats.pages=stats.pages.saturating_add(bytes/PAGE_SIZE);}
   stats
  }
  pub fn area(&self,index:usize)->Option<Area>{self.areas.get(index).filter(|_|index<self.len).copied()}
  pub fn flags_at(&self,address:usize)->Option<VmFlags>{self.areas[..self.len].iter().find(|a|address>=a.start&&address<a.end).map(|a|a.flags)}
  pub fn find_vma(&self,address:usize)->Option<Area>{self.areas[..self.len].iter().find(|a|address>=a.start&&address<a.end).copied()}
  pub fn set_cow(&mut self,start:usize,end:usize,cow:bool)->Result<(),i64>{let a=self.areas[..self.len].iter_mut().find(|a|a.start==start&&a.end==end).ok_or(crate::errno::EINVAL)?;a.cow=cow;Ok(())}
  pub fn set_locked_range(&mut self,addr:usize,len:usize,locked:bool)->Result<(),i64>{let end=addr.checked_add(len).ok_or(crate::errno::EINVAL)?;for a in self.areas[..self.len].iter_mut(){if a.start>=addr && a.end<=end {a.locked=locked}}Ok(())}
  /* Ranges that must stay physically shared after fork. */
  pub fn shared_writable_ranges(&self,mut cb:impl FnMut(usize,usize)){for a in &self.areas[..self.len]{if a.flags.contains(VmFlags::SHARED)&&a.flags.contains(VmFlags::WRITE){cb(a.start,a.end);}}}
  pub fn mark_all_cow(&mut self){for i in 0..self.len{if !self.areas[i].flags.contains(VmFlags::EXTERNAL){self.areas[i].cow=true;}}}
  pub fn clear(&mut self){self.areas=[Area{start:0,end:0,flags:VmFlags(0),kind:VmKind::Anon,cow:false,locked:false};MAX_AREAS];self.len=0;self.next_hint=MMAP_BASE;}
}
fn align_up(v:usize)->Result<usize,i64>{if v==0{return Err(crate::errno::EINVAL)}v.checked_add(PAGE_SIZE-1).map(|x|x&!(PAGE_SIZE-1)).ok_or(crate::errno::ENOMEM)}

pub fn resource_self_test()->bool{
 let mut map=Map::new();let flags=VmFlags(VmFlags::USER.0|VmFlags::READ.0|VmFlags::WRITE.0);
 let start=MMAP_BASE;
 if map.insert(start,PAGE_SIZE*3,flags,VmKind::Anon,false).is_err(){return false}
 let stats=map.resource_stats();
 if stats.areas!=1||stats.bytes!=PAGE_SIZE*3||stats.pages!=3{return false}
 if map.protect(start,PAGE_SIZE*3,VmFlags(VmFlags::USER.0|VmFlags::READ.0)).is_err(){return false}
 map.clear();
 map.resource_stats()==ResourceStats::default()
}
