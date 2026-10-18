//! Single-CPU round-robin scheduler, task lifecycle and process ownership.
use core::sync::atomic::{AtomicBool,AtomicU32,Ordering};
use crate::process::Process;
use crate::sync::IrqSpinLock;
const MAX_TASKS:usize=8;const STACK_SIZE:usize=32768;
#[repr(align(16))]struct Stack([u8;STACK_SIZE]);
#[derive(Clone,Copy,PartialEq)]pub enum State{Unused,Running,Runnable,Sleeping,Zombie}
#[derive(Clone,Copy)]pub struct Task{pub pid:u32,pub ppid:u32,pub pgid:u32,pub state:State,pub rsp:u64,pub kernel_stack_top:u64,pub cr3:u64,pub fs_base:u64,pub wake_tick:u64,pub wake_pending:bool,pub exit_code:i32,pub term_signal:u8}
const EMPTY:Task=Task{pid:0,ppid:0,pgid:0,state:State::Unused,rsp:0,kernel_stack_top:0,cr3:0,fs_base:0,wake_tick:0,wake_pending:false,exit_code:0,term_signal:0};
struct Scheduler{tasks:[Task;MAX_TASKS],current:usize,next_pid:u32}
static SCHED:IrqSpinLock<Scheduler>=IrqSpinLock::new(Scheduler{tasks:[EMPTY;MAX_TASKS],current:0,next_pid:2});
/* One Process per user task slot; kernel-only tasks leave their entry empty. */
static PROCS:IrqSpinLock<[Option<Process>;MAX_TASKS]>=IrqSpinLock::new([const{None};MAX_TASKS]);
static mut STACKS:[Stack;MAX_TASKS]=[const{Stack([0;STACK_SIZE])};MAX_TASKS];
static NEED_RESCHED:AtomicBool=AtomicBool::new(false);static PID:AtomicU32=AtomicU32::new(1);
unsafe extern "sysv64"{fn switch_context(old:*mut u64,new:u64);}
pub fn init(bootstrap_stack_top:u64){let mut s=SCHED.lock();let cr3=crate::arch::x86_64::paging::PageTableRoot::current().pml4;s.tasks[0]=Task{pid:1,ppid:0,pgid:1,state:State::Running,rsp:0,kernel_stack_top:bootstrap_stack_top,cr3,fs_base:0,wake_tick:0,wake_pending:false,exit_code:0,term_signal:0};crate::println!("task: scheduler bootstrap pid=1 capacity={}",MAX_TASKS);}
pub fn current_pid()->u32{PID.load(Ordering::Relaxed)}
pub fn current_ppid()->u32{let s=SCHED.lock();s.tasks[s.current].ppid}
pub fn sched_pids()->[u32;8]{let s=SCHED.lock();let mut p=[0;8];for i in 0..8{p[i]=s.tasks[i].pid};p}
pub fn current_kernel_stack_top()->u64{let s=SCHED.lock();s.tasks[s.current].kernel_stack_top}
/* Locks are never nested the other way around: the scheduler guard is released
   before PROCS is taken, so closure bodies may lock anything but SCHED. */
