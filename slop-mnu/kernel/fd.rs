//! Open-file descriptions and per-process descriptor tables.
//! dup and fork share descriptions, mirroring the Unix file-description model.
use crate::{errno,sync::SpinLock,vfs};
/* Linux allows 1024 descriptors per process and a far larger system-wide
   table; the old 32/64 limits made ordinary shells fail with EMFILE. */
pub const MAX_FDS:usize=1024;const MAX_OPEN:usize=512;
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub struct ResourceStats{pub descriptors:usize,pub open_descriptions:usize,pub open_references:usize}
#[derive(Clone,Copy)]pub enum OpenKind{Console,Inode(vfs::Inode),PipeRead(u8),PipeWrite(u8),Socket(u8),EventFd(u64,u8),Epoll,TimerFd(u64,u64),CharDevice(u64),BlockDevice(u64),Inotify,SignalFd(u64),Netlink,IoUring,PidFd(u32)}
pub const O_APPEND:u32=0o2000;
pub const O_NONBLOCK:u32=0o4000;
const STATUS_MASK:u32=O_APPEND|O_NONBLOCK;
const FD_CLOEXEC:u8=1;
#[derive(Clone,Copy)]struct OpenFile{refs:u32,offset:u64,flags:u32,kind:OpenKind}
static OPEN:SpinLock<[Option<OpenFile>;MAX_OPEN]>=SpinLock::new([const{None};MAX_OPEN]);
static mut IOURING_SQ_RING:[u64;MAX_OPEN]=[0;MAX_OPEN];
static mut IOURING_CQ_RING:[u64;MAX_OPEN]=[0;MAX_OPEN];
static mut IOURING_SQES:[u64;MAX_OPEN]=[0;MAX_OPEN];
static mut IOURING_ENTRIES:[u32;MAX_OPEN]=[0;MAX_OPEN];
static mut IOURING_SQ_HEAD:[u32;MAX_OPEN]=[0;MAX_OPEN];
static mut IOURING_CQ_TAIL:[u32;MAX_OPEN]=[0;MAX_OPEN];
static mut IOURING_MAP_REFS:[u32;MAX_OPEN]=[0;MAX_OPEN];
static mut IOURING_CLOSING:[bool;MAX_OPEN]=[false;MAX_OPEN];

const FREE_FD:i32=-1;
/* The close-on-exec flags are a bitmap: a descriptor table with a thousand
   entries must still fit in a kernel stack. */
