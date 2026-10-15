//! Bootstrap per-process descriptor table for console and initramfs files.
use crate::{errno,sync::SpinLock,vfs};
const MAX_FDS:usize=32;
#[derive(Clone,Copy)]enum Kind{Free,Console,Inode(vfs::Inode)}
#[derive(Clone,Copy)]struct File{kind:Kind,offset:u64}
const FREE:File=File{kind:Kind::Free,offset:0};
static TABLE:SpinLock<[File;MAX_FDS]>=SpinLock::new([FREE;MAX_FDS]);
pub fn init(){let mut t=TABLE.lock();t[0]=File{kind:Kind::Console,offset:0};t[1]=t[0];t[2]=t[0];}
pub fn open(path:&str)->Result<i32,i64>{let inode=vfs::lookup(path)?;let mut t=TABLE.lock();let fd=(3..MAX_FDS).find(|&i|matches!(t[i].kind,Kind::Free)).ok_or(errno::EAGAIN)?;t[fd]=File{kind:Kind::Inode(inode),offset:0};Ok(fd as i32)}
pub fn close(fd:usize)->Result<(),i64>{if fd<3||fd>=MAX_FDS{return Err(errno::EBADF)}let mut t=TABLE.lock();if matches!(t[fd].kind,Kind::Free){return Err(errno::EBADF)}t[fd]=FREE;Ok(())}
pub fn read(fd:usize,dst:&mut[u8])->Result<usize,i64>{if fd>=MAX_FDS{return Err(errno::EBADF)}let mut t=TABLE.lock();match t[fd].kind{Kind::Console=>Ok(0),Kind::Inode(n)=>{let got=vfs::read(n,t[fd].offset,dst)?;t[fd].offset+=got as u64;Ok(got)},Kind::Free=>Err(errno::EBADF)}}
pub fn seek(fd:usize,off:i64,whence:u64)->Result<u64,i64>{if fd>=MAX_FDS{return Err(errno::EBADF)}let mut t=TABLE.lock();let f=t[fd];let size=match f.kind{Kind::Inode(n)=>n.size,Kind::Console=>return Err(errno::ESPIPE),Kind::Free=>return Err(errno::EBADF)};let base=match whence{0=>0,1=>f.offset,2=>size,_=>return Err(errno::EINVAL)};let next=if off>=0{base.checked_add(off as u64)}else{base.checked_sub(off.unsigned_abs())}.ok_or(errno::EINVAL)?;t[fd].offset=next;Ok(next)}
pub fn inode(fd:usize)->Result<vfs::Inode,i64>{if fd>=MAX_FDS{return Err(errno::EBADF)}match TABLE.lock()[fd].kind{Kind::Inode(n)=>Ok(n),_=>Err(errno::EBADF)}}
