//! Open-file descriptions and per-process descriptor tables.
//! dup and fork share descriptions, mirroring the Unix file-description model.
use crate::{errno,sync::SpinLock,vfs};
pub const MAX_FDS:usize=32;const MAX_OPEN:usize=64;
#[derive(Clone,Copy)]pub enum OpenKind{Console,Inode(vfs::Inode),PipeRead(u8),PipeWrite(u8),Socket(u8)}
#[derive(Clone,Copy)]struct OpenFile{refs:u32,offset:u64,kind:OpenKind}
static OPEN:SpinLock<[Option<OpenFile>;MAX_OPEN]>=SpinLock::new([const{None};MAX_OPEN]);

const FREE_FD:i32=-1;
pub struct FdTable{slots:[i32;MAX_FDS]}
impl FdTable{
 pub fn stdio()->Result<Self,i64>{let mut t=Self{slots:[FREE_FD;MAX_FDS]};for fd in 0..3{let id=alloc_file(OpenKind::Console)?;t.slots[fd]=id as i32;}Ok(t)}
 pub fn get(&self,fd:usize)->Option<u32>{if fd>=MAX_FDS{return None}if self.slots[fd]<0{None}else{Some(self.slots[fd]as u32)}}
 pub fn install(&mut self,id:u32)->Result<i32,i64>{self.install_from(0,id)}
 pub fn install_from(&mut self,min:usize,id:u32)->Result<i32,i64>{let fd=(min..MAX_FDS).find(|&i|self.slots[i]<0).ok_or(errno::EMFILE)?;self.slots[fd]=id as i32;Ok(fd as i32)}
 pub fn install_pair(&mut self,first:u32,second:u32)->Result<(i32,i32),i64>{let a=(0..MAX_FDS).find(|&i|self.slots[i]<0).ok_or(errno::EMFILE)?;self.slots[a]=first as i32;let b=match (a+1..MAX_FDS).find(|&i|self.slots[i]<0){Some(b)=>b,None=>{self.slots[a]=FREE_FD;return Err(errno::EMFILE)}};self.slots[b]=second as i32;Ok((a as i32,b as i32))}
 pub fn replace(&mut self,fd:usize,id:u32)->Result<(),i64>{if fd>=MAX_FDS{return Err(errno::EBADF)}if self.slots[fd]>=0{release(self.slots[fd]as u32)}self.slots[fd]=id as i32;Ok(())}
 pub fn close(&mut self,fd:usize)->Result<(),i64>{if fd>=MAX_FDS{return Err(errno::EBADF)}let s=self.slots[fd];if s<0{return Err(errno::EBADF)}self.slots[fd]=FREE_FD;release(s as u32);Ok(())}
 pub fn clone_incref(&self)->Self{let mut t=Self{slots:self.slots};for &s in t.slots.iter(){if s>=0{incref(s as u32)}}t}
 pub fn close_all(&mut self){for s in self.slots.iter_mut(){if *s>=0{release(*s as u32);*s=FREE_FD}}}
}
impl Drop for FdTable{fn drop(&mut self){self.close_all()}}

pub fn alloc_file(kind:OpenKind)->Result<u32,i64>{
 let mut t=OPEN.lock();let id=t.iter().position(|s|s.is_none()).ok_or(errno::EMFILE)?;
 t[id]=Some(OpenFile{refs:1,offset:0,kind});drop(t);
 match kind{OpenKind::PipeRead(p)=>crate::pipe::add_reader(p),OpenKind::PipeWrite(p)=>crate::pipe::add_writer(p),_=>{}}
 Ok(id as u32)
}
pub fn incref(id:u32){if let Some(f)=OPEN.lock()[id as usize].as_mut(){f.refs+=1}}
pub fn release(id:u32){let kind={let mut t=OPEN.lock();match t[id as usize].as_mut(){None=>return,Some(f)=>{f.refs-=1;if f.refs>0{return}let k=f.kind;t[id as usize]=None;k}}};match kind{OpenKind::PipeRead(p)=>crate::pipe::close_reader(p),OpenKind::PipeWrite(p)=>crate::pipe::close_writer(p),_=>{}}}
pub fn kind(id:u32)->Option<OpenKind>{OPEN.lock()[id as usize].map(|f|f.kind)}
/* Directory-walk cursor used by getdents64. */
pub fn offset_get(id:u32)->u64{OPEN.lock()[id as usize].map(|f|f.offset).unwrap_or(0)}
/* Positional variants used by pread64/pwrite64; they ignore the file offset. */
pub fn read_at(id:u32,off:u64,dst:&mut[u8])->Result<usize,i64>{
 match OPEN.lock()[id as usize].ok_or(errno::EBADF)?.kind{
  OpenKind::Inode(n)=>vfs::read_off(n,off,dst),
  OpenKind::PipeRead(p)=>crate::pipe::read(p,dst),
  OpenKind::Socket(s)=>crate::socket::recv(s,dst),
  _=>Err(errno::EBADF),
 }
}
pub fn write_at(id:u32,off:u64,src:&[u8])->Result<usize,i64>{
 match OPEN.lock()[id as usize].ok_or(errno::EBADF)?.kind{
  OpenKind::Inode(n)=>vfs::write_off(n,off,src),
  OpenKind::PipeWrite(p)=>crate::pipe::write(p,src),
  OpenKind::Socket(s)=>{crate::socket::send(s,src)},
  _=>Err(errno::EBADF),
 }
}
pub fn offset_set(id:u32,v:u64){if let Some(f)=OPEN.lock()[id as usize].as_mut(){f.offset=v}}