const CLOSE_WORDS:usize=MAX_FDS/64;
pub struct FdTable{slots:[i32;MAX_FDS],close_exec:[u64;CLOSE_WORDS]}
impl FdTable{
 pub fn stdio()->Result<Self,i64>{let mut t=Self{slots:[FREE_FD;MAX_FDS],close_exec:[0;CLOSE_WORDS]};for fd in 0..3{let id=alloc_file(OpenKind::Console)?;t.slots[fd]=id as i32;}Ok(t)}
 pub fn get(&self,fd:usize)->Option<u32>{if fd>=MAX_FDS{return None}if self.slots[fd]<0{None}else{Some(self.slots[fd]as u32)}}
 fn clear_close(&mut self,fd:usize){self.close_exec[fd/64]&=!(1u64<<(fd%64));}
 fn set_close_bit(&mut self,fd:usize){self.close_exec[fd/64]|=1u64<<(fd%64);}
 fn close_bit(&self,fd:usize)->bool{self.close_exec[fd/64]&(1u64<<(fd%64))!=0}
 pub fn install(&mut self,id:u32)->Result<i32,i64>{self.install_from(0,id)}
 pub fn install_from(&mut self,min:usize,id:u32)->Result<i32,i64>{let fd=(min..MAX_FDS).find(|&i|self.slots[i]<0).ok_or(errno::EMFILE)?;self.slots[fd]=id as i32;self.clear_close(fd);Ok(fd as i32)}
 pub fn install_pair(&mut self,first:u32,second:u32)->Result<(i32,i32),i64>{let a=(0..MAX_FDS).find(|&i|self.slots[i]<0).ok_or(errno::EMFILE)?;self.slots[a]=first as i32;self.clear_close(a);let b=match (a+1..MAX_FDS).find(|&i|self.slots[i]<0){Some(b)=>b,None=>{self.slots[a]=FREE_FD;return Err(errno::EMFILE)}};self.slots[b]=second as i32;self.clear_close(b);Ok((a as i32,b as i32))}
 pub fn replace(&mut self,fd:usize,id:u32)->Result<(),i64>{if fd>=MAX_FDS{return Err(errno::EBADF)}if self.slots[fd]>=0{release_closed(self.slots[fd]as u32)}self.slots[fd]=id as i32;self.clear_close(fd);Ok(())}
   pub fn close(&mut self,fd:usize)->Result<(),i64>{if fd>=MAX_FDS{return Err(errno::EBADF)}let s=self.slots[fd];if s<0{return Err(errno::EBADF)}self.slots[fd]=FREE_FD;self.clear_close(fd);release_closed(s as u32);Ok(())}
   pub fn get_close_exec(&self,fd:usize)->Option<bool>{if fd>=MAX_FDS||self.slots[fd]<0{return None}Some(self.close_bit(fd))}
   pub fn set_close_exec(&mut self,fd:usize,v:bool)->Result<(),i64>{if fd>=MAX_FDS||self.slots[fd]<0{return Err(errno::EBADF)}if v{self.set_close_bit(fd)}else{self.clear_close(fd)}Ok(())}
   /* Closes every descriptor marked FD_CLOEXEC, as execve must. Sockets need
      the extra peer teardown that close(2) performs. */
   pub fn close_on_exec(&mut self)->usize{
    let mut closed=0;
    for i in 0..MAX_FDS{
     if !self.close_bit(i)||self.slots[i]<0{continue}
     let id=self.slots[i]as u32;
     self.slots[i]=FREE_FD;self.clear_close(i);
     /* release_closed frees the description (and its socket, if any) only
        on the last reference; never close the socket unconditionally, since
        fork-shared siblings may still hold it. */
     release_closed(id);
     closed+=1;
    }
    closed
   }
   pub fn clone_incref(&self)->Self{let mut t=Self{slots:self.slots,close_exec:self.close_exec};for &s in t.slots.iter(){if s>=0{incref(s as u32)}}t}
  pub fn descriptor_count(&self)->usize{self.slots.iter().filter(|&&s|s>=0).count()}
  pub fn open_fds(&self,mut cb:impl FnMut(usize)){for i in 0..MAX_FDS{if self.slots[i]>=0{cb(i)}}}
  pub fn open_description_count(&self)->usize{
   let mut ids=[FREE_FD;MAX_FDS];let mut count=0;
   for &slot in &self.slots{if slot<0||ids[..count].contains(&slot){continue}ids[count]=slot;count+=1;}
   count
  }
  pub fn close_all(&mut self)->usize{let mut closed=0;for i in 0..MAX_FDS{let s=self.slots[i];if s>=0{release(s as u32);self.slots[i]=FREE_FD;self.clear_close(i);closed+=1}}closed}
}
impl Drop for FdTable{fn drop(&mut self){let _=self.close_all();}}

pub fn resource_stats()->ResourceStats{
 let t=OPEN.lock();
 let mut stats=ResourceStats::default();
 for file in t.iter().flatten(){stats.open_descriptions+=1;stats.open_references+=file.refs as usize;}
 stats
}

pub fn resource_self_test()->bool{
 let before=resource_stats();
 let table=match FdTable::stdio(){Ok(t)=>t,Err(_)=>return false};
 if table.descriptor_count()!=3||table.open_description_count()!=3{return false}
 let after_create=resource_stats();
 if after_create.open_descriptions!=before.open_descriptions+3||after_create.open_references!=before.open_references+3{return false}
 let clone=table.clone_incref();
 if clone.descriptor_count()!=3||clone.open_description_count()!=3{return false}
 let after_clone=resource_stats();
 if after_clone.open_descriptions!=after_create.open_descriptions||after_clone.open_references!=after_create.open_references+3{return false}
 drop(clone);
 if resource_stats()!=after_create{return false}
 drop(table);
 resource_stats()==before
}

pub fn alloc_file(kind:OpenKind)->Result<u32,i64>{alloc_file_flags(kind,0)}
pub fn alloc_file_flags(kind:OpenKind,flags:u32)->Result<u32,i64>{
 let mut t=OPEN.lock();let id=t.iter().enumerate().position(|(i,s)|s.is_none()&&!unsafe{IOURING_CLOSING[i]}).ok_or(errno::EMFILE)?;
 t[id]=Some(OpenFile{refs:1,offset:0,flags:flags&STATUS_MASK,kind});drop(t);
 match kind{OpenKind::PipeRead(p)=>crate::pipe::add_reader(p),OpenKind::PipeWrite(p)=>crate::pipe::add_writer(p),_=>{}}
 Ok(id as u32)
}
pub fn incref(id:u32){if let Some(f)=OPEN.lock()[id as usize].as_mut(){f.refs+=1}}
pub fn release(id:u32){release_inner(id,false)}
/* A real close(2): drops the process's POSIX record locks on the inode even
   while other descriptors keep the description alive (Linux semantics). */