pub fn with_current_process<R>(f:impl FnOnce(&mut Process)->R)->Option<R>{let slot=SCHED.lock().current;let mut procs=PROCS.lock();procs[slot].as_mut().map(f)}
pub fn has_current_process()->bool{let slot=SCHED.lock().current;PROCS.lock()[slot].is_some()}
fn take_slot(s:&mut Scheduler)->Result<usize,i64>{(1..MAX_TASKS).find(|&i|s.tasks[i].state==State::Unused).ok_or(crate::errno::EAGAIN)}
fn alloc_pid(s:&mut Scheduler)->u32{let pid=s.next_pid;s.next_pid=s.next_pid.wrapping_add(1).max(2);pid}
fn build_kernel_stack(slot:usize,entry:u64)->u64{let top=unsafe{core::ptr::addr_of!(STACKS[slot].0)as u64+STACK_SIZE as u64};let mut sp=(top&!15)-8;unsafe{sp-=8;*(sp as*mut u64)=entry;for _ in 0..6{sp-=8;*(sp as*mut u64)=0;}}sp}
pub fn spawn_kernel(entry:fn())->Result<u32,i64>{let mut s=SCHED.lock();let slot=take_slot(&mut s)?;let pid=alloc_pid(&mut s);let top=unsafe{core::ptr::addr_of!(STACKS[slot].0)as u64+STACK_SIZE as u64};let sp=build_kernel_stack(slot,kernel_thread_trampoline as *const () as u64);KERNEL_ENTRIES.lock()[slot]=Some(entry);let cr3=crate::arch::x86_64::paging::PageTableRoot::current().pml4;let ppid=s.tasks[s.current].pid;let pgid=s.tasks[s.current].pgid;s.tasks[slot]=Task{pid,ppid,pgid,state:State::Runnable,rsp:sp,kernel_stack_top:top,cr3,fs_base:0,wake_tick:0,wake_pending:false,exit_code:0,term_signal:0};Ok(pid)}
static KERNEL_ENTRIES:IrqSpinLock<[Option<fn()>;MAX_TASKS]>=IrqSpinLock::new([None;MAX_TASKS]);
static USER_STARTS:IrqSpinLock<[Option<crate::exec_runtime::Loaded>;MAX_TASKS]>=IrqSpinLock::new([None;MAX_TASKS]);
static FORK_RESUMES:IrqSpinLock<[Option<crate::arch::x86_64::syscall::UserFrame>;MAX_TASKS]>=IrqSpinLock::new([const{None};MAX_TASKS]);
pub fn spawn_user(loaded:crate::exec_runtime::Loaded)->Result<u32,i64>{let proc=Process::new(loaded.cr3,loaded.bootstrap_cr3,loaded.image_slot,loaded.stack_slot)?;let mut s=SCHED.lock();let slot=take_slot(&mut s)?;let pid=alloc_pid(&mut s);let ppid=s.tasks[s.current].pid;let pgid=s.tasks[s.current].pgid;let top=unsafe{core::ptr::addr_of!(STACKS[slot].0)as u64+STACK_SIZE as u64};let sp=build_kernel_stack(slot,user_thread_trampoline as *const () as u64);PROCS.lock()[slot]=Some(proc);USER_STARTS.lock()[slot]=Some(loaded);s.tasks[slot]=Task{pid,ppid,pgid,state:State::Runnable,rsp:sp,kernel_stack_top:top,cr3:loaded.cr3,fs_base:0,wake_tick:0,wake_pending:false,exit_code:0,term_signal:0};Ok(pid)}
pub fn spawn_fork(mut child:Process,frame:crate::arch::x86_64::syscall::UserFrame)->Result<u32,i64>{let child_cr3=child.cr3;let mut s=SCHED.lock();let slot=match take_slot(&mut s){Ok(v)=>v,Err(e)=>{drop(s);child.abandon();return Err(e)}};let pid=alloc_pid(&mut s);let parent=s.current;let ppid=s.tasks[parent].pid;let pgid=s.tasks[parent].pgid;let fs_base=s.tasks[parent].fs_base;let top=unsafe{core::ptr::addr_of!(STACKS[slot].0)as u64+STACK_SIZE as u64};let sp=build_kernel_stack(slot,fork_thread_trampoline as *const () as u64);FORK_RESUMES.lock()[slot]=Some(frame);PROCS.lock()[slot]=Some(child);s.tasks[slot]=Task{pid,ppid,pgid,state:State::Runnable,rsp:sp,kernel_stack_top:top,cr3:child_cr3,fs_base,wake_tick:0,wake_pending:false,exit_code:0,term_signal:0};Ok(pid)}
pub fn fork_process(frame:crate::arch::x86_64::syscall::UserFrame)->Result<u32,i64>{let child_cr3=match with_current_process(|p|p.clone_address_space()){Some(Ok(v))=>v,Some(Err(e))=>return Err(e),None=>return Err(crate::errno::ESRCH)};let child=match with_current_process(|p|p.fork_child(child_cr3)){Some(Ok(v))=>v,Some(Err(e))=>return Err(e),None=>return Err(crate::errno::ESRCH)};spawn_fork(child,frame)}
extern "sysv64" fn user_thread_trampoline()->!{let slot=SCHED.lock().current;let loaded=USER_STARTS.lock()[slot].take().unwrap();crate::println!("task: entering user pid={} rip={:#x} rsp={:#x}",current_pid(),loaded.entry,loaded.rsp);unsafe{crate::arch::x86_64::user::enter(loaded.entry,loaded.rsp)}}
extern "sysv64" fn fork_thread_trampoline()->!{let slot=SCHED.lock().current;let frame=FORK_RESUMES.lock()[slot].take().unwrap();crate::println!("task: fork child resumed pid={}",current_pid());unsafe{crate::arch::x86_64::user::resume(&frame)}}
extern "sysv64" fn kernel_thread_trampoline()->!{let slot=SCHED.lock().current;let entry=KERNEL_ENTRIES.lock()[slot].take().unwrap();unsafe{core::arch::asm!("sti",options(nomem,nostack,preserves_flags));}entry();finish_current(0)}
pub fn yield_now(){schedule(false)}
pub fn on_timer_tick(){NEED_RESCHED.store(true,Ordering::Release);wake_sleepers(crate::arch::x86_64::apic::ticks());}
pub fn take_need_resched()->bool{NEED_RESCHED.swap(false,Ordering::AcqRel)}
/* A wake that lands before the target blocks is latched in wake_pending, so a
   register-then-block waiter can never lose it. */
