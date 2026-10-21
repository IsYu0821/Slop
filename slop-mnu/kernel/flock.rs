//! flock(2) advisory locks keyed by the open-file description.
//! Locks live on the inode but are owned by the description, so dup and fork
//! share a lock while an independent open(2) contends. Locks are dropped when
//! the last descriptor referencing the description closes.
use crate::{errno,sync::SpinLock,wait_queue::WaitQueue};
const MAX_LOCKS:usize=32;
pub const LOCK_SH:u64=1;
pub const LOCK_EX:u64=2;
pub const LOCK_NB:u64=4;
pub const LOCK_UN:u64=8;
const OP_MASK:u64=LOCK_SH|LOCK_EX|LOCK_NB|LOCK_UN;
struct Lock{used:bool,ino:u64,id:u32,exclusive:bool,q:WaitQueue}
const EMPTY:Lock=Lock{used:false,ino:0,id:0,exclusive:false,q:WaitQueue::new()};
static LOCKS:[SpinLock<Lock>;MAX_LOCKS]=[const{SpinLock::new(EMPTY)};MAX_LOCKS];

/* Finds the first entry from another description that blocks this request. */
fn conflict(ino:u64,id:u32,exclusive:bool)->Option<usize>{
 for i in 0..MAX_LOCKS{
  let l=LOCKS[i].lock();
  if l.used&&l.id!=id&&l.ino==ino&&(exclusive||l.exclusive){return Some(i)}
 }
 None
}
fn place(ino:u64,id:u32,exclusive:bool)->Result<(),i64>{
 for i in 0..MAX_LOCKS{
  let mut l=LOCKS[i].lock();
  if !l.used{l.used=true;l.ino=ino;l.id=id;l.exclusive=exclusive;return Ok(())}
 }
 Err(errno::ENOMEM)
}
pub fn flock(id:u32,ino:u64,op:u64)->Result<(),i64>{
 if op&!OP_MASK!=0{return Err(errno::EINVAL)}
 if op&LOCK_UN!=0{release(id);return Ok(())}
 if op&(LOCK_SH|LOCK_EX)==0{return Err(errno::EINVAL)}
 let exclusive=op&LOCK_EX!=0;
 let nonblock=op&LOCK_NB!=0;
 loop{
  /* Re-requesting replaces any lock this description already holds. */
  release(id);
  match conflict(ino,id,exclusive){
   None=>return place(ino,id,exclusive),
   Some(idx)=>{
    if nonblock{return Err(errno::EAGAIN)}
    let registered={let l=LOCKS[idx].lock();l.q.register().is_ok()};
    if !registered{return Err(errno::EAGAIN)}
    crate::task::block_current();
   }
  }
 }
}
pub fn release(id:u32){
 for i in 0..MAX_LOCKS{
  let mut l=LOCKS[i].lock();
  if l.used&&l.id==id{l.used=false;l.id=0;l.q.wake_all();}
 }
}