pub fn release_closed(id:u32){release_inner(id,true)}
fn release_inner(id:u32,closed:bool){
 let (kind,last)={let mut t=OPEN.lock();match t.get_mut(id as usize).and_then(Option::as_mut){None=>return,Some(f)=>{f.refs-=1;let last=f.refs==0;let k=f.kind;if last{t[id as usize]=None;}(k,last)}}};
 if closed{if let OpenKind::Inode(n)=kind{crate::record_lock::on_close(crate::task::current_pid(),n.ino);}}
 if !last{return}
 crate::flock::release(id);crate::record_lock::release_ofd(id);
 match kind{OpenKind::PipeRead(p)=>crate::pipe::close_reader(p),OpenKind::PipeWrite(p)=>crate::pipe::close_writer(p),OpenKind::Inotify=>crate::inotify::close(id),OpenKind::IoUring=>iouring_close(id),OpenKind::Socket(s)=>crate::socket::close(s),_=>{}}
}
pub fn kind(id:u32)->Option<OpenKind>{OPEN.lock()[id as usize].map(|f|f.kind)}
/* True when any open file description refers to the given inode number.
   Unlinking such a file must not recycle its frames: the open
   descriptions keep working on them, like Linux anonymous inodes. */
pub fn any_open_ino(ino:u64)->bool{
 let t=OPEN.lock();
 for slot in t.iter().flatten(){
  if let OpenKind::Inode(n)=slot.kind{if n.ino==ino{return true}}
 }
 false
}
/* Reprograms a signalfd mask (signalfd(fd, mask) with a valid fd). */
pub fn signalfd_set_mask(id:u32,mask:u64){if let Some(f)=OPEN.lock()[id as usize].as_mut(){if let OpenKind::SignalFd(m)=&mut f.kind{*m=mask;}}}
/* Readiness for poll/select/epoll: a signalfd is readable when any masked
   signal is pending in the calling process. */
pub fn signalfd_readable(mask:u64)->bool{crate::task::with_current_process(|p|p.pending&mask!=0).unwrap_or(false)}
/* Per-open-file status flags (O_APPEND/O_NONBLOCK), shared by dup and fork. */
pub fn flags(id:u32)->u32{OPEN.lock()[id as usize].map(|f|f.flags).unwrap_or(0)}
pub fn flags_set(id:u32,set:u32,clear:u32){if let Some(f)=OPEN.lock()[id as usize].as_mut(){f.flags=(f.flags|(set&STATUS_MASK))&!(clear&STATUS_MASK)}}
pub fn is_nonblock(id:u32)->bool{flags(id)&O_NONBLOCK!=0}
/* Linux signalfd_siginfo: 128 bytes, signo@0 errno@4 code@8 pid@12 uid@16
   fd@20 tid@24 band@28 overrun@32 trapno@36 status@40 int@44 ptr@48 ... */
fn signalfd_encode(dst:&mut [u8],signo:u32,errno_:i32,code:i32,pid:u32,uid:u32,status:i32,intval:i32,ptrval:u64){
 let mut b=[0u8;128];
 b[0..4].copy_from_slice(&signo.to_ne_bytes());
 b[4..8].copy_from_slice(&errno_.to_ne_bytes());
 b[8..12].copy_from_slice(&code.to_ne_bytes());
 b[12..16].copy_from_slice(&pid.to_ne_bytes());
 b[16..20].copy_from_slice(&uid.to_ne_bytes());
 /* ssi_fd/si_tid/si_band/overrun/trapno stay zero: no kernel pointers leak. */
 b[40..44].copy_from_slice(&status.to_ne_bytes());
 b[44..48].copy_from_slice(&intval.to_ne_bytes());
 b[48..56].copy_from_slice(&ptrval.to_ne_bytes());
 dst[..128].copy_from_slice(&b);
}
/* Reads one queued signal in the signalfd mask with Linux shared-pending
   semantics: the same pending bit backs signalfd, sigwaitinfo/sigtimedwait
   and handler delivery. Consuming here clears the bit (and the stored
   SIGCHLD status / one RT queue entry) but leaves child task states alone,
   so wait4/waitid still reap; a wait never clears the bit, so a later
   signalfd read still reports. First queued event wins while pending. */
