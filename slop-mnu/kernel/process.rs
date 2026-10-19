//! Per-process state: private address space, VM ledger, brk and descriptors.
use crate::{arch::x86_64::{paging::{PhysAccess,PageTableRoot,PRESENT,WRITABLE,USER,NX,ADDR},phys::Identity},fd::FdTable,vm::{Map,VmFlags,VmKind}};
const PAGE:u64=4096;
const MAX_SECCOMP_FILTERS:usize=64;
#[derive(Clone,Copy)]struct SeccompFilterRef{frame:u64,refs:u32}
const EMPTY_SECCOMP_FILTER:SeccompFilterRef=SeccompFilterRef{frame:0,refs:0};
static SECCOMP_FILTER_REFS:crate::sync::SpinLock<[SeccompFilterRef;MAX_SECCOMP_FILTERS]>=crate::sync::SpinLock::new([EMPTY_SECCOMP_FILTER;MAX_SECCOMP_FILTERS]);
/* Slot 129 keeps the heap and anonymous mappings out of the firmware identity map. */
pub const BRK_BASE:u64=0x0000_4080_0000_0000;
pub fn slot_of(va:u64)->usize{((va>>39)&511)as usize}
pub fn seccomp_filter_register(frame:u64)->Result<(),i64>{
 let mut refs=SECCOMP_FILTER_REFS.lock();let slot=refs.iter().position(|entry|entry.frame==0).ok_or(crate::errno::ENOMEM)?;refs[slot]=SeccompFilterRef{frame,refs:1};Ok(())
}
pub fn seccomp_filter_acquire(frame:u64)->Result<(),i64>{
 let mut refs=SECCOMP_FILTER_REFS.lock();let entry=refs.iter_mut().find(|entry|entry.frame==frame&&frame!=0).ok_or(crate::errno::EINVAL)?;entry.refs=entry.refs.checked_add(1).ok_or(crate::errno::EOVERFLOW)?;Ok(())
}
pub fn seccomp_filter_release(frame:u64)->Result<(),i64>{
 let release={let mut refs=SECCOMP_FILTER_REFS.lock();let entry=refs.iter_mut().find(|entry|entry.frame==frame&&frame!=0).ok_or(crate::errno::EINVAL)?;if entry.refs>1{entry.refs-=1;false}else{*entry=EMPTY_SECCOMP_FILTER;true}};
 if release{crate::memory::free_frame(frame)?}Ok(())
}

