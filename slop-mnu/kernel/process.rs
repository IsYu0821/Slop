//! Per-process state: private address space, VM ledger, brk and descriptors.
use crate::{arch::x86_64::{paging::{PhysAccess,PageTableRoot,PRESENT,WRITABLE,USER,NX,ADDR},phys::Identity},fd::FdTable,vm::{Map,VmFlags}};
const PAGE:u64=4096;
/* Slot 129 keeps the heap and anonymous mappings out of the firmware identity map. */
pub const BRK_BASE:u64=0x0000_4080_0000_0000;
pub fn slot_of(va:u64)->usize{((va>>39)&511)as usize}

pub struct Process{
 pub cr3:u64,pub bootstrap_cr3:u64,
 pub image_slot:usize,pub stack_slot:usize,pub mmap_slot:usize,
 pub vm:Map,pub brk:u64,pub clear_child_tid:u64,pub fds:FdTable,pub cwd:[u8;128],pub cwd_len:usize,pub root:[u8;128],pub root_len:usize,
 pub pending:u64,pub sigmask:u64,pub handlers:[crate::signal::Action;64],pub sigframe:u64,pub exe_path:[u8;64],pub exe_len:usize,pub name:[u8;16],pub hostname:[u8;65],
 pub uid:u32,pub gid:u32,pub euid:u32,pub egid:u32,pub suid:u32,pub sgid:u32,pub fsuid:u32,pub fsgid:u32,
 pub cap_inh:u64,pub cap_prm:u64,pub cap_eff:u64,pub cap_bset:u64,
 pub ns_uts:u64,pub ns_ipc:u64,pub ns_mnt:u64,pub ns_pid:u64,pub ns_net:u64,pub ns_user:u64,pub ns_cgroup:u64,pub ns_time:u64,
 pub seccomp_mode:u32,pub umask:u32,
 pub groups:[u32;32],pub ngroups:usize,
}
impl Process{
 pub fn new(cr3:u64,bootstrap_cr3:u64,image_slot:usize,stack_slot:usize)->Result<Self,i64>{
  Ok(Self{cr3,bootstrap_cr3,image_slot,stack_slot,mmap_slot:slot_of(crate::vm::MMAP_BASE as u64),vm:Map::new(),brk:BRK_BASE,clear_child_tid:0,fds:FdTable::stdio()?,pending:0,sigmask:0,handlers:crate::signal::handlers_dfl(),sigframe:0,exe_path:[0;64],exe_len:0,cwd:{let mut a=[0u8;128];a[0]=b'/';a},cwd_len:1,root:{let mut a=[0u8;128];a[0]=b'/';a},root_len:1,name:[0;16],hostname:{let mut a=[0u8;65];a[..4].copy_from_slice(b"slop");a},uid:0,gid:0,euid:0,egid:0,suid:0,sgid:0,fsuid:0,fsgid:0,cap_inh:u64::MAX,cap_prm:u64::MAX,cap_eff:u64::MAX,cap_bset:u64::MAX,ns_uts:1,ns_ipc:1,ns_mnt:1,ns_pid:1,ns_net:1,ns_user:1,ns_cgroup:1,ns_time:1,seccomp_mode:0,umask:0,groups:[0;32],ngroups:0})
 }
 pub fn root(&self)->PageTableRoot{PageTableRoot{pml4:self.cr3}}
 pub fn clear_child_tid(&self)->u64{self.clear_child_tid}
 pub fn set_clear_child_tid(&mut self,v:u64){self.clear_child_tid=v}
 pub fn set_exe_path(&mut self,p:&str){let b=p.as_bytes();let n=core::cmp::min(b.len(),64);self.exe_path[..n].copy_from_slice(&b[..n]);self.exe_len=n;}
 pub fn cwd_str(&self)->&[u8]{&self.cwd[..self.cwd_len]}
 pub fn set_cwd(&mut self,p:&str)->bool{let b=p.as_bytes();if b.len()>=128{return false}self.cwd=[0;128];self.cwd[..b.len()].copy_from_slice(b);self.cwd_len=b.len().max(1);true}
 pub fn root_str(&self)->&[u8]{&self.root[..self.root_len]}
 pub fn set_root(&mut self,p:&str)->bool{let b=p.as_bytes();if b.len()>=128{return false}self.root=[0;128];self.root[..b.len()].copy_from_slice(b);self.root_len=b.len().max(1);true}
 pub fn reset_exec_state(&mut self){self.brk=BRK_BASE;self.vm=Map::new();self.clear_child_tid=0;self.handlers=crate::signal::handlers_dfl();self.sigframe=0;}
 fn pte_flags(prot:u64)->Result<u64,i64>{if prot&!7!=0{return Err(crate::errno::EINVAL)}Ok(USER|if prot&2!=0{WRITABLE}else{0}|if prot&4==0{NX}else{0})}
 fn vm_flags(prot:u64)->Result<VmFlags,i64>{let mut f=VmFlags::USER;let _=Self::pte_flags(prot)?;if prot&1!=0{f.0|=VmFlags::READ.0}if prot&2!=0{f.0|=VmFlags::WRITE.0}if prot&4!=0{f.0|=VmFlags::EXEC.0}f.0|=VmFlags::ANON.0;Ok(f)}
 fn map_zeroed(&self,start:u64,len:u64,prot:u64)->Result<(),i64>{let root=self.root();let phys=Identity;let flags=Self::pte_flags(prot)?;let end=start.checked_add(len).ok_or(crate::errno::ENOMEM)?;let mut va=start;while va<end{let pa=match crate::memory::alloc_frame(){Some(v)=>v,None=>{self.rollback(start,va);return Err(crate::errno::ENOMEM)}};unsafe{phys.table(pa).fill(0)}if let Err(e)=root.map_page(&phys,va,pa,flags){let _=crate::memory::free_frame(pa);self.rollback(start,va);return Err(e)}va+=PAGE;}Ok(())}
 fn rollback(&self,start:u64,end:u64){let root=self.root();let phys=Identity;let mut va=start;while va<end{if let Ok(pa)=root.unmap_page(&phys,va){let _=crate::memory::free_frame(pa);}va+=PAGE;}}
 fn unmap_pages(&self,start:u64,len:u64)->Result<(),i64>{let root=self.root();let phys=Identity;let end=start.checked_add(len).ok_or(crate::errno::EINVAL)?;let mut va=start;while va<end{let pa=root.unmap_page(&phys,va)?;crate::memory::free_frame(pa)?;va+=PAGE;}Ok(())}
 fn up(v:u64)->Result<usize,i64>{v.checked_add(PAGE-1).map(|x|(x&!(PAGE-1))as usize).ok_or(crate::errno::ENOMEM)}
 pub fn brk(&mut self,request:u64)->u64{let old=self.brk;if request==0{return old}if request<BRK_BASE||request>=crate::usercopy::USER_MAX as u64{return old}let old_page=match Self::up(old){Ok(v)=>v,Err(_)=>return old};let new_page=match Self::up(request){Ok(v)=>v,Err(_)=>return old};if new_page>old_page{let len=new_page-old_page;let flags=match Self::vm_flags(3){Ok(v)=>v,Err(_)=>return old};if self.vm.insert(old_page,len,flags).is_err(){return old}if self.map_zeroed(old_page as u64,len as u64,3).is_err(){let _=self.vm.remove(old_page,len);return old}}else if new_page<old_page{return old}self.brk=request;request}
 pub fn mmap(&mut self,addr:usize,len:usize,prot:u64,flags:u64,fd:i64,offset:u64)->Result<usize,i64>{
 const MAP_FIXED:u64=0x10;const MAP_ANON:u64=0x20;const MAP_PRIVATE:u64=0x02;const MAP_SHARED:u64=0x01;
 if len==0{return Err(crate::errno::EINVAL)}
 let anon=flags&MAP_ANON!=0;
 if anon&&(fd!=-1){return Err(crate::errno::EINVAL)}
 if !anon&&(fd<0||offset%4096!=0){return Err(crate::errno::EINVAL)}
 if flags&!(MAP_FIXED|MAP_ANON|MAP_PRIVATE|MAP_SHARED|0x20000|0x100)!=0{return Err(crate::errno::EINVAL)}
 if !anon&&(flags&MAP_SHARED!=0){return Err(crate::errno::ENODEV)}
 let vf=Self::vm_flags(prot)?;
 let size=Self::up(len as u64)?;
 let start=if flags&MAP_FIXED!=0{if addr&4095!=0{return Err(crate::errno::EINVAL)}self.vm.insert(addr,size,vf)?;addr}else{self.vm.allocate(size,vf)?};
 if anon{
  if let Err(e)=self.map_zeroed(start as u64,size as u64,prot){let _=self.vm.remove(start,size);return Err(e)}
 }else{
  if let Err(e)=self.map_zeroed(start as u64,size as u64,prot){let _=self.vm.remove(start,size);return Err(e)}
  /* File-backed: copy file bytes into the mapped pages. */
  let id=match self.fds.get(fd as usize){Some(id)=>id,None=>{let _=self.vm.remove(start,size);return Err(crate::errno::EBADF)}};
  let inode=match crate::fd::kind(id){Some(crate::fd::OpenKind::Inode(n))=>n,_=>{let _=self.vm.remove(start,size);return Err(crate::errno::ENODEV)}};
  if prot&2!=0{let _=self.vm.remove(start,size);return Err(crate::errno::ENODEV)}
  let file_size=inode.size as usize;
  let off=offset as usize;
  let mut done=0usize;
  let mut buf=[0u8;512];
  while done<size{
   let remain=size-done;
   let foff=off+done;
   let n=if foff<file_size{core::cmp::min(buf.len(),core::cmp::min(remain,file_size-foff))}else{0};
   if n>0{
    let src=unsafe{core::slice::from_raw_parts(inode.data.add(foff),n)};
    buf[..n].copy_from_slice(src);
   }else{let z=core::cmp::min(buf.len(),remain);for b in &mut buf[..z]{*b=0}}
   let chunk=core::cmp::min(buf.len(),remain);
   if chunk>0{
    let dst=start+done;
    if let Err(e)=crate::usercopy::copy_to_user(dst,&buf[..chunk]){let _=self.vm.remove(start,size);return Err(e)}
   }
   done+=chunk;
  }
 }
 Ok(start)
 }
  /* Grows or shrinks an anonymous mapping: allocate the new range, copy the
     live bytes across, then release the old range. */
  pub fn mremap(&mut self,old:usize,old_size:usize,new_size:usize,flags:u64)->Result<usize,i64>{
   const MREMAP_MAYMOVE:u64=1;
   if old%4096!=0||old_size==0||new_size==0{return Err(crate::errno::EINVAL)}
   if flags&!MREMAP_MAYMOVE!=0{return Err(crate::errno::EINVAL)}
   let vf=Self::vm_flags(3)?;
   let dst=self.vm.allocate(new_size,vf)?;
   if self.map_zeroed(dst as u64,new_size as u64,3).is_err(){let _=self.vm.remove(dst,new_size);return Err(crate::errno::ENOMEM)}
   let mut chunk=[0u8;512];
   let mut done=0usize;
   while done<core::cmp::min(old_size,new_size){
    let n=core::cmp::min(chunk.len(),core::cmp::min(old_size,new_size)-done);
    if let Err(e)=crate::usercopy::copy_from_user(&mut chunk[..n],old+done){let _=self.munmap(old,old_size);return Err(e)}
    if let Err(e)=crate::usercopy::copy_to_user(dst+done,&chunk[..n]){let _=self.munmap(old,old_size);return Err(e)}
    done+=n;
   }
   let _=self.munmap(old,old_size);
   Ok(dst)
  }
 pub fn munmap(&mut self,addr:usize,len:usize)->Result<(),i64>{if addr&4095!=0||len==0{return Err(crate::errno::EINVAL)}let size=Self::up(len as u64)?;self.unmap_pages(addr as u64,size as u64)?;self.vm.remove(addr,size)}
 pub fn mprotect(&mut self,addr:usize,len:usize,prot:u64)->Result<(),i64>{
 if addr&4095!=0||len==0{return Err(crate::errno::EINVAL)}let size=Self::up(len as u64)?;let f=Self::vm_flags(prot)?;let root=self.root();let phys=Identity;let flags=Self::pte_flags(prot)?;let mut va=addr as u64;let end=va+size as u64;while va<end{root.protect_page(&phys,va,flags)?;va+=PAGE}
 /* Execve images have no VMA ledger entry; treat that as bookkeeping only. */
 let _=self.vm.protect(addr,size,f);Ok(())}
 pub fn mapped_for_test(&self,addr:u64,write:bool)->bool{self.root().query(&Identity,addr).map(|(_,f)|f&PRESENT!=0&&f&USER!=0&&(!write||f&WRITABLE!=0)).unwrap_or(false)}
 /* Deep-copies the private PML4 slots of the caller into a fresh CR3. */
 pub fn clone_address_space(&self)->Result<u64,i64>{
  let access=Identity;let slots=[self.image_slot,self.stack_slot,self.mmap_slot];
  let child=self.root().clone_bootstrap(&access,&slots)?;
  for &slot in &slots{if let Err(e)=copy_slot(&self.root(),&access,&child,slot){for &done in &slots{let _=child.destroy_private_slot(&access,done);}let _=crate::memory::free_frame(child.pml4);return Err(e)}}
  Ok(child.pml4)
 }
  pub fn fork_child(&self,cr3:u64)->Result<Self,i64>{Ok(Self{cr3,bootstrap_cr3:self.bootstrap_cr3,image_slot:self.image_slot,stack_slot:self.stack_slot,mmap_slot:self.mmap_slot,vm:self.vm.clone(),brk:self.brk,clear_child_tid:0,fds:self.fds.clone_incref(),pending:self.pending,sigmask:self.sigmask,handlers:self.handlers,sigframe:0,exe_path:self.exe_path,exe_len:self.exe_len,cwd:self.cwd,cwd_len:self.cwd_len,root:self.root,root_len:self.root_len,name:self.name,hostname:self.hostname,uid:self.uid,gid:self.gid,euid:self.euid,egid:self.egid,suid:self.suid,sgid:self.sgid,fsuid:self.fsuid,fsgid:self.fsgid,cap_inh:self.cap_inh,cap_prm:self.cap_prm,cap_eff:self.cap_eff,cap_bset:self.cap_bset,ns_uts:self.ns_uts,ns_ipc:self.ns_ipc,ns_mnt:self.ns_mnt,ns_pid:self.ns_pid,ns_net:self.ns_net,ns_user:self.ns_user,ns_cgroup:self.ns_cgroup,ns_time:self.ns_time,seccomp_mode:self.seccomp_mode,umask:self.umask,groups:self.groups,ngroups:self.ngroups})}
 /* Releases address space and descriptors after an unrecoverable spawn failure. */
 pub fn abandon(&mut self){let access=Identity;let root=self.root();for &slot in &[self.image_slot,self.stack_slot,self.mmap_slot]{let _=root.destroy_private_slot(&access,slot);}let _=crate::memory::free_frame(self.cr3);self.fds.close_all();}
 /* Runs on exit and on fatal user faults; ends on the bootstrap tables. */
 pub fn teardown_exit(&mut self,status:i32,vector:u64){
  let bootstrap=PageTableRoot{pml4:self.bootstrap_cr3};unsafe{bootstrap.activate()}
  let access=Identity;let root=self.root();
  if root.destroy_private_slot(&access,self.image_slot).is_err(){crate::println!("warning: image slot teardown failed")}
  if self.stack_slot!=self.image_slot&&root.destroy_private_slot(&access,self.stack_slot).is_err(){crate::println!("warning: stack slot teardown failed")}
  if self.mmap_slot!=self.image_slot&&self.mmap_slot!=self.stack_slot&&root.destroy_private_slot(&access,self.mmap_slot).is_err(){crate::println!("warning: mmap slot teardown failed")}
  if crate::memory::free_frame(self.cr3).is_err(){crate::println!("warning: process PML4 recycle failed")}
  self.fds.close_all();
  crate::println!("process address space reclaimed status={} vector={}",status,vector);
 }
 /* Swaps in a freshly committed exec image and tears down the old one. */
 pub fn replace_image(&mut self,new_cr3:u64,image_slot:usize,stack_slot:usize)->Result<(),i64>{
  let old=(self.cr3,self.image_slot,self.stack_slot);
  unsafe{PageTableRoot{pml4:new_cr3}.activate()}
  crate::task::bind_current_address_space(new_cr3,crate::task::current_kernel_stack_top());
  self.cr3=new_cr3;self.image_slot=image_slot;self.stack_slot=stack_slot;
  let access=Identity;let root=PageTableRoot{pml4:old.0};
  if root.destroy_private_slot(&access,old.1).is_err(){crate::println!("warning: old exec image teardown failed")}
  if old.2!=old.1&&root.destroy_private_slot(&access,old.2).is_err(){crate::println!("warning: old exec stack teardown failed")}
  if crate::memory::free_frame(old.0).is_err(){crate::println!("warning: old exec PML4 recycle failed")}Ok(())
 }
}
pub fn fs_base()->u64{crate::task::fs_base()}
pub fn set_fs_base(v:u64)->bool{if v<crate::usercopy::USER_MAX as u64{crate::task::set_current_fs_base(v);true}else{false}}
fn copy_slot(src:&PageTableRoot,a:&Identity,dst:&PageTableRoot,slot:usize)->Result<(),i64>{let pml4=unsafe{a.table(src.pml4)};let top=pml4[slot];if top&PRESENT==0{return Ok(())}if top&(1<<7)!=0{return Err(crate::errno::EINVAL)}
 let pdpt=unsafe{a.table(top&ADDR)};
 for i in 0..512{let e3=pdpt[i];if e3&PRESENT==0{continue}if e3&(1<<7)!=0{return Err(crate::errno::EINVAL)}let pd=unsafe{a.table(e3&ADDR)};
  for j in 0..512{let e2=pd[j];if e2&PRESENT==0{continue}if e2&(1<<7)!=0{return Err(crate::errno::EINVAL)}let pt=unsafe{a.table(e2&ADDR)};
   for k in 0..512{let e1=pt[k];if e1&PRESENT==0{continue}
    let pa=crate::memory::alloc_frame().ok_or(crate::errno::ENOMEM)?;
    unsafe{a.table(pa).copy_from_slice(unsafe{a.table(e1&ADDR)})}
    let va=((slot as u64)<<39)|((i as u64)<<30)|((j as u64)<<21)|((k as u64)<<12);
    if let Err(e)=dst.map_page(a,va,pa,(e1&0xfff)|(e1&NX)){let _=crate::memory::free_frame(pa);return Err(e)}
   }}}
 Ok(())
}
