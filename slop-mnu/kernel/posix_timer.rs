//! POSIX interval timers (timer_create family) and legacy alarm/setitimer.
//! Signals follow the process-wide bitmask model: expiry marks the signal
//! pending on the owner and wakes it; si_value/si_overrun are stored on the
//! owning process for rt_sigtimedwait to report.
use core::sync::atomic::{AtomicU32,Ordering};
use crate::sync::SpinLock;
use crate::errno;

const MAX_TIMERS:usize=32;
const MAX_ALARMS:usize=32;

#[derive(Clone,Copy)]
pub enum Notify{None,Signal{sig:i32,value:u64}}

#[derive(Clone,Copy)]
struct Timer{used:bool,id:u32,owner:u32,clock:i32,notify:Notify,active:bool,next:u64,interval:u64,overrun:u32}
const EMPTY_T:Timer=Timer{used:false,id:0,owner:0,clock:0,notify:Notify::None,active:false,next:0,interval:0,overrun:0};
static TIMERS:SpinLock<[Timer;MAX_TIMERS]>=SpinLock::new([EMPTY_T;MAX_TIMERS]);
static NEXT_ID:AtomicU32=AtomicU32::new(1);

#[derive(Clone,Copy)]
/* which: 0 ITIMER_REAL (next is an absolute tick), 1 ITIMER_VIRTUAL and
   2 ITIMER_PROF count down user (and kernel) ticks attributed to the pid. */
struct Alarm{used:bool,pid:u32,which:u8,active:bool,next:u64,interval:u64}
const EMPTY_A:Alarm=Alarm{used:false,pid:0,which:0,active:false,next:0,interval:0};
static ALARMS:SpinLock<[Alarm;MAX_ALARMS]>=SpinLock::new([EMPTY_A;MAX_ALARMS]);

/* CLOCK_REALTIME, CLOCK_MONOTONIC and CLOCK_BOOTTIME are distinguishable in
   Slop/MNU; the coarse/CPU clocks are rejected where Linux would reject them. */
pub fn valid_clock(clock:i32)->bool{matches!(clock,0|1|7)}

pub fn create(owner:u32,clock:i32,notify:Notify)->Result<u32,i64>{
 if !valid_clock(clock){return Err(errno::EINVAL)}
 let mut t=TIMERS.lock();
 let slot=t.iter().position(|e|!e.used).ok_or(errno::EAGAIN)?;
 let id=NEXT_ID.fetch_add(1,Ordering::Relaxed);
 t[slot]=Timer{used:true,id,owner,clock,notify,active:false,next:0,interval:0,overrun:0};
 Ok(id)
}

fn find<'a>(t:&'a mut [Timer;MAX_TIMERS],id:u32,owner:u32)->Result<&'a mut Timer,i64>{
 t.iter_mut().find(|e|e.used&&e.id==id&&e.owner==owner).ok_or(errno::EINVAL)
}

pub fn settime(id:u32,owner:u32,active:bool,next:u64,interval:u64)->Result<(),i64>{
 let mut t=TIMERS.lock();
 let e=find(&mut t,id,owner)?;
 e.active=active;e.next=next;e.interval=interval;e.overrun=0;
 Ok(())
}

/* Remaining ticks (0 when disarmed) and the interval. */
pub fn gettime(id:u32,owner:u32,now:u64)->Result<(u64,u64),i64>{
 let mut t=TIMERS.lock();
 let e=find(&mut t,id,owner)?;
 Ok((if e.active{e.next.saturating_sub(now)}else{0},e.interval))
}

pub fn getoverrun(id:u32,owner:u32)->Result<u32,i64>{
 let mut t=TIMERS.lock();
 let e=find(&mut t,id,owner)?;
 Ok(e.overrun)
}

pub fn delete(id:u32,owner:u32)->Result<(),i64>{
 let mut t=TIMERS.lock();
 let e=find(&mut t,id,owner)?;
 *e=EMPTY_T;
 Ok(())
}

/* Removes every POSIX timer owned by pid (process exit, execve). */
pub fn release_pid(pid:u32){
 let mut t=TIMERS.lock();
 for e in t.iter_mut(){if e.used&&e.owner==pid{*e=EMPTY_T;}}
 drop(t);
 let mut a=ALARMS.lock();
 for e in a.iter_mut(){if e.used&&e.pid==pid{*e=EMPTY_A;}}
}