pub struct Process{
 pub cr3:u64,pub bootstrap_cr3:u64,
 pub image_slot:usize,pub stack_slot:usize,pub mmap_slot:usize,
 pub vm:Map,pub brk:u64,pub clear_child_tid:u64,pub fds:FdTable,pub cwd:[u8;128],pub cwd_len:usize,pub root:[u8;128],pub root_len:usize,
 pub pending:u64,pub sigmask:u64,pub si_code:[i32;64],pub si_value:[u64;64],pub si_overrun:[u32;64],pub robust_list_head:u64,pub robust_list_len:u64,pub handlers:[crate::signal::Action;64],pub sigframe:u64,pub exe_path:[u8;64],pub exe_len:usize,pub name:[u8;16],pub hostname:[u8;65],
 pub cmdline:[u8;512],pub cmdline_len:usize,pub environ:[u8;512],pub environ_len:usize,
 pub segs:[crate::exec_runtime::MapSeg;crate::exec_runtime::MAX_MAP_SEGS],pub nsegs:usize,pub stack_base:u64,pub stack_top:u64,
 pub seccomp_filter:usize,pub seccomp_filter_len:u16,
 pub uid:u32,pub gid:u32,pub euid:u32,pub egid:u32,pub suid:u32,pub sgid:u32,pub fsuid:u32,pub fsgid:u32,
 pub cap_inh:u64,pub cap_prm:u64,pub cap_eff:u64,pub cap_bset:u64,
 pub ns_uts:u64,pub ns_ipc:u64,pub ns_mnt:u64,pub ns_pid:u64,pub ns_net:u64,pub ns_user:u64,pub ns_cgroup:u64,pub ns_time:u64,
  pub seccomp_mode:u32,pub no_new_privs:bool,pub umask:u32,
 pub groups:[u32;32],pub ngroups:usize,
}
impl Process{
 pub fn new(cr3:u64,bootstrap_cr3:u64,image_slot:usize,stack_slot:usize)->Result<Self,i64>{
   Ok(Self{cr3,bootstrap_cr3,image_slot,stack_slot,mmap_slot:slot_of(crate::vm::MMAP_BASE as u64),vm:Map::new(),brk:BRK_BASE,clear_child_tid:0,fds:FdTable::stdio()?,pending:0,sigmask:0,si_code:[0;64],si_value:[0;64],si_overrun:[0;64],robust_list_head:0,robust_list_len:0,handlers:crate::signal::handlers_dfl(),sigframe:0,exe_path:[0;64],exe_len:0,cwd:{let mut a=[0u8;128];a[0]=b'/';a},cwd_len:1,root:{let mut a=[0u8;128];a[0]=b'/';a},root_len:1,name:[0;16],hostname:{let mut a=[0u8;65];a[..4].copy_from_slice(b"slop");a},cmdline:[0;512],cmdline_len:0,environ:[0;512],environ_len:0,segs:[crate::exec_runtime::MapSeg::default();crate::exec_runtime::MAX_MAP_SEGS],nsegs:0,stack_base:0,stack_top:0,seccomp_filter:0,seccomp_filter_len:0,uid:0,gid:0,euid:0,egid:0,suid:0,sgid:0,fsuid:0,fsgid:0,cap_inh:u64::MAX,cap_prm:u64::MAX,cap_eff:u64::MAX,cap_bset:u64::MAX,ns_uts:1,ns_ipc:1,ns_mnt:1,ns_pid:1,ns_net:1,ns_user:1,ns_cgroup:1,ns_time:1,seccomp_mode:0,no_new_privs:false,umask:0,groups:[0;32],ngroups:0})
 }
 pub fn root(&self)->PageTableRoot{PageTableRoot{pml4:self.cr3}}
 pub fn clear_child_tid(&self)->u64{self.clear_child_tid}
 pub fn set_clear_child_tid(&mut self,v:u64){self.clear_child_tid=v}
 pub fn set_exe_path(&mut self,p:&str){let b=p.as_bytes();let n=core::cmp::min(b.len(),64);self.exe_path[..n].copy_from_slice(&b[..n]);self.exe_len=n;}
 pub fn set_exec_meta(&mut self,l:&crate::exec_runtime::Loaded){self.cmdline=l.cmdline;self.cmdline_len=l.cmdline_len;self.environ=l.environ;self.environ_len=l.environ_len;self.segs=l.segs;self.nsegs=l.nsegs;self.stack_base=l.stack_base;self.stack_top=l.stack_top;}
 /* Virtual size in bytes: committed image segments, stack, brk and ledger areas. */
 pub fn vma_bytes(&self)->u64{let mut total=0u64;for s in &self.segs[..self.nsegs]{total=total.saturating_add(s.end-s.start)}if self.stack_top>self.stack_base{total=total.saturating_add(self.stack_top-self.stack_base)}let brk_span=Self::up(self.brk).map(|b|b as u64).unwrap_or(0).saturating_sub(BRK_BASE);total=total.saturating_add(brk_span);let stats=self.vm.resource_stats();total.saturating_add(stats.bytes as u64)}
 /* Resident user pages across segments, stack, brk and ledger areas. */
 pub fn resident_pages(&self)->u64{
  let root=self.root();let phys=Identity;let mut resident=0u64;let mut scan=|start:u64,end:u64,resident:&mut u64|{let mut va=start;while va<end{if root.query(&phys,va).map(|(_,f)|f&PRESENT!=0&&f&USER!=0).unwrap_or(false){*resident+=1}va+=PAGE;}};
  for s in &self.segs[..self.nsegs]{scan(s.start,s.end,&mut resident)}
  if self.stack_top>self.stack_base{scan(self.stack_base,self.stack_top,&mut resident)}
  for i in 0..self.vm.resource_stats().areas{if let Some(a)=self.vm.area(i){scan(a.start as u64,a.end as u64,&mut resident)}}
  resident
 }
 /* madvise(DONTNEED/FREE): drop physical frames but keep the reservation. */
 pub fn madvise(&mut self,addr:usize,len:usize,advice:u64)->Result<usize,i64>{
  if addr&4095!=0{return Err(crate::errno::EINVAL)}
  if len==0{return Ok(0)}
  let size=Self::up(len as u64)?;
  match advice{
   /* MADV_DONTNEED/MADV_FREE: pages read as zero again on next touch. */
   4|8=>{self.unmap_pages(addr as u64,size as u64)?;Ok(0)}
   _=>Ok(0),
  }
 }
 pub fn cwd_str(&self)->&[u8]{&self.cwd[..self.cwd_len]}
 pub fn set_cwd(&mut self,p:&str)->bool{let b=p.as_bytes();if b.len()>=128{return false}self.cwd=[0;128];self.cwd[..b.len()].copy_from_slice(b);self.cwd_len=b.len().max(1);true}
 pub fn root_str(&self)->&[u8]{&self.root[..self.root_len]}
 pub fn set_root(&mut self,p:&str)->bool{let b=p.as_bytes();if b.len()>=128{return false}self.root=[0;128];self.root[..b.len()].copy_from_slice(b);self.root_len=b.len().max(1);true}
 pub fn reset_exec_state(&mut self){self.brk=BRK_BASE;self.vm=Map::new();self.clear_child_tid=0;self.handlers=crate::signal::handlers_dfl();self.sigframe=0;self.robust_list_head=0;self.robust_list_len=0;}
 fn pte_flags(prot:u64)->Result<u64,i64>{if prot&!7!=0{return Err(crate::errno::EINVAL)}Ok(USER|if prot&2!=0{WRITABLE}else{0}|if prot&4==0{NX}else{0})}
 fn vm_flags(prot:u64)->Result<VmFlags,i64>{let mut f=VmFlags::USER;let _=Self::pte_flags(prot)?;if prot&1!=0{f.0|=VmFlags::READ.0}if prot&2!=0{f.0|=VmFlags::WRITE.0}if prot&4!=0{f.0|=VmFlags::EXEC.0}f.0|=VmFlags::ANON.0;Ok(f)}
  fn unmap_pages(&self,start:u64,len:u64)->Result<(),i64>{let root=self.root();let phys=Identity;let end=start.checked_add(len).ok_or(crate::errno::EINVAL)?;let mut va=start;while va<end{match root.unmap_page(&phys,va){Ok(pa)=>{if self.vm.flags_at(va as usize).unwrap_or(VmFlags(0)).contains(VmFlags::EXTERNAL){crate::fd::iouring_mapping_release(pa)}else{crate::memory::release_frame(pa)?}}Err(e)=>{if e!=crate::errno::ENOENT{return Err(e)}}}va+=PAGE;}Ok(())}
  fn unmap_external_mappings(root:&PageTableRoot,vm:&Map){let access=Identity;for i in 0..vm.resource_stats().areas{if let Some(area)=vm.area(i){if !area.flags.contains(VmFlags::EXTERNAL){continue}let mut va=area.start as u64;while va<area.end as u64{if let Ok(frame)=root.unmap_page(&access,va){crate::fd::iouring_mapping_release(frame)}va+=PAGE;}}}}
 fn up(v:u64)->Result<usize,i64>{v.checked_add(PAGE-1).map(|x|(x&!(PAGE-1))as usize).ok_or(crate::errno::ENOMEM)}
 pub fn brk(&mut self,request:u64)->u64{let old=self.brk;if request==0{return old}if request<BRK_BASE||request>=crate::usercopy::USER_MAX as u64{return old}let old_page=match Self::up(old){Ok(v)=>v,Err(_)=>return old};let new_page=match Self::up(request){Ok(v)=>v,Err(_)=>return old};if new_page>old_page{let len=new_page-old_page;let flags=match Self::vm_flags(3){Ok(v)=>v,Err(_)=>return old};if self.vm.insert(old_page,len,flags,VmKind::Brk,false).is_err(){return old}}else if new_page<old_page{return old}self.brk=request;request}
 pub fn mmap(&mut self,addr:usize,len:usize,prot:u64,flags:u64,fd:i64,offset:u64)->Result<usize,i64>{
  const MAP_FIXED:u64=0x10;const MAP_ANON:u64=0x20;const MAP_PRIVATE:u64=0x02;const MAP_SHARED:u64=0x01;const MAP_POPULATE:u64=0x8000;
  if len==0{return Err(crate::errno::EINVAL)}
  let map_type=flags&(MAP_PRIVATE|MAP_SHARED);
  if map_type==0||map_type==(MAP_PRIVATE|MAP_SHARED){return Err(crate::errno::EINVAL)}
  let anon=flags&MAP_ANON!=0;
 let iouring=!anon && self.fds.get(fd as usize).map(|id|matches!(crate::fd::kind(id),Some(crate::fd::OpenKind::IoUring))).unwrap_or(false);
 if anon&&(fd!=-1){return Err(crate::errno::EINVAL)}
 if !anon && !iouring &&(fd<0||offset%4096!=0){return Err(crate::errno::EINVAL)}
 if flags&!(MAP_FIXED|MAP_ANON|MAP_PRIVATE|MAP_SHARED|MAP_POPULATE|0x20000|0x100)!=0{return Err(crate::errno::EINVAL)}
 if !anon && !iouring &&(flags&MAP_SHARED!=0){return Err(crate::errno::ENODEV)}
  let size=Self::up(len as u64)?;
  let mut vf=Self::vm_flags(prot)?;
  if !iouring&&flags&MAP_SHARED!=0{vf.0|=VmFlags::SHARED.0;}
  if iouring{if flags&MAP_SHARED==0||size!=PAGE as usize{return Err(crate::errno::EINVAL)}vf.0|=VmFlags::EXTERNAL.0;}
 let (vma_kind,vma_cow)=if iouring {
  (VmKind::Anon,false)
 }else if anon {
  (VmKind::Anon,false)
 }else{
  let id=self.fds.get(fd as usize).ok_or(crate::errno::EBADF)?;
  let inode=match crate::fd::kind(id){Some(crate::fd::OpenKind::Inode(n))=>n,_=>{return Err(crate::errno::ENODEV)}};
  if prot&2!=0{return Err(crate::errno::ENODEV)}
  (VmKind::File{inode,offset}, flags&MAP_PRIVATE!=0)
 };
 let start=if flags&MAP_FIXED!=0{if addr&4095!=0{return Err(crate::errno::EINVAL)}self.vm.insert(addr,size,vf,vma_kind,vma_cow)?;addr}else{self.vm.allocate(size,vf,vma_kind,vma_cow)?};
 if iouring{
  let id=self.fds.get(fd as usize).ok_or(crate::errno::EBADF)?;
  let frame=match offset as u64 {0=>crate::fd::iouring_sq_ring(id),0x8000000=>crate::fd::iouring_cq_ring(id),0x10000000=>crate::fd::iouring_sqes(id),_=>0};
   if frame==0{let _=self.vm.remove(start,size);return Err(crate::errno::EINVAL)}
   let pte=Self::pte_flags(prot)?;
   if let Err(e)=self.root().map_page(&crate::arch::x86_64::phys::Identity,start as u64,frame,pte){let _=self.vm.remove(start,size);return Err(e)}
   if let Err(e)=crate::fd::iouring_mapping_acquire(id){let _=self.root().unmap_page(&crate::arch::x86_64::phys::Identity,start as u64);let _=self.vm.remove(start,size);return Err(e)}
 }
 /* Anonymous, file-backed and brk pages are demand-paged by the fault
    handler: the VMA ledger keeps kind and offset, so mmap only reserves. */
 if flags&MAP_POPULATE!=0&&!iouring{
  let writable=vf.contains(VmFlags::WRITE);
  let mut va=start as u64;let end=(start+size)as u64;
  while va<end{
   if !(self.root().query(&Identity,va).map(|(_,f)|f&PRESENT!=0&&(!writable||f&WRITABLE!=0)).unwrap_or(false)){
    if self.handle_page_fault(va,writable).is_err(){let _=self.munmap(start,size);return Err(crate::errno::ENOMEM)}
   }
   va+=PAGE;
  }
 }
 Ok(start)
 }
  /* Grows or shrinks an anonymous mapping: allocate the new range, copy the
     live bytes across, then release the old range. */
  pub fn mremap(&mut self,old:usize,old_size:usize,new_size:usize,flags:u64,new_addr:u64)->Result<usize,i64>{
   const MREMAP_MAYMOVE:u64=1; const MREMAP_FIXED:u64=2;
   if old%4096!=0||old_size==0||new_size==0{return Err(crate::errno::EINVAL)}
   if flags&!(MREMAP_MAYMOVE|MREMAP_FIXED)!=0{return Err(crate::errno::EINVAL)}
   if self.vm.flags_at(old).unwrap_or(VmFlags(0)).contains(VmFlags::EXTERNAL){return Err(crate::errno::EOPNOTSUPP)}
   let vf=Self::vm_flags(3)?;
   let dst=if flags&MREMAP_FIXED!=0{
    if new_addr as usize %4096!=0{return Err(crate::errno::EINVAL)}
    let _=self.munmap(new_addr as usize,new_size);
    if let Err(e)=self.vm.insert(new_addr as usize,new_size,vf,VmKind::Anon,false){return Err(e)}
    new_addr as usize
   }else{
    self.vm.allocate(new_size,vf,VmKind::Anon,false)?
   };
   /* Runs under with_current_process, so usercopy cannot be reused here:
      fault both sides in through the process itself, then copy the frames. */
   let root=self.root();let phys=Identity;
   let ok=|va:u64,write:bool|root.query(&phys,va).map(|(_,f)|f&crate::arch::x86_64::paging::PRESENT!=0&&f&crate::arch::x86_64::paging::USER!=0&&(!write||f&crate::arch::x86_64::paging::WRITABLE!=0)).unwrap_or(false);
   let total=core::cmp::min(old_size,new_size);
   let mut done=0usize;
   while done<total{
    let n=core::cmp::min(PAGE as usize,total-done);
    if !ok((old+done) as u64,false)&&self.handle_page_fault((old+done) as u64,false).is_err(){let _=self.munmap(old,old_size);return Err(crate::errno::EFAULT)}
    if !ok((dst+done) as u64,true)&&self.handle_page_fault((dst+done) as u64,true).is_err(){let _=self.munmap(old,old_size);return Err(crate::errno::EFAULT)}
    let (spa,_)=match root.query(&phys,(old+done) as u64){Some(v)=>v,None=>{let _=self.munmap(old,old_size);return Err(crate::errno::EFAULT)}};
    let (dpa,_)=match root.query(&phys,(dst+done) as u64){Some(v)=>v,None=>{let _=self.munmap(old,old_size);return Err(crate::errno::EFAULT)}};
    unsafe{phys.bytes(dpa,n).copy_from_slice(phys.bytes(spa,n))}
    done+=PAGE as usize;
   }
   let _=self.munmap(old,old_size);
   Ok(dst)
  }
 pub fn munmap(&mut self,addr:usize,len:usize)->Result<(),i64>{if addr&4095!=0||len==0{return Err(crate::errno::EINVAL)}let size=Self::up(len as u64)?;self.unmap_pages(addr as u64,size as u64)?;self.vm.remove(addr,size)}
 pub fn mprotect(&mut self,addr:usize,len:usize,prot:u64)->Result<(),i64>{
 if addr&4095!=0||len==0{return Err(crate::errno::EINVAL)}let size=Self::up(len as u64)?;let f=Self::vm_flags(prot)?;let root=self.root();let phys=Identity;let flags=Self::pte_flags(prot)?;let mut va=addr as u64;let end=va+size as u64;while va<end{
  /* Demand-paged pages have no PTE yet: protection lives in the VMA ledger
     until the fault handler installs the mapping with the recorded flags. */
  /* Shared COW pages must stay read-only; a later write fault upgrades them. */
  let cow=self.vm.find_vma(va as usize).map(|a|a.cow).unwrap_or(false);
  if !(cow&&flags&crate::arch::x86_64::paging::WRITABLE!=0){
   if let Err(e)=root.protect_page(&phys,va,flags){if e!=crate::errno::ENOENT{return Err(e)}}
  }
  va+=PAGE;
 }
 /* Execve images have no VMA ledger entry; treat that as bookkeeping only. */
 let _=self.vm.protect(addr,size,f);Ok(())}
 pub fn mapped_for_test(&self,addr:u64,write:bool)->bool{self.root().query(&Identity,addr).map(|(_,f)|f&PRESENT!=0&&f&USER!=0&&(!write||f&WRITABLE!=0)).unwrap_or(false)}
 pub fn mlock(&mut self,addr:usize,len:usize)->Result<usize,i64>{if addr&4095!=0||len==0{return Err(crate::errno::EINVAL)}self.vm.set_locked_range(addr,len,true)?;Ok(0)}
 pub fn munlock(&mut self,addr:usize,len:usize)->Result<usize,i64>{if addr&4095!=0||len==0{return Err(crate::errno::EINVAL)}self.vm.set_locked_range(addr,len,false)?;Ok(0)}
 pub fn handle_page_fault(&mut self,addr:u64,write:bool)->Result<(),()>{
  if addr as usize >= crate::usercopy::USER_MAX {return Err(())}
  let page=(addr as usize)&!4095;
  let root=self.root();
  let phys=crate::arch::x86_64::phys::Identity;
  // Present PTE: fork marks every shared private page read-only, so a write
  // fault here is COW unless the mapping itself is read-only on purpose.
  if let Some((pa,pte_flags))=root.query(&phys,addr){
   if pte_flags & crate::arch::x86_64::paging::PRESENT != 0 {
    if write && pte_flags & crate::arch::x86_64::paging::WRITABLE == 0 && pte_flags & crate::arch::x86_64::paging::USER != 0 {
     if let Some(vma)=self.vm.find_vma(addr as usize){
      if !vma.flags.contains(VmFlags::WRITE){return Err(())}
     }
     let frame=pa & crate::arch::x86_64::paging::ADDR;
     if crate::memory::refcount(frame)>1 {
      let new=crate::memory::alloc_frame().ok_or(())?;
      unsafe{phys.table(new).copy_from_slice(phys.table(frame));}
      let _=root.unmap_page(&phys,page as u64);
      root.map_page(&phys,page as u64,new,pte_flags & !crate::arch::x86_64::paging::ADDR | crate::arch::x86_64::paging::WRITABLE).map_err(|_|())?;
      let _=crate::memory::refcount_dec(frame);
     }else{
      /* Sole owner (for example after the COW sibling exited): promote in place. */
      root.protect_page(&phys,page as u64,pte_flags & !crate::arch::x86_64::paging::ADDR | crate::arch::x86_64::paging::WRITABLE).map_err(|_|())?;
     }
     return Ok(())
    }
    return Err(())
   }
  }
  let vma=self.vm.find_vma(addr as usize).ok_or(())?;
  // Not present: allocate/map.
  let mut prot=0u64;
  if vma.flags.contains(VmFlags::READ){prot|=1}
  if vma.flags.contains(VmFlags::WRITE){prot|=2}
  if vma.flags.contains(VmFlags::EXEC){prot|=4}
  if (!vma.flags.contains(VmFlags::READ)&&!vma.flags.contains(VmFlags::WRITE)&&!vma.flags.contains(VmFlags::EXEC))||(write&&!vma.flags.contains(VmFlags::WRITE)){return Err(())}
  let pf=Self::pte_flags(prot).map_err(|_|())?;
  match vma.kind {
   VmKind::File{inode,offset}=>{
    let frame=crate::memory::alloc_frame().ok_or(())?;
    unsafe{phys.table(frame).fill(0);}
    let file_off=offset + (page as u64 - vma.start as u64);
    let mut buf=[0u8;4096];
    let _=crate::vfs::read_off(inode,file_off,&mut buf);
    unsafe{core::slice::from_raw_parts_mut(frame as *mut u8,4096).copy_from_slice(&buf);}
    root.map_page(&phys,page as u64,frame,pf).map_err(|_|())?
   }
   _=>{
    let frame=crate::memory::alloc_frame().ok_or(())?;
    unsafe{phys.table(frame).fill(0);}
    root.map_page(&phys,page as u64,frame,pf).map_err(|_|())?
   }
  }
  Ok(())
 }
 /* COW fork: shares the private frames of the caller with a fresh CR3, then
    revokes write access on both sides so the next store faults into the COW
    path. Frames carry refcounts; teardown releases them through release_frame. */
 pub fn clone_address_space(&self)->Result<u64,i64>{
  let access=Identity;
  let mut all=[self.image_slot,self.stack_slot,self.mmap_slot];let mut n=0usize;
  for i in 0..all.len(){let s=all[i];if !all[..n].contains(&s){all[n]=s;n+=1;}}
  let slots=&all[..n];
  let child=self.root().clone_bootstrap(&access,slots)?;
  for &slot in slots{if let Err(e)=share_slot(&self.root(),&access,&child,slot){for &done in slots{let _=child.destroy_private_slot(&access,done);}let _=crate::memory::free_frame(child.pml4);return Err(e)}}
  for &slot in slots{make_slot_readonly(&self.root(),&access,slot)}
  /* MAP_SHARED pages were never meant to be copied: re-enable writes on
     both sides so the shared frames stay shared (no COW on either side). */
  self.vm.shared_writable_ranges(|s,e|{
   writable_range(&access,&self.root(),s as u64,e as u64);
   writable_range(&access,&child,s as u64,e as u64);
  });
  /* Stale writable TLB entries would bypass the COW fault; reload CR3. */
  unsafe{PageTableRoot{pml4:self.cr3}.activate()}
  Ok(child.pml4)
 }
  pub fn fork_child(&self,cr3:u64)->Result<Self,i64>{if self.seccomp_filter!=0{seccomp_filter_acquire(self.seccomp_filter as u64)?;}Ok(Self{cr3,bootstrap_cr3:self.bootstrap_cr3,image_slot:self.image_slot,stack_slot:self.stack_slot,mmap_slot:self.mmap_slot,vm:self.vm.clone(),brk:self.brk,clear_child_tid:0,fds:self.fds.clone_incref(),pending:self.pending,sigmask:self.sigmask,si_code:self.si_code,si_value:self.si_value,si_overrun:self.si_overrun,robust_list_head:self.robust_list_head,robust_list_len:self.robust_list_len,handlers:self.handlers,sigframe:0,exe_path:self.exe_path,exe_len:self.exe_len,cwd:self.cwd,cwd_len:self.cwd_len,root:self.root,root_len:self.root_len,name:self.name,hostname:self.hostname,
 cmdline:self.cmdline,cmdline_len:self.cmdline_len,environ:self.environ,environ_len:self.environ_len,segs:self.segs,nsegs:self.nsegs,stack_base:self.stack_base,stack_top:self.stack_top,
 seccomp_filter:self.seccomp_filter,seccomp_filter_len:self.seccomp_filter_len,uid:self.uid,gid:self.gid,euid:self.euid,egid:self.egid,suid:self.suid,sgid:self.sgid,fsuid:self.fsuid,fsgid:self.fsgid,cap_inh:self.cap_inh,cap_prm:self.cap_prm,cap_eff:self.cap_eff,cap_bset:self.cap_bset,ns_uts:self.ns_uts,ns_ipc:self.ns_ipc,ns_mnt:self.ns_mnt,ns_pid:self.ns_pid,ns_net:self.ns_net,ns_user:self.ns_user,ns_cgroup:self.ns_cgroup,ns_time:self.ns_time,seccomp_mode:self.seccomp_mode,no_new_privs:self.no_new_privs,umask:self.umask,groups:self.groups,ngroups:self.ngroups})}
 /* Releases address space and descriptors after an unrecoverable spawn failure. */
  pub fn abandon(&mut self){let access=Identity;let root=self.root();Self::unmap_external_mappings(&root,&self.vm);for &slot in &[self.image_slot,self.stack_slot,self.mmap_slot]{let _=root.destroy_private_slot(&access,slot);}let _=crate::memory::free_frame(self.cr3);if self.seccomp_filter!=0{let _=seccomp_filter_release(self.seccomp_filter as u64);self.seccomp_filter=0;self.seccomp_filter_len=0;}self.fds.close_all();}
 /* Runs on exit and on fatal user faults; ends on the bootstrap tables. */
  pub fn teardown_exit(&mut self,status:i32,vector:u64){
   let fd_descriptors=self.fds.descriptor_count();
   let fd_descriptions=self.fds.open_description_count();
   let vm=self.vm.resource_stats();
   let tasks=crate::task::resource_stats();
   crate::println!("resources: exit pid={} fds={} open_files={} mmap_areas={} mmap_pages={} tasks_active={} tasks_zombie={}",crate::task::current_pid(),fd_descriptors,fd_descriptions,vm.areas,vm.pages,tasks.active,tasks.zombies);
   let bootstrap=PageTableRoot{pml4:self.bootstrap_cr3};unsafe{bootstrap.activate()}
   let access=Identity;let root=self.root();Self::unmap_external_mappings(&root,&self.vm);
   let mut cleanup_ok=true;
   if let Err(e)=root.destroy_private_slot(&access,self.image_slot){crate::println!("warning: image slot teardown failed errno={}",e);cleanup_ok=false}
   if self.stack_slot!=self.image_slot{if let Err(e)=root.destroy_private_slot(&access,self.stack_slot){crate::println!("warning: stack slot teardown failed errno={}",e);cleanup_ok=false}}
   if self.mmap_slot!=self.image_slot&&self.mmap_slot!=self.stack_slot{if let Err(e)=root.destroy_private_slot(&access,self.mmap_slot){crate::println!("warning: mmap slot teardown failed errno={}",e);cleanup_ok=false}}
   if let Err(e)=crate::memory::free_frame(self.cr3){crate::println!("warning: process PML4 recycle failed errno={}",e);cleanup_ok=false}
   if self.seccomp_filter!=0{if let Err(e)=seccomp_filter_release(self.seccomp_filter as u64){crate::println!("warning: seccomp filter release failed errno={}",e);cleanup_ok=false}self.seccomp_filter=0;self.seccomp_filter_len=0;}
   crate::record_lock::release_pid(crate::task::current_pid());
   let closed_fds=self.fds.close_all();
   self.vm.clear();
   if closed_fds!=fd_descriptors||self.fds.descriptor_count()!=0||self.vm.resource_stats()!=crate::vm::ResourceStats::default(){cleanup_ok=false}
   if cleanup_ok{crate::println!("resources: reclaimed pid={} fds={} mmap_areas={} mmap_pages={}",crate::task::current_pid(),closed_fds,vm.areas,vm.pages)}else{crate::println!("resources: cleanup incomplete pid={} fds_closed={} mmap_areas={} mmap_pages={}",crate::task::current_pid(),closed_fds,vm.areas,vm.pages)}
   crate::println!("process address space reclaimed status={} vector={}",status,vector);
 }
 /* Swaps in a freshly committed exec image and tears down the old one. */
  pub fn replace_image(&mut self,new_cr3:u64,image_slot:usize,stack_slot:usize)->Result<(),i64>{
   let old=(self.cr3,self.image_slot,self.stack_slot,self.mmap_slot);let old_vm=self.vm.clone();
  unsafe{PageTableRoot{pml4:new_cr3}.activate()}
  crate::task::bind_current_address_space(new_cr3,crate::task::current_kernel_stack_top());
  self.cr3=new_cr3;self.image_slot=image_slot;self.stack_slot=stack_slot;
   let access=Identity;let root=PageTableRoot{pml4:old.0};Self::unmap_external_mappings(&root,&old_vm);
  if root.destroy_private_slot(&access,old.1).is_err(){crate::println!("warning: old exec image teardown failed")}
   if old.2!=old.1&&root.destroy_private_slot(&access,old.2).is_err(){crate::println!("warning: old exec stack teardown failed")}
   if old.3!=old.1&&old.3!=old.2&&root.destroy_private_slot(&access,old.3).is_err(){crate::println!("warning: old exec mmap teardown failed")}
  if crate::memory::free_frame(old.0).is_err(){crate::println!("warning: old exec PML4 recycle failed")}Ok(())
 }
}
impl Drop for Process{fn drop(&mut self){if self.seccomp_filter!=0{let _=seccomp_filter_release(self.seccomp_filter as u64);self.seccomp_filter=0;self.seccomp_filter_len=0;}}}
pub fn fs_base()->u64{crate::task::fs_base()}
pub fn set_fs_base(v:u64)->bool{if v<crate::usercopy::USER_MAX as u64{crate::task::set_current_fs_base(v);true}else{false}}
/* Re-asserts WRITABLE on present leaves of [start,end) so shared anonymous
   pages keep behaving like one frame in both address spaces. */
