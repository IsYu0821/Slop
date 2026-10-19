//! POSIX and open-file-description record locks behind fcntl(2).
//! POSIX locks (F_GETLK/F_SETLK/F_SETLKW) are owned by the process and are
//! all released when any descriptor for the inode closes in that process.
//! OFD locks (F_OFD_*) are owned by the open-file description and survive
//! descriptor closes until the last reference goes away. Ranges are stored
//! half-open [start, end) with u64::MAX standing for end-of-file; a request
//! replaces the owner's overlapping ranges, splitting or trimming as needed.
use crate::{errno,sync::SpinLock,wait_queue::WaitQueue};

pub const F_RDLCK:i16=0;pub const F_WRLCK:i16=1;pub const F_UNLCK:i16=2;
const MAX_LOCKS:usize=64;

#[derive(Clone,Copy,PartialEq,Eq)]pub enum Owner{Pid(u32),Ofd(u32)}
#[derive(Clone,Copy)]struct Entry{used:bool,ino:u64,owner:Owner,exclusive:bool,start:u64,end:u64}
const EMPTY:Entry=Entry{used:false,ino:0,owner:Owner::Pid(0),exclusive:false,start:0,end:0};
static LOCKS:SpinLock<[Entry;MAX_LOCKS]>=SpinLock::new([EMPTY;MAX_LOCKS]);
/* One queue for everyone blocked in F_SETLKW; every release wakes it and the
   waiters re-check, which is conservative but keeps the bookkeeping small. */
static BLOCKED:SpinLock<WaitQueue>=SpinLock::new(WaitQueue::new());

fn overlap(a0:u64,a1:u64,b0:u64,b1:u64)->bool{a0<b1&&b0<a1}

/* First entry owned by someone else that blocks the request. */
fn conflict(ino:u64,owner:Owner,exclusive:bool,start:u64,end:u64)->Option<Entry>{
 for e in LOCKS.lock().iter(){
  if e.used&&e.ino==ino&&e.owner!=owner&&overlap(e.start,e.end,start,end)&&(exclusive||e.exclusive){return Some(*e)}
 }
 None
}

/* Trims the owner's locks so none overlap [start, end); returns whether
   anything changed. A fully covered run is removed; a partially covered one
   is clipped, splitting in two when the request lies strictly inside. */
fn clear_range(ino:u64,owner:Owner,start:u64,end:u64)->bool{
 let mut l=LOCKS.lock();let mut changed=false;let mut extra=-1i32;
 for i in 0..MAX_LOCKS{
  let e=l[i];
  if !e.used||e.ino!=ino||e.owner!=owner||!overlap(e.start,e.end,start,end){continue}
  changed=true;
  let head=e.start<start;
  let tail=e.end>end;
  if head&&tail{
   if extra<0{extra=match (0..MAX_LOCKS).find(|&j|!l[j].used){Some(j)=>j as i32,None=>{l[i]=EMPTY;continue}}}
   l[i]=Entry{used:true,ino,owner,exclusive:e.exclusive,start:e.start,end:start};
   l[extra as usize]=Entry{used:true,ino,owner,exclusive:e.exclusive,start:end,end:e.end};
   extra=-1;
  }else if head{l[i]=Entry{used:true,ino,owner,exclusive:e.exclusive,start:e.start,end:start};}
  else if tail{l[i]=Entry{used:true,ino,owner,exclusive:e.exclusive,start:end,end:e.end};}
  else{l[i]=EMPTY;}
 }
 changed
}

fn place(ino:u64,owner:Owner,exclusive:bool,start:u64,end:u64)->Result<(),i64>{
 if start>=end{return Ok(())}
 let mut l=LOCKS.lock();
 match (0..MAX_LOCKS).find(|&i|!l[i].used){Some(i)=>{l[i]=Entry{used:true,ino,owner,exclusive,start,end};Ok(())},None=>Err(errno::ENOLCK)}
}

fn wake_waiters(){BLOCKED.lock().wake_all();}

/* Applies one F_SETLK-style request; blocking selects F_SETLKW semantics,
   which waits for a conflict to go away and returns EINTR on a signal. */
pub fn set(ino:u64,owner:Owner,exclusive:bool,start:u64,end:u64,blocking:bool)->Result<(),i64>{
 loop{
  if let Some(_)=conflict(ino,owner,exclusive,start,end){
   if !blocking{return Err(errno::EAGAIN)}
   if crate::task::has_deliverable_signal(){return Err(errno::EINTR)}
   if !BLOCKED.lock().register().is_ok(){return Err(errno::EAGAIN)}
   crate::task::block_current();
   if crate::task::has_deliverable_signal(){return Err(errno::EINTR)}
   continue;
  }
  clear_range(ino,owner,start,end);
  place(ino,owner,exclusive,start,end)?;
  wake_waiters();
  return Ok(());
 }
}

/* Removes the owner's locks over [start, end) (F_UNLCK). */
pub fn unlock(ino:u64,owner:Owner,start:u64,end:u64)->Result<(),i64>{
 if clear_range(ino,owner,start,end){wake_waiters();}
 Ok(())
}

/* Describes the first blocking lock for F_GETLK output:
   (owner pid, exclusive, start, end); OFD-owned locks report pid 0. */
pub fn query(ino:u64,owner:Owner,exclusive:bool,start:u64,end:u64)->Option<(u32,bool,u64,u64)>{
 conflict(ino,owner,exclusive,start,end).map(|e|{
  let pid=match e.owner{Owner::Pid(p)=>p,Owner::Ofd(_)=>0};
  (pid,e.exclusive,e.start,e.end)
 })
}

/* Any close(2) of an inode drops the process's POSIX locks on it. */
pub fn on_close(pid:u32,ino:u64){
 if clear_range_owner(ino,Owner::Pid(pid)){wake_waiters();}
}
fn clear_range_owner(ino:u64,owner:Owner)->bool{
 let mut l=LOCKS.lock();let mut changed=false;
 for i in 0..MAX_LOCKS{
  let e=l[i];
  if e.used&&e.ino==ino&&e.owner==owner{l[i]=EMPTY;changed=true;}
 }
 changed
}

/* Last descriptor of an open-file description went away. */
pub fn release_ofd(id:u32){
 if clear_range_owner_any(Owner::Ofd(id)){wake_waiters();}
}
fn clear_range_owner_any(owner:Owner)->bool{
 let mut l=LOCKS.lock();let mut changed=false;
 for i in 0..MAX_LOCKS{
  let e=l[i];
  if e.used&&e.owner==owner{l[i]=EMPTY;changed=true;}
 }
 changed
}

/* Process exit: drop every lock the process owned. */
pub fn release_pid(pid:u32){
 if clear_range_owner_any(Owner::Pid(pid)){wake_waiters();}
}

pub fn resource_stats()->usize{LOCKS.lock().iter().filter(|e|e.used).count()}