pub fn sleep_until(tick:u64){let mut s=SCHED.lock();let current=s.current;if s.tasks[current].wake_pending{s.tasks[current].wake_pending=false;return}s.tasks[current].wake_tick=tick;s.tasks[current].state=State::Sleeping;drop(s);schedule(true)}
fn wake_sleepers(now:u64){let mut s=SCHED.lock();for t in &mut s.tasks{if t.state==State::Sleeping&&t.wake_pending{t.wake_pending=false;t.state=State::Runnable}else if t.state==State::Sleeping&&t.wake_tick<=now{t.state=State::Runnable;}}}
pub fn block_current(){let mut s=SCHED.lock();let current=s.current;if s.tasks[current].wake_pending{s.tasks[current].wake_pending=false;return}s.tasks[current].state=State::Sleeping;s.tasks[current].wake_tick=u64::MAX;drop(s);schedule(true)}
pub fn wake_pid(pid:u32)->bool{let mut s=SCHED.lock();if let Some(t)=s.tasks.iter_mut().find(|t|t.pid==pid&&t.state!=State::Unused&&t.state!=State::Zombie){if t.state==State::Sleeping{t.state=State::Runnable}else{t.wake_pending=true}true}else{false}}
/* Marks sig pending on the task owning pid and wakes it if blocked. */
pub fn signal_pid(pid:u32,sig:i32)->Result<(),i64>{
 let slot={let s=SCHED.lock();s.tasks.iter().position(|t|t.pid==pid&&t.state!=State::Unused).ok_or(crate::errno::ESRCH)?};
 PROCS.lock()[slot].as_mut().ok_or(crate::errno::ESRCH)?.pending|=crate::signal::bit(sig);
 wake_pid(pid);
 Ok(())
}
fn finish(code:i32,signal:u8)->!{{let mut s=SCHED.lock();let current=s.current;s.tasks[current].state=State::Zombie;s.tasks[current].exit_code=code;s.tasks[current].term_signal=signal;let parent=s.tasks[current].ppid;drop(s);if parent!=0{let _=signal_pid(parent,crate::signal::SIGCHLD);}}loop{schedule(true);unsafe{core::arch::asm!("hlt",options(nomem,nostack));}}}
pub fn finish_current(code:i32)->!{finish(code,0)}
pub fn exit(code:i32)->!{crate::println!("task {} exited {}",current_pid(),code);with_current_process(|proc|{crate::println!("task exited status={}",code);proc.teardown_exit(code,0)});finish(code,0)}
/* Kills the current task as if terminated by sig; wait status reports the signal. */
pub fn exit_signal(sig:i32)->!{crate::println!("task {} terminated by signal {}",current_pid(),sig);with_current_process(|proc|proc.teardown_exit(128+sig,sig as u64));finish(128+sig,sig as u8)}
pub fn exit_fault(vector:u64,error:u64,rip:u64,cr2:u64)->!{crate::println!("user fault vector={} error={:#x} rip={:#x} cr2={:#x}",vector,error,rip,cr2);with_current_process(|proc|proc.teardown_exit(128+11,vector));finish(128+11,11)}
fn schedule(blocked:bool){let(old_ptr,next)= {let mut s=SCHED.lock();let old=s.current;if !blocked&&s.tasks[old].state==State::Running{s.tasks[old].state=State::Runnable;}let mut next=old;for step in 1..=MAX_TASKS{let i=(old+step)%MAX_TASKS;if s.tasks[i].state==State::Runnable{next=i;break}}if next==old{s.tasks[old].state=State::Running;return}s.tasks[next].state=State::Running;s.current=next;(&mut s.tasks[old].rsp as*mut u64,s.tasks[next])};PID.store(next.pid,Ordering::Relaxed);unsafe{apply_arch_state(next);switch_context(old_ptr,next.rsp)}}