fn writable_range(a:&Identity,root:&PageTableRoot,start:u64,end:u64){
 use crate::arch::x86_64::paging::{PRESENT,WRITABLE,ADDR};
 let pml4=unsafe{a.table(root.pml4)};
 let mut va=start&!(4096-1);
 while va<end{
  let e4=pml4[(va>>39&511)as usize];
  if e4&PRESENT!=0{
   let pdpt=unsafe{a.table(e4&ADDR)};
   let e3=pdpt[(va>>30&511)as usize];
   if e3&PRESENT!=0&&e3&(1<<7)==0{
    let pd=unsafe{a.table(e3&ADDR)};
    let e2=pd[(va>>21&511)as usize];
    if e2&PRESENT!=0&&e2&(1<<7)==0{
     let pt=unsafe{a.table(e2&ADDR)};
     let i1=(va>>12&511)as usize;
     let e1=pt[i1];
     if e1&PRESENT!=0{pt[i1]=e1|WRITABLE;}
    }
   }
  }
  va+=4096;
 }
}
fn make_slot_readonly(root:&PageTableRoot,a:&Identity,slot:usize){
 let pml4=unsafe{a.table(root.pml4)};
 let top=pml4[slot];
 if top&crate::arch::x86_64::paging::PRESENT==0{return;}
 let pdpt=unsafe{a.table(top&crate::arch::x86_64::paging::ADDR)};
 for i in 0..512{
  let e3=pdpt[i];
  if e3&crate::arch::x86_64::paging::PRESENT==0{continue;}
  if e3&(1<<7)!=0{continue;}
  let pd=unsafe{a.table(e3&crate::arch::x86_64::paging::ADDR)};
  for j in 0..512{
   let e2=pd[j];
   if e2&crate::arch::x86_64::paging::PRESENT==0{continue;}
   if e2&(1<<7)!=0{continue;}
   let pt=unsafe{a.table(e2&crate::arch::x86_64::paging::ADDR)};
   for k in 0..512{
    if pt[k]&crate::arch::x86_64::paging::PRESENT!=0{pt[k]&=!crate::arch::x86_64::paging::WRITABLE;}
   }
  }
 }
}
fn share_slot(src:&PageTableRoot,a:&Identity,dst:&PageTableRoot,slot:usize)->Result<(),i64>{
 use crate::arch::x86_64::paging::{PRESENT,WRITABLE,USER,ADDR,NX};
 let pml4_src=unsafe{a.table(src.pml4)};
 let top=pml4_src[slot];
 if top&PRESENT==0{return Ok(())}
 let pdpt_pa=crate::memory::alloc_frame().ok_or(crate::errno::ENOMEM)?;
 unsafe{a.table(pdpt_pa).fill(0)}
 unsafe{a.table(dst.pml4)[slot]=pdpt_pa|PRESENT|WRITABLE|USER}
 let pdpt_src=unsafe{a.table(top&ADDR)};
 for i in 0..512{
  let e3=pdpt_src[i];
  if e3&PRESENT==0{continue;}
  if e3&(1<<7)!=0{return Err(crate::errno::EINVAL)}
  let pd_pa=crate::memory::alloc_frame().ok_or(crate::errno::ENOMEM)?;
  unsafe{a.table(pd_pa).fill(0)}
  unsafe{a.table(pdpt_pa)[i]=pd_pa|PRESENT|WRITABLE|USER}
  let pd_src=unsafe{a.table(e3&ADDR)};
  for j in 0..512{
   let e2=pd_src[j];
   if e2&PRESENT==0{continue;}
   if e2&(1<<7)!=0{return Err(crate::errno::EINVAL)}
   let pt_pa=crate::memory::alloc_frame().ok_or(crate::errno::ENOMEM)?;
   unsafe{a.table(pt_pa).fill(0)}
   unsafe{a.table(pd_pa)[j]=pt_pa|PRESENT|WRITABLE|USER}
   let pt_src=unsafe{a.table(e2&ADDR)};
   for k in 0..512{
    let e1=pt_src[k];
    if e1&PRESENT==0{continue;}
    let pa=e1&ADDR;
    crate::memory::refcount_inc(pa);
    /* Shared leaves start read-only so the first store on either side faults. */
    let flags=((e1&0xfff)|(e1&NX))&!WRITABLE;
    unsafe{a.table(pt_pa)[k]=pa|flags;}
   }
  }
 }
 Ok(())
}