/* Sets or cancels one of the three per-process interval timers; returns the
   previous (remaining, interval) in ticks. A missing entry is created only
   when the new timer is armed. */
pub fn alarm_set(pid:u32,which:u8,active:bool,next:u64,interval:u64,now:u64)->Result<(u64,u64),i64>{
 let mut a=ALARMS.lock();
 if let Some(e)=a.iter_mut().find(|e|e.used&&e.pid==pid&&e.which==which){
  let old=(if e.active{if which==0{e.next.saturating_sub(now)}else{e.next}}else{0},e.interval);
  if active{e.active=true;e.next=next;e.interval=interval}else{e.active=false;e.next=0;e.interval=0;}
  return Ok(old)
 }
 if !active{return Ok((0,0))}
 let slot=a.iter().position(|e|!e.used).ok_or(errno::EAGAIN)?;
 a[slot]=Alarm{used:true,pid,which,active:true,next,interval};
 Ok((0,0))
}

pub fn alarm_get(pid:u32,which:u8,now:u64)->(u64,u64){
 let a=ALARMS.lock();
 if let Some(e)=a.iter().find(|e|e.used&&e.pid==pid&&e.which==which){
  if e.active{return(if which==0{e.next.saturating_sub(now)}else{e.next},e.interval)}
 }
 (0,0)
}

/* CPU-tick hook for ITIMER_VIRTUAL (user time only) and ITIMER_PROF (user
   plus kernel time). Fires at most one coalesced signal of each kind. */
pub fn cpu_tick(pid:u32,user:bool){
 let mut vfire=false;let mut pfire=false;
 {
  let mut a=ALARMS.lock();
  for e in a.iter_mut(){
   if !e.used||!e.active||e.pid!=pid||e.which==0{continue}
   if e.which==1&&!user{continue}
   e.next=e.next.saturating_sub(1);
   if e.next==0{
    if e.interval>0{e.next=e.interval}else{e.active=false}
    if e.which==1{vfire=true}else{pfire=true}
   }
  }
 }
 if vfire{let _=crate::task::timer_signal(pid,crate::signal::SIGVTALRM,0,0);}
 if pfire{let _=crate::task::timer_signal(pid,crate::signal::SIGPROF,0,0);}
}

/* Tick hook: advances expired timers and queues their signals after releasing
   the timer table lock (signal delivery takes the scheduler locks). */
pub fn tick(now:u64){
 let mut events:[(u32,u32,i32,u64,u32);MAX_TIMERS]=[(0,0,0,0,0);MAX_TIMERS];
 let mut n=0;
 {
  let mut t=TIMERS.lock();
  for e in t.iter_mut(){
   if !e.used||!e.active||e.next>now{continue}
   let expirations=if e.interval>0{1+(now-e.next)/e.interval}else{1};
   if e.interval>0{e.next=e.next.saturating_add(expirations*e.interval)}else{e.active=false;e.next=0;}
   if let Notify::Signal{sig,value}=e.notify{
    if n<events.len(){events[n]=(e.id,e.owner,sig,value,(expirations-1)as u32);n+=1}
   }
  }
 }
 let mut a=ALARMS.lock();
 let mut alarm_pids:[u32;MAX_ALARMS]=[0;MAX_ALARMS];
 let mut an=0;
 for e in a.iter_mut(){
  if !e.used||!e.active||e.which!=0||e.next>now{continue}
  let expirations=if e.interval>0{1+(now-e.next)/e.interval}else{1};
  if e.interval>0{e.next=e.next.saturating_add(expirations*e.interval)}else{e.active=false;e.next=0;}
  if an<alarm_pids.len(){alarm_pids[an]=e.pid;an+=1}
 }
 drop(a);
 for i in 0..n{
  let (id,owner,sig,value,extra)=events[i];
  let queued=crate::task::timer_signal(owner,sig,value,extra);
  let mut t=TIMERS.lock();
  if let Some(e)=t.iter_mut().find(|e|e.used&&e.id==id&&e.owner==owner){
   if queued{e.overrun=extra}else{e.overrun=e.overrun.saturating_add(extra+1)}
  }
 }
 for i in 0..an{
  let _=crate::task::timer_signal(alarm_pids[i],crate::signal::SIGALRM,0,0);
 }
}