pub fn set_current_fs_base(value:u64){let mut s=SCHED.lock();let current=s.current;s.tasks[current].fs_base=value;drop(s);unsafe{crate::arch::x86_64::msr::wrmsr(crate::arch::x86_64::msr::IA32_FS_BASE,value)}}
pub fn fs_base()->u64{let s=SCHED.lock();s.tasks[s.current].fs_base}
pub fn bind_current_address_space(cr3:u64,kernel_stack_top:u64){let mut s=SCHED.lock();let current=s.current;s.tasks[current].cr3=cr3;s.tasks[current].kernel_stack_top=kernel_stack_top;drop(s);unsafe{crate::arch::x86_64::descriptors::set_rsp0(kernel_stack_top);crate::arch::x86_64::syscall::set_kernel_stack(kernel_stack_top)}}
unsafe fn apply_arch_state(task:Task){unsafe{let current=crate::arch::x86_64::paging::PageTableRoot::current().pml4;if current!=task.cr3{crate::arch::x86_64::paging::PageTableRoot{pml4:task.cr3}.activate();}crate::arch::x86_64::msr::wrmsr(crate::arch::x86_64::msr::IA32_FS_BASE,task.fs_base);crate::arch::x86_64::descriptors::set_rsp0(task.kernel_stack_top);crate::arch::x86_64::syscall::set_kernel_stack(task.kernel_stack_top);}}
pub enum WaitOutcome{Reaped(u32,i32),NoHang,Blocked}
fn encode_status(t:Task)->i32{if t.term_signal!=0{t.term_signal as i32}else{(t.exit_code&0xff)<<8}}
pub fn reap_child(pid_sel:i64,nohang:bool)->Result<WaitOutcome,i64>{
 let me=current_pid();let my_pgid={let s=SCHED.lock();s.tasks[s.current].pgid};
 let matches=|t:&Task|t.ppid==me&&(pid_sel==-1||(pid_sel==0&&t.pgid==my_pgid)||(pid_sel>0&&t.pid==pid_sel as u32)||(pid_sel< -1&&t.pgid==(-pid_sel) as u32));
 let mut s=SCHED.lock();
 if let Some(slot)=s.tasks.iter().position(|t|t.state==State::Zombie&&matches(t)){let t=s.tasks[slot];s.tasks[slot]=EMPTY;drop(s);KERNEL_ENTRIES.lock()[slot]=None;USER_STARTS.lock()[slot]=None;FORK_RESUMES.lock()[slot]=None;PROCS.lock()[slot]=None;return Ok(WaitOutcome::Reaped(t.pid,encode_status(t)))}
 let has_child=s.tasks.iter().any(|t|t.state!=State::Unused&&matches(t));drop(s);
 if !has_child{return Err(crate::errno::ECHILD)}
 if nohang{Ok(WaitOutcome::NoHang)}else{Ok(WaitOutcome::Blocked)}
}
pub fn wait_child(child_pid:u32)->Result<i32,i64>{loop{match reap_child(child_pid as i64,false)?{WaitOutcome::Reaped(_,code)=>return Ok(code),_=>{yield_now();/* Idle until the next interrupt so sleeping children can be woken by ticks. */unsafe{core::arch::asm!("sti","hlt","cli",options(nomem,nostack));}}}}}
pub fn scheduler_self_test()->Result<(),&'static str>{static A:AtomicU32=AtomicU32::new(0);static B:AtomicU32=AtomicU32::new(0);static TLS_OK:AtomicU32=AtomicU32::new(0);fn a(){set_current_fs_base(0x1000);A.fetch_add(1,Ordering::Relaxed);yield_now();if unsafe{crate::arch::x86_64::msr::rdmsr(crate::arch::x86_64::msr::IA32_FS_BASE)}==0x1000{TLS_OK.fetch_add(1,Ordering::Relaxed);}A.fetch_add(1,Ordering::Relaxed);}fn b(){set_current_fs_base(0x2000);B.fetch_add(1,Ordering::Relaxed);yield_now();if unsafe{crate::arch::x86_64::msr::rdmsr(crate::arch::x86_64::msr::IA32_FS_BASE)}==0x2000{TLS_OK.fetch_add(1,Ordering::Relaxed);}B.fetch_add(1,Ordering::Relaxed);}spawn_kernel(a).map_err(|_|"spawn A failed")?;spawn_kernel(b).map_err(|_|"spawn B failed")?;for _ in 0..8{yield_now();if A.load(Ordering::Relaxed)==2&&B.load(Ordering::Relaxed)==2&&TLS_OK.load(Ordering::Relaxed)==2{let ca=wait_child(2).map_err(|_|"reap A failed")?;let cb=wait_child(3).map_err(|_|"reap B failed")?;if ca!=0||cb!=0{return Err("unexpected child status")}crate::println!("scheduler: context, FS base and zombie reap self-test passed");return Ok(())}}Err("scheduler self-test stalled")}
pub fn run()->!{loop{yield_now();unsafe{core::arch::asm!("sti","hlt",options(nomem,nostack));}}}