fn signalfd_read(mask:u64,dst:&mut[u8],id:u32)->Result<usize,i64>{
 if dst.len()<128{return Err(errno::EINVAL)}
 loop{
  let found=crate::task::with_current_process(|p|{
   let m=p.pending&mask;
   if m==0{return None}
   let sig=(m.trailing_zeros()as i32)+1;
   let (code,pid,uid,status,intval,ptrval)=if sig==crate::signal::SIGCHLD{
    /* Child-status reports use the stored event, not the generic slots. */
    (p.sch_code,p.sch_pid,p.sch_uid,p.sch_status,0,0u64)
   }else if crate::signal::is_rt(sig){
    let i=crate::signal::rt_index(sig);
    let v=if p.rt_len[i]>0{p.rt_value[i][0]}else{0};
    (p.si_code[sig as usize],p.pending_from[sig as usize],0,v as i32,v as i32,v)
   }else{
    let c=p.si_code[sig as usize];
    let v=p.si_value[sig as usize];
    /* Plain kill reports SI_USER(0); sigqueue reports SI_QUEUE with value. */
    if c==crate::signal::SI_QUEUE{(c,p.pending_from[sig as usize],0,v as i32,v as i32,v)}
    else{(c,p.pending_from[sig as usize],0,0,0,0)}
   };
   /* Consume: clear the bit, drop one RT instance (keeping the bit when
      more remain), and reset the SIGCHLD slot. */
   p.pending&=!crate::signal::bit(sig);
   if sig==crate::signal::SIGCHLD{p.sch_code=0;p.sch_pid=0;p.sch_uid=0;p.sch_status=0;}
   if crate::signal::is_rt(sig){
    let i=crate::signal::rt_index(sig);
    if p.rt_len[i]>0{
     p.rt_len[i]-=1;
     let n=p.rt_len[i] as usize;
     let mut k=0;while k<n{p.rt_value[i][k]=p.rt_value[i][k+1];k+=1;}
     if p.rt_len[i]>0{p.pending|=crate::signal::bit(sig);}
    }
   }
   Some((sig as u32,code,pid,uid,status,intval,ptrval))
  }).flatten();
  if let Some((signo,code,pid,uid,status,intval,ptrval))=found{
   signalfd_encode(dst,signo,0,code,pid,uid,status,intval,ptrval);
   return Ok(128)
  }
  /* No masked signal queued: a handled out-of-mask signal interrupts. */
  if crate::task::has_deliverable_signal(){return Err(errno::EINTR)}
  if flags(id)&O_NONBLOCK!=0{return Err(errno::EAGAIN)}
  crate::task::block_current();
 }
}
/* Directory-walk cursor used by getdents64. */
pub fn offset_get(id:u32)->u64{OPEN.lock()[id as usize].map(|f|f.offset).unwrap_or(0)}
/* Positional variants used by pread64/pwrite64; they ignore the file offset. */
pub fn read_at(id:u32,off:u64,dst:&mut[u8])->Result<usize,i64>{
 let f=OPEN.lock()[id as usize].ok_or(errno::EBADF)?;
 match f.kind{
  OpenKind::Inode(n)=>vfs::read_off(n,off,dst),
  OpenKind::PipeRead(p)=>crate::pipe::read(p,dst,f.flags&O_NONBLOCK!=0),
  OpenKind::Socket(s)=>crate::socket::recv(s,dst,f.flags&O_NONBLOCK!=0),
  _=>Err(errno::EBADF),
 }
}
pub fn write_at(id:u32,off:u64,src:&[u8])->Result<usize,i64>{
 let f=OPEN.lock()[id as usize].ok_or(errno::EBADF)?;
 match f.kind{
  OpenKind::Inode(n)=>vfs::write_off(n,off,src),
  OpenKind::PipeWrite(p)=>crate::pipe::write(p,src,f.flags&O_NONBLOCK!=0),
  OpenKind::Socket(s)=>{crate::socket::send(s,src,f.flags&O_NONBLOCK!=0)},
  _=>Err(errno::EBADF),
 }
}
pub fn offset_set(id:u32,v:u64){if let Some(f)=OPEN.lock()[id as usize].as_mut(){f.offset=v}}