pub fn open(path:&str)->Result<i32,i64>{let inode=vfs::lookup(path)?;let id=alloc_file(OpenKind::Inode(inode))?;match crate::task::with_current_process(|p|p.fds.install(id)){Some(Ok(fd))=>Ok(fd),Some(Err(e))=>{release(id);Err(e)},None=>{release(id);Err(errno::ESRCH)}}}
/* O_CREAT path: creates the node when missing, then opens it. */
pub fn create_open(path:&str,mode:u32)->Result<i32,i64>{let inode=match vfs::lookup(path){Ok(n)=>n,Err(_)=>vfs::create(path,mode)?};let id=alloc_file(OpenKind::Inode(inode))?;match crate::task::with_current_process(|p|p.fds.install(id)){Some(Ok(fd))=>Ok(fd),Some(Err(e))=>{release(id);Err(e)},None=>{release(id);Err(errno::ESRCH)}}}
pub fn close(fd:usize)->Result<(),i64>{
 let kind=crate::task::with_current_process(|p|p.fds.get(fd)).flatten().map(|id|crate::fd::kind(id));
 crate::task::with_current_process(|p|p.fds.close(fd)).unwrap_or(Err(errno::ESRCH))?;
 if let Some(Some(crate::fd::OpenKind::Socket(s)))=kind{crate::socket::close(s);}
 Ok(())
}
pub fn read(id:u32,dst:&mut[u8])->Result<usize,i64>{
 let(kind,offset)={let t=OPEN.lock();let f=t[id as usize].ok_or(errno::EBADF)?;(f.kind,f.offset)};
 match kind{
  OpenKind::Console=>Ok(0),
  OpenKind::Inode(n)=>{let got=vfs::read(n,offset,dst)?;if got>0{if let Some(f)=OPEN.lock()[id as usize].as_mut(){f.offset+=got as u64}}Ok(got)},
  OpenKind::PipeRead(p)=>crate::pipe::read(p,dst),
  OpenKind::Socket(s)=>crate::socket::recv(s,dst),
  OpenKind::PipeWrite(_)=>Err(errno::EBADF),
 }
}
pub fn write(id:u32,src:&[u8])->Result<usize,i64>{
 let(kind,offset)={let t=OPEN.lock();let f=t[id as usize].ok_or(errno::EBADF)?;(f.kind,f.offset)};
 match kind{
  OpenKind::Console=>{crate::console::write_bytes(src);Ok(src.len())},
  OpenKind::PipeWrite(p)=>crate::pipe::write(p,src),
  OpenKind::Socket(s)=>{crate::socket::send(s,src)},
  OpenKind::Inode(n)=>{
   let got=vfs::write(n,offset,src)?;
   if let Some(f)=OPEN.lock()[id as usize].as_mut(){f.offset+=got as u64}
   Ok(got)
  },
  OpenKind::PipeRead(_)=>Err(errno::EBADF),
 }
}
pub fn seek(id:u32,off:i64,whence:u64)->Result<u64,i64>{
 let(kind,offset)={let t=OPEN.lock();let f=t[id as usize].ok_or(errno::EBADF)?;(f.kind,f.offset)};
 let size=match kind{OpenKind::Inode(n)=>n.size,OpenKind::Console=>return Err(errno::ESPIPE),OpenKind::PipeRead(_)|OpenKind::PipeWrite(_)|OpenKind::Socket(_)=>return Err(errno::ESPIPE)};
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
 const O_CLOEXEC:u64=0x80000;
 if flags&!O_CLOEXEC!=0{return Err(errno::EINVAL)}
 let p=crate::pipe::alloc()?;
 let r=alloc_file(OpenKind::PipeRead(p))?;
 let w=match alloc_file(OpenKind::PipeWrite(p)){Ok(v)=>v,Err(e)=>{release(r);return Err(e)}};
 let installed=crate::task::with_current_process(|proc|proc.fds.install_pair(r,w));
 match installed{Some(Ok(pair))=>Ok(pair),Some(Err(e))=>{release(r);release(w);Err(e)},None=>{release(r);release(w);Err(errno::ESRCH)}}
}
