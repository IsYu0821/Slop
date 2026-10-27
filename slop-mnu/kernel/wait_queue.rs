//! Fixed-capacity wait queues backed by scheduler task slots.
use crate::sync::IrqSpinLock;
const MAX_WAITERS:usize=8;
#[derive(Clone,Copy)]struct Waiter{pid:u32,used:bool}
const EMPTY:Waiter=Waiter{pid:0,used:false};
pub struct WaitQueue{waiters:IrqSpinLock<[Waiter;MAX_WAITERS]>}
impl WaitQueue{
 pub const fn new()->Self{Self{waiters:IrqSpinLock::new([EMPTY;MAX_WAITERS])}}
 /** Enlists the current task without blocking. Callers that observe the wait
     condition under their own lock use this, then drop it and call wait(). */
 pub fn register(&self)->Result<(),i64>{let pid=crate::task::current_pid();let mut q=self.waiters.lock();let slot=q.iter_mut().find(|w|!w.used).ok_or(crate::errno::EAGAIN)?;*slot=Waiter{pid,used:true};Ok(())}
 pub fn wait(&self){crate::task::block_current()}
 pub fn sleep(&self)->Result<(),i64>{self.register()?;self.wait();Ok(())}
 pub fn wake_one(&self)->bool{let pid={let mut q=self.waiters.lock();match q.iter_mut().find(|w|w.used){Some(w)=>{w.used=false;w.pid},None=>return false}};crate::task::wake_pid(pid)}
 pub fn wake_all(&self)->usize{let mut n=0;while self.wake_one(){n+=1}n}
 /* Drops every waiter without waking: pool reuse must not inherit stale
    registrations, which would wake unrelated tasks that recycled the pid. */
 pub fn clear(&self){let mut q=self.waiters.lock();for w in q.iter_mut(){*w=EMPTY}}
}