pub fn open(path:&str)->Result<i32,i64>{open_flags(path,0)}
pub fn open_flags(path:&str,status:u32)->Result<i32,i64>{let inode=vfs::lookup(path)?;let kind=if inode.mode&0o170000==0o020000{OpenKind::CharDevice(inode.rdev)}else if inode.mode&0o170000==0o060000{OpenKind::BlockDevice(inode.rdev)}else{OpenKind::Inode(inode)};let id=alloc_file_flags(kind,status)?;match crate::task::with_current_process(|p|p.fds.install(id)){Some(Ok(fd))=>Ok(fd),Some(Err(e))=>{release(id);Err(e)},None=>{release(id);Err(errno::ESRCH)}}}
/* O_CREAT path: creates the node when missing, then opens it. */
pub fn create_open(path:&str,mode:u32)->Result<i32,i64>{create_open_flags(path,mode,0)}
pub fn create_open_flags(path:&str,mode:u32,status:u32)->Result<i32,i64>{let inode=match vfs::lookup(path){Ok(n)=>n,Err(_)=>vfs::create(path,mode)?};let id=alloc_file_flags(if inode.mode&0o170000==0o020000{OpenKind::CharDevice(inode.rdev)}else if inode.mode&0o170000==0o060000{OpenKind::BlockDevice(inode.rdev)}else{OpenKind::Inode(inode)},status)?;match crate::task::with_current_process(|p|p.fds.install(id)){Some(Ok(fd))=>Ok(fd),Some(Err(e))=>{release(id);Err(e)},None=>{release(id);Err(errno::ESRCH)}}}
pub fn close(fd:usize)->Result<(),i64>{
 /* The descriptor-table close drops one reference; the socket itself is
    freed only when its last reference goes away (via the saved kind).
    Closing it unconditionally here would break processes that still
    share the description after fork (EPIPE/EOF on live peers). */
 crate::task::with_current_process(|p|p.fds.close(fd)).unwrap_or(Err(errno::ESRCH))?;
 Ok(())
}
pub fn read(id:u32,dst:&mut[u8])->Result<usize,i64>{
 let(kind,offset,flags)={let t=OPEN.lock();let f=t[id as usize].ok_or(errno::EBADF)?;(f.kind,f.offset,f.flags)};
 let nonblock=flags&O_NONBLOCK!=0;
 match kind{
  OpenKind::Console=>Ok(0),
  OpenKind::Inode(n)=>{let got=vfs::read(n,offset,dst)?;if got>0{if let Some(f)=OPEN.lock()[id as usize].as_mut(){f.offset+=got as u64}}Ok(got)},
  OpenKind::CharDevice(dev)=>crate::device::read((dev>>8)as u32,(dev&0xff)as u32,dst),
  OpenKind::BlockDevice(dev)=>{let got=crate::device::blk_read((dev>>8)as u32,(dev&0xff)as u32,offset,dst)?; if let Some(f)=OPEN.lock()[id as usize].as_mut(){f.offset+=got as u64;} Ok(got)},
  OpenKind::PipeRead(p)=>crate::pipe::read(p,dst,nonblock),
  OpenKind::Socket(s)=>crate::socket::recv(s,dst,nonblock),
  OpenKind::EventFd(c,sem)=>{
    if dst.len()<8{return Err(errno::EINVAL)}
    let mut t=OPEN.lock();let f=t[id as usize].as_mut().ok_or(errno::EBADF)?;
    if let OpenKind::EventFd(counter,_)=&mut f.kind{
      if *counter==0{return Err(errno::EAGAIN)}
      let val=if sem!=0{1}else{*counter};
      let bytes=val.to_ne_bytes();dst[..8].copy_from_slice(&bytes);
      *counter=counter.saturating_sub(val);
      Ok(8)
    }else{Err(errno::EBADF)}
  },
  OpenKind::Epoll=>Err(errno::EBADF),
  OpenKind::Inotify=>crate::inotify::read(id,dst),
   OpenKind::SignalFd(mask)=>{
     return signalfd_read(mask,dst,id)
   },
  OpenKind::Netlink=>Ok(0),
  OpenKind::IoUring=>Err(crate::errno::EBADF),
  OpenKind::PidFd(_)=>Err(crate::errno::EBADF),
  OpenKind::TimerFd(exp,ivl)=>{
    let mut exp=exp; let mut ivl=ivl;
    let mut now=crate::arch::x86_64::apic::ticks();
    if now<exp{
      crate::task::sleep_until(exp);
      now=crate::arch::x86_64::apic::ticks();
    }
    let mut expirations=1u64;
    if ivl>0 && now>exp{
      expirations=1+(now-exp)/ivl.max(1);
      exp += expirations*ivl.max(1);
    } else if ivl>0 {
      exp = exp.saturating_add(ivl);
    } else {
      exp = u64::MAX;
    }
    // update kind in place
    let mut t=OPEN.lock();
    if let Some(f)=t[id as usize].as_mut(){
      if let OpenKind::TimerFd(e,i)=&mut f.kind{*e=exp;*i=ivl;}
    }
    drop(t);
    let bytes=expirations.to_ne_bytes();
    if dst.len()<8{return Err(errno::EINVAL)}
    dst[..8].copy_from_slice(&bytes);
    Ok(8)
  },
  OpenKind::PipeWrite(_)=>Err(errno::EBADF),
  OpenKind::CharDevice(dev)=>crate::device::read((dev>>8)as u32,(dev&0xff)as u32,dst),
  OpenKind::BlockDevice(dev)=>{let got=crate::device::blk_read((dev>>8)as u32,(dev&0xff)as u32,offset,dst)?; if let Some(f)=OPEN.lock()[id as usize].as_mut(){f.offset+=got as u64;} Ok(got)},
  OpenKind::Inotify=>crate::inotify::read(id,dst),
  OpenKind::SignalFd(mask)=>{
    let pending=crate::task::with_current_process(|p|p.pending).unwrap_or(0);
    let m=pending & mask;
    if m==0{return Err(errno::EAGAIN)}
    let sig=(m.trailing_zeros() as i32)+1;
    let _=crate::task::with_current_process(|p|{p.pending &= !crate::signal::bit(sig);});
    let mut buf=[0u8;128];
    buf[0..4].copy_from_slice(&sig.to_ne_bytes());
    let pid=crate::task::current_pid() as i32;
    buf[12..16].copy_from_slice(&pid.to_ne_bytes());
    if dst.len()<128{return Err(errno::EINVAL)}
    dst[..128].copy_from_slice(&buf);
    Ok(128)
  },
  OpenKind::Netlink=>Ok(0),
  OpenKind::IoUring=>Err(errno::EBADF),
  OpenKind::PidFd(_)=>Err(errno::EBADF),
 }
}
pub fn write(id:u32,src:&[u8])->Result<usize,i64>{
 let(kind,offset,flags)={let t=OPEN.lock();let f=t[id as usize].ok_or(errno::EBADF)?;(f.kind,f.offset,f.flags)};
 let nonblock=flags&O_NONBLOCK!=0;
 match kind{
  OpenKind::Console=>{crate::console::write_bytes(src);Ok(src.len())},
  OpenKind::PipeWrite(p)=>crate::pipe::write(p,src,nonblock),
  OpenKind::Socket(s)=>{crate::socket::send(s,src,nonblock)},
  OpenKind::EventFd(c,_)=>{
    if src.len()<8{return Err(errno::EINVAL)}
    let val=u64::from_ne_bytes(src[..8].try_into().unwrap());
    let mut t=OPEN.lock();let f=t[id as usize].as_mut().ok_or(errno::EBADF)?;
    if let OpenKind::EventFd(counter,_)=&mut f.kind{*counter=counter.saturating_add(val);Ok(8)}else{Err(errno::EBADF)}
  },
  OpenKind::Epoll=>Err(errno::EBADF),
  OpenKind::TimerFd(_,_)=>Err(errno::EBADF),
  OpenKind::Inode(n)=>{
   let start=if flags&O_APPEND!=0{crate::vfs::inode_size(n.ino).unwrap_or(n.size)}else{offset};
   let got=vfs::write(n,start,src)?;
   if let Some(f)=OPEN.lock()[id as usize].as_mut(){f.offset=start+got as u64}
   Ok(got)
  },
  OpenKind::PipeRead(_)=>Err(errno::EBADF),
  OpenKind::CharDevice(dev)=>crate::device::write((dev>>8)as u32,(dev&0xff)as u32,src),
  OpenKind::BlockDevice(dev)=>{let got=crate::device::blk_write((dev>>8)as u32,(dev&0xff)as u32,offset,src)?; if let Some(f)=OPEN.lock()[id as usize].as_mut(){f.offset+=got as u64;} Ok(got)},
  OpenKind::Netlink=>Ok(src.len()),
  OpenKind::IoUring=>Err(errno::EBADF),
  OpenKind::PidFd(_)=>Err(errno::EBADF),
  OpenKind::Inotify=>Err(errno::EBADF),
  OpenKind::SignalFd(_)=>Err(errno::EBADF),
 }
}
pub fn seek(id:u32,off:i64,whence:u64)->Result<u64,i64>{
 let(kind,offset)={let t=OPEN.lock();let f=t[id as usize].ok_or(errno::EBADF)?;(f.kind,f.offset)};
 let size=match kind{OpenKind::Inode(n)=>crate::vfs::inode_size(n.ino).unwrap_or(n.size),OpenKind::Console=>return Err(errno::ESPIPE),OpenKind::PipeRead(_)|OpenKind::PipeWrite(_)|OpenKind::Socket(_)|OpenKind::EventFd(_,_)|OpenKind::Epoll|OpenKind::TimerFd(_,_)|OpenKind::CharDevice(_)|OpenKind::BlockDevice(_)|OpenKind::Inotify|OpenKind::SignalFd(_)|OpenKind::Netlink|OpenKind::IoUring|OpenKind::PidFd(_)=>return Err(errno::ESPIPE)};
 let size=match kind{OpenKind::BlockDevice(_)=>crate::driver::virtio_blk::capacity(),_=>0};
 let base=match whence{0=>0,1=>offset,2=>size,_=>return Err(errno::EINVAL)};let next=if off>=0{base.checked_add(off as u64)}else{base.checked_sub(off.unsigned_abs())}.ok_or(errno::EINVAL)?;
 if let Some(f)=OPEN.lock()[id as usize].as_mut(){f.offset=next}Ok(next)
}
pub fn dup2(oldfd:usize,newfd:usize)->Result<i32,i64>{
 if newfd>=MAX_FDS{return Err(errno::EBADF)}
 crate::task::with_current_process(|p|{let id=p.fds.get(oldfd).ok_or(errno::EBADF)?;if oldfd==newfd{return Ok(newfd as i32)}incref(id);p.fds.replace(newfd,id)?;Ok(newfd as i32)}).unwrap_or(Err(errno::ESRCH))
}
pub fn dup_from(oldfd:usize,min:usize)->Result<i32,i64>{
 if min>=MAX_FDS{return Err(errno::EINVAL)}
 crate::task::with_current_process(|p|{let id=p.fds.get(oldfd).ok_or(errno::EBADF)?;incref(id);match p.fds.install_from(min,id){Ok(fd)=>Ok(fd),Err(e)=>{release(id);Err(e)}}}).unwrap_or(Err(errno::ESRCH))
}
pub fn pipe2(flags:u64)->Result<(i32,i32),i64>{
 const O_CLOEXEC:u64=0x80000;const O_NONBLOCK:u64=0o4000;
 if flags&!(O_CLOEXEC|O_NONBLOCK)!=0{return Err(errno::EINVAL)}
 let status=if flags&O_NONBLOCK!=0{self::O_NONBLOCK}else{0};
 let p=crate::pipe::alloc()?;
 let r=alloc_file_flags(OpenKind::PipeRead(p),status)?;
 let w=match alloc_file_flags(OpenKind::PipeWrite(p),status){Ok(v)=>v,Err(e)=>{release(r);return Err(e)}};
 let installed=crate::task::with_current_process(|proc|{let pair=proc.fds.install_pair(r,w)?;if flags&O_CLOEXEC!=0{let _=proc.fds.set_close_exec(pair.0 as usize,true);let _=proc.fds.set_close_exec(pair.1 as usize,true);}Ok(pair)});
 match installed{Some(Ok(pair))=>Ok(pair),Some(Err(e))=>{release(r);release(w);Err(e)},None=>{release(r);release(w);Err(errno::ESRCH)}}
}

