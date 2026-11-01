//! Minimal futex: FUTEX_WAIT/WAKE plus the BITSET pair modern glibc
//! condition variables require (WAIT_BITSET/WAKE_BITSET, with absolute
//! timeouts and CLOCK_REALTIME). REQUEUE and PI opcodes are rejected. A
//! fixed waiter table keyed by user address backs the supported operations;
//! Linux wake/wait semantics for those ops are preserved, including the
//! EAGAIN value race check.
use crate::{errno,sync::SpinLock,task};
pub const FUTEX_WAIT:u32=0;pub const FUTEX_WAKE:u32=1;
pub const FUTEX_WAIT_BITSET:u32=9;pub const FUTEX_WAKE_BITSET:u32=10;
pub const FUTEX_PRIVATE_FLAG:u32=0x80;pub const FUTEX_CLOCK_REALTIME:u32=0x100;
pub const FUTEX_BITSET_MATCH_ANY:u32=0xffff_ffff;
const MAX_WAITERS:usize=64;
struct Waiter{used:bool,uaddr:u64,pid:u32,bitset:u32}
const EMPTY:Waiter=Waiter{used:false,uaddr:0,pid:0,bitset:0};
static WAITERS:SpinLock<[Waiter;MAX_WAITERS]>=SpinLock::new([EMPTY;MAX_WAITERS]);
/* Reads the u32 the kernel compares against the caller-supplied value. */
fn peek(addr:u64)->Result<u32,i64>{
 let mut b=[0u8;4];
 if let Err(e)=unsafe{crate::usercopy::copy_from_user(&mut b,addr as usize)}{return Err(e)}
 Ok(u32::from_ne_bytes(b))
}
/* Blocks until woken or the deadline runs out; deadline_ns==u64::MAX waits
   forever. Sleeps are relative; callers convert absolute futex timeouts
   (WAIT_BITSET) to a deadline before blocking. */
fn block_until(deadline_ns:u64,now:u64)->Result<(),i64>{
 if deadline_ns==u64::MAX{
  task::block_current();
  return Ok(())
 }
 if deadline_ns<=now{return Err(errno::ETIMEDOUT)}
 task::sleep_until(crate::time::ticks_after_ns(deadline_ns.saturating_sub(now)));
 Ok(())
}
fn now_ns(realtime:bool)->u64{if realtime{crate::time::realtime_ns()}else{crate::time::monotonic_ns()}}
/* Reads an absolute timeout timespec (WAIT_BITSET) into nanoseconds. */
fn read_abstime(timeout:u64)->Result<u64,i64>{
 let mut ts=[0u8;16];
 if let Err(e)=unsafe{crate::usercopy::copy_from_user(&mut ts,timeout as usize)}{return Err(e)}
 let sec=u64::from_ne_bytes(ts[0..8].try_into().unwrap());
 let nsec=u64::from_ne_bytes(ts[8..16].try_into().unwrap());
 if nsec>=1_000_000_000{return Err(errno::EINVAL)}
 if sec>u32::MAX as u64{return Err(errno::EINVAL)}
 Ok(sec.saturating_mul(1_000_000_000).saturating_add(nsec))
}
pub fn dispatch(uaddr:u64,op:u32,val:u32,timeout:u64,val3:u32)->Result<i64,i64>{
 if uaddr==0||uaddr%4!=0{return Err(errno::EFAULT)}
 match op&!(FUTEX_PRIVATE_FLAG|FUTEX_CLOCK_REALTIME){
  FUTEX_WAIT=>wait(uaddr,val,timeout,op,FUTEX_BITSET_MATCH_ANY),
  FUTEX_WAKE=>wake(uaddr,val,FUTEX_BITSET_MATCH_ANY),
  FUTEX_WAIT_BITSET=>{
   if val3==0{return Err(errno::EINVAL)}
   wait(uaddr,val,timeout,op,val3)
  },
  FUTEX_WAKE_BITSET=>{
   if val3==0{return Err(errno::EINVAL)}
   wake(uaddr,val,val3)
  },
  _=>Err(errno::ENOSYS),
 }
}
/* timeout==0 means infinite for both WAIT (relative) and WAIT_BITSET
   (absolute NULL). WAIT takes a relative timeout; WAIT_BITSET takes an
   absolute timeout in the clock selected by FUTEX_CLOCK_REALTIME. */
fn wait(uaddr:u64,val:u32,timeout:u64,op:u32,bitset:u32)->Result<i64,i64>{
 let bitset_wait=op&!(FUTEX_PRIVATE_FLAG|FUTEX_CLOCK_REALTIME)==FUTEX_WAIT_BITSET;
 let realtime=op&FUTEX_CLOCK_REALTIME!=0;
 let deadline=if timeout==0{u64::MAX}else if bitset_wait{read_abstime(timeout)?}else{
  let mut ts=[0u8;16];
  if let Err(e)=unsafe{crate::usercopy::copy_from_user(&mut ts,timeout as usize)}{return Err(e)}
  let sec=u64::from_ne_bytes(ts[0..8].try_into().unwrap());
  let nsec=u64::from_ne_bytes(ts[8..16].try_into().unwrap());
  if nsec>=1_000_000_000{return Err(errno::EINVAL)}
  if sec>u32::MAX as u64{return Err(errno::EINVAL)}
  now_ns(false).saturating_add(sec.saturating_mul(1_000_000_000).saturating_add(nsec))
 };
 let pid=task::current_pid();
 loop{
  /* The word is read and the waiter is registered under the same lock the
     waker uses to take the waiter out of the table. A wakeup that comes
     between the two would otherwise be lost: the waker would find no
     waiter, and the waiter would block after the word had changed. */
  {let mut w=WAITERS.lock();
   if peek(uaddr)?!=val{return Err(errno::EAGAIN)}
   if w.iter().any(|e|e.used&&e.uaddr==uaddr&&e.pid==pid){return Err(errno::EAGAIN)}
   match w.iter().position(|e|!e.used){Some(i)=>{w[i]=Waiter{used:true,uaddr,pid,bitset}},None=>return Err(errno::ENOMEM)}};
  let res=block_until(deadline,now_ns(realtime));
  /* wake() removes our entry; find it by identity rather than slot index,
     since the slot may have been reused by another waiter meanwhile. */
  let still_registered={let mut w=WAITERS.lock();
   match w.iter().position(|e|e.used&&e.uaddr==uaddr&&e.pid==pid){Some(i)=>{w[i]=EMPTY;true},None=>false}};
  if !still_registered{return Ok(0)}
  match res{Ok(())=>continue,Err(e)=>return Err(e)}
 }
}
pub fn wake(uaddr:u64,count:u32,bitset:u32)->Result<i64,i64>{
 let mut woken=0i64;
 let targets:([u32;MAX_WAITERS],usize)={
  let mut w=WAITERS.lock();
  let mut t=[0u32;MAX_WAITERS];let mut n=0;
  for e in w.iter_mut(){
   if e.used&&e.uaddr==uaddr&&(e.bitset&bitset)!=0&&n<count as usize{t[n]=e.pid;e.used=false;n+=1}
  }
  (t,n)
 };
 for &pid in targets.0.iter().take(targets.1){
  task::wake_pid(pid);
  woken+=1;
 }
 Ok(woken)
}
