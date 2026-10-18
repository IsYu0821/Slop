//! Minimal futex: FUTEX_WAIT, FUTEX_WAKE and the PRIVATE/CROSS_PROCESS,
//! REQUEUE and PI opcodes are rejected. A fixed waiter table keyed by user
//! address backs the two supported operations; Linux wake/wait semantics for
//! those ops are preserved, including the EAGAIN value race check.
use crate::{errno,sync::SpinLock,task};
pub const FUTEX_WAIT:u32=0;pub const FUTEX_WAKE:u32=1;
pub const FUTEX_PRIVATE_FLAG:u32=0x80;
const MAX_WAITERS:usize=64;
struct Waiter{used:bool,uaddr:u64,pid:u32}
const EMPTY:Waiter=Waiter{used:false,uaddr:0,pid:0};
static WAITERS:SpinLock<[Waiter;MAX_WAITERS]>=SpinLock::new([EMPTY;MAX_WAITERS]);
/* Reads the u32 the kernel compares against the caller-supplied value. */
fn peek(addr:u64)->Result<u32,i64>{
 let mut b=[0u8;4];
 if let Err(e)=unsafe{crate::usercopy::copy_from_user(&mut b,addr as usize)}{return Err(e)}
 Ok(u32::from_ne_bytes(b))
}
/* Blocks until woken or the deadline elapses; deadline_ns==u64::MAX waits forever. */
fn block_until(deadline_ns:u64)->Result<(),i64>{
 if deadline_ns==u64::MAX{
  task::block_current();
  return Ok(())
 }
 let now=crate::time::monotonic_ns();
 if now>=deadline_ns{return Err(errno::ETIMEDOUT)}
 task::sleep_until(crate::time::ticks_after_ns(deadline_ns-now));
 Ok(())
}
pub fn dispatch(uaddr:u64,op:u32,val:u32,timeout:u64)->Result<i64,i64>{
 if uaddr==0||uaddr%4!=0{return Err(errno::EFAULT)}
 match op&!FUTEX_PRIVATE_FLAG{
  FUTEX_WAIT=>wait(uaddr,val,timeout,op),
  FUTEX_WAKE=>wake(uaddr,val),
  _=>Err(errno::ENOSYS),
 }
}
fn wait(uaddr:u64,val:u32,timeout:u64,op:u32)->Result<i64,i64>{
 if peek(uaddr)?!=val{return Err(errno::EAGAIN)}
 let deadline=if timeout==0{u64::MAX}else{
  let mut ts=[0u8;16];
  if let Err(e)=unsafe{crate::usercopy::copy_from_user(&mut ts,timeout as usize)}{return Err(e)}
  let sec=u64::from_ne_bytes(ts[0..8].try_into().unwrap());
  let nsec=u64::from_ne_bytes(ts[8..16].try_into().unwrap());
  if sec>u32::MAX as u64{return Err(errno::EINVAL)}
  sec.saturating_mul(1_000_000_000).saturating_add(nsec)
 };
 let pid=task::current_pid();
 loop{
  let slot={let mut w=WAITERS.lock();
   if w.iter().any(|e|e.used&&e.uaddr==uaddr&&e.pid==pid){return Err(errno::EAGAIN)}
   match w.iter().position(|e|!e.used){Some(i)=>{w[i]=Waiter{used:true,uaddr,pid};i},None=>return Err(errno::ENOMEM)}};
  let res=block_until(deadline);
  /* wake() clears the slot, so a cleared slot means we were woken. */
  let woken={let mut w=WAITERS.lock();let held=w[slot].used;w[slot]=EMPTY;!held};
  if woken{return Ok(0)}
  match res{Ok(())=>continue,Err(e)=>return Err(e)}
 }
}
fn wake(uaddr:u64,count:u32)->Result<i64,i64>{
 let mut woken=0i64;
 let targets:([u32;MAX_WAITERS],usize)={
  let mut w=WAITERS.lock();
  let mut t=[0u32;MAX_WAITERS];let mut n=0;
  for e in w.iter_mut(){
   if e.used&&e.uaddr==uaddr&&n<count as usize{t[n]=e.pid;e.used=false;n+=1}
  }
  (t,n)
 };
 for &pid in targets.0.iter().take(targets.1){
  task::wake_pid(pid);
  woken+=1;
 }
 Ok(woken)
}