pub fn timerfd_set(id:u32,exp:u64,ivl:u64){
 if let Some(f)=OPEN.lock()[id as usize].as_mut(){
  if let OpenKind::TimerFd(e,i)=&mut f.kind{*e=exp;*i=ivl;}
 }
}
pub fn timerfd_get(id:u32)->Option<(u64,u64)>{
 match kind(id){Some(OpenKind::TimerFd(e,i))=>Some((e,i)),_=>None}
}

pub fn iouring_set(id:u32,sq:u64,cq:u64,sqes:u64,entries:u32){let i=id as usize;if i>=MAX_OPEN{return}unsafe{IOURING_SQ_RING[i]=sq;IOURING_CQ_RING[i]=cq;IOURING_SQES[i]=sqes;IOURING_ENTRIES[i]=entries;IOURING_SQ_HEAD[i]=0;IOURING_CQ_TAIL[i]=0;IOURING_MAP_REFS[i]=0;IOURING_CLOSING[i]=false;}}
pub fn iouring_clear(id:u32){let i=id as usize;if i>=MAX_OPEN{return}unsafe{IOURING_SQ_RING[i]=0;IOURING_CQ_RING[i]=0;IOURING_SQES[i]=0;IOURING_ENTRIES[i]=0;IOURING_SQ_HEAD[i]=0;IOURING_CQ_TAIL[i]=0;IOURING_MAP_REFS[i]=0;IOURING_CLOSING[i]=false;}}
pub fn iouring_sq_ring(id:u32)->u64{let i=id as usize;if i>=MAX_OPEN{return 0}unsafe{IOURING_SQ_RING[i]}}
pub fn iouring_cq_ring(id:u32)->u64{let i=id as usize;if i>=MAX_OPEN{return 0}unsafe{IOURING_CQ_RING[i]}}
pub fn iouring_sqes(id:u32)->u64{let i=id as usize;if i>=MAX_OPEN{return 0}unsafe{IOURING_SQES[i]}}
pub fn iouring_entries(id:u32)->u32{let i=id as usize;if i>=MAX_OPEN{return 0}unsafe{IOURING_ENTRIES[i]}}
pub fn iouring_sq_head(id:u32)->u32{let i=id as usize;if i>=MAX_OPEN{return 0}unsafe{IOURING_SQ_HEAD[i]}}
pub fn iouring_set_sq_head(id:u32,value:u32){let i=id as usize;if i<MAX_OPEN{unsafe{IOURING_SQ_HEAD[i]=value;}}}
pub fn iouring_cq_tail(id:u32)->u32{let i=id as usize;if i>=MAX_OPEN{return 0}unsafe{IOURING_CQ_TAIL[i]}}
pub fn iouring_set_cq_tail(id:u32,value:u32){let i=id as usize;if i<MAX_OPEN{unsafe{IOURING_CQ_TAIL[i]=value;}}}
pub fn iouring_mapping_acquire(id:u32)->Result<(),i64>{let i=id as usize;if i>=MAX_OPEN{return Err(errno::EBADF)}unsafe{if IOURING_ENTRIES[i]==0||IOURING_CLOSING[i]{return Err(errno::EINVAL)}IOURING_MAP_REFS[i]=IOURING_MAP_REFS[i].checked_add(1).ok_or(errno::EOVERFLOW)?;}Ok(())}
pub fn iouring_mapping_release(frame:u64){
 let mut free=[0u64;3];let mut reclaim=false;
 unsafe{for i in 0..MAX_OPEN{if frame!=IOURING_SQ_RING[i]&&frame!=IOURING_CQ_RING[i]&&frame!=IOURING_SQES[i]{continue}if IOURING_MAP_REFS[i]>0{IOURING_MAP_REFS[i]-=1}if IOURING_MAP_REFS[i]==0&&IOURING_CLOSING[i]{free=[IOURING_SQ_RING[i],IOURING_CQ_RING[i],IOURING_SQES[i]];IOURING_SQ_RING[i]=0;IOURING_CQ_RING[i]=0;IOURING_SQES[i]=0;IOURING_ENTRIES[i]=0;IOURING_SQ_HEAD[i]=0;IOURING_CQ_TAIL[i]=0;IOURING_CLOSING[i]=false;reclaim=true}break}}
 if reclaim{for pa in free{if pa!=0{let _=crate::memory::free_frame(pa);}}}
}
fn iouring_close(id:u32){
 let i=id as usize;if i>=MAX_OPEN{return}let frames;unsafe{if IOURING_ENTRIES[i]==0{return}if IOURING_MAP_REFS[i]!=0{IOURING_CLOSING[i]=true;return}frames=[IOURING_SQ_RING[i],IOURING_CQ_RING[i],IOURING_SQES[i]];IOURING_SQ_RING[i]=0;IOURING_CQ_RING[i]=0;IOURING_SQES[i]=0;IOURING_ENTRIES[i]=0;IOURING_SQ_HEAD[i]=0;IOURING_CQ_TAIL[i]=0;IOURING_CLOSING[i]=false;}
 for pa in frames{if pa!=0{let _=crate::memory::free_frame(pa);}}
}
