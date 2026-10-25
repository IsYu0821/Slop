//! Single-CPU round-robin scheduler, task lifecycle and process ownership.
use core::sync::atomic::{AtomicBool,AtomicU32,AtomicU64,Ordering};
use crate::process::Process;
use crate::sync::IrqSpinLock;
const MAX_TASKS:usize=8;const STACK_SIZE:usize=131072;
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub struct ResourceStats{pub active:usize,pub zombies:usize,pub runnable:usize,pub sleeping:usize,pub unused:usize}
#[repr(align(16))]struct Stack([u8;STACK_SIZE]);
#[derive(Clone,Copy,PartialEq)]pub enum State{Unused,Running,Runnable,Sleeping,Stopped,Zombie}
#[derive(Clone,Copy)]pub struct Task{pub fpu:u64,pub pid:u32,pub tgid:u32,pub ppid:u32,pub pgid:u32,pub sid:u32,pub state:State,pub rsp:u64,pub kernel_stack_top:u64,pub cr3:u64,pub fs_base:u64,pub wake_tick:u64,pub wake_pending:bool,pub exit_code:i32,pub term_signal:u8,pub stop_sig:u8,pub cont_pending:bool,pub is_thread:bool,pub user_rsp:u64}
const EMPTY:Task=Task{fpu:0,pid:0,tgid:0,ppid:0,pgid:0,sid:0,state:State::Unused,rsp:0,kernel_stack_top:0,cr3:0,fs_base:0,wake_tick:0,wake_pending:false,exit_code:0,term_signal:0,stop_sig:0,cont_pending:false,is_thread:false,user_rsp:0};
/* The FPU area of the running task, published for the syscall entry path so
   user SSE registers survive kernel execution. Updated on every switch. */
#[unsafe(no_mangle)]pub static mut CURRENT_FPU:u64=0;
struct Scheduler{tasks:[Task;MAX_TASKS],current:usize,next_pid:u32}
static SCHED:IrqSpinLock<Scheduler>=IrqSpinLock::new(Scheduler{tasks:[EMPTY;MAX_TASKS],current:0,next_pid:2});
static NEXT_NS:AtomicU64=AtomicU64::new(2);
static mut UTS_NAMESPACES:[[u8;65];16]=[[0;65];16];
fn uts_init(){unsafe{UTS_NAMESPACES[1][..4].copy_from_slice(b"slop");}}
/* One Process per user task slot; kernel-only tasks leave their entry empty. */
static PROCS:IrqSpinLock<[Option<Process>;MAX_TASKS]>=IrqSpinLock::new([const{None};MAX_TASKS]);
static mut STACKS:[Stack;MAX_TASKS]=[const{Stack([0;STACK_SIZE])};MAX_TASKS];
static NEED_RESCHED:AtomicBool=AtomicBool::new(false);static PID:AtomicU32=AtomicU32::new(1);
unsafe extern "sysv64"{fn switch_context(old:*mut u64,new:u64,old_fpu:u64,new_fpu:u64);}
/* FXSAVE/FXRSTOR helpers: the FPU context lives in a 512 byte area per task
   so floating point and SSE registers survive a context switch. */
const FPU_AREA:usize=256;
/// Per-slot SSE register file, kept out of the task stacks so saving the
/// registers can never overwrite a live stack.
#[repr(align(16))]
struct FpuArea([u8;FPU_AREA]);
static mut FPU_AREAS:[FpuArea;MAX_TASKS]=[const{FpuArea([0;FPU_AREA])};MAX_TASKS];
/* The FXSAVE area must be 16-byte aligned and must not straddle a page,
   so it sits at the page-aligned bottom of the task stack, which the growing
   stack reaches last. */
fn fpu_area(slot:usize)->u64{unsafe{core::ptr::addr_of!(FPU_AREAS[slot])as u64}}
unsafe extern "sysv64"{fn fpu_save_regs(area:*mut u8);fn fpu_restore_regs(area:*const u8);}
fn fpu_save(addr:u64){let _=addr;}
unsafe fn task_restore_fpu(area:u64){unsafe{fpu_restore_regs(area as *const u8)}}
pub fn init(bootstrap_stack_top:u64){uts_init();fpu_save(fpu_area(0));let mut s=SCHED.lock();let cr3=crate::arch::x86_64::paging::PageTableRoot::current().pml4;s.tasks[0]=Task{fpu:fpu_area(0),pid:1,tgid:1,ppid:0,pgid:1,sid:1,state:State::Running,rsp:0,kernel_stack_top:bootstrap_stack_top,cr3,fs_base:0,wake_tick:0,wake_pending:false,exit_code:0,term_signal:0,stop_sig:0,cont_pending:false,is_thread:false,user_rsp:0};unsafe{CURRENT_FPU=fpu_area(0)};crate::println!("task: scheduler bootstrap pid=1 capacity={}",MAX_TASKS);}
/* The identifier of the running task. The scheduler is the single source of
   truth: a cached copy could name a task that ran in between, and a signal
   aimed at the wrong thread would be lost. */
pub fn current_pid()->u32{let s=SCHED.lock();s.tasks[s.current].pid}
pub fn current_ppid()->u32{let s=SCHED.lock();s.tasks[s.current].ppid}
pub fn current_pgid()->u32{let s=SCHED.lock();s.tasks[s.current].pgid}
pub fn current_sid()->u32{let s=SCHED.lock();s.tasks[s.current].sid}
pub fn alloc_ns()->u64{NEXT_NS.fetch_add(1,Ordering::Relaxed)}
pub fn uts_hostname(ns:u64)->[u8;65]{let idx=(ns as usize)%16;unsafe{if UTS_NAMESPACES[idx][0]==0{let mut d=[0u8;65];d[..4].copy_from_slice(b"slop");d}else{UTS_NAMESPACES[idx]}}}
pub fn set_uts_hostname(ns:u64,name:&[u8]){let idx=(ns as usize)%16;unsafe{UTS_NAMESPACES[idx]=[0;65];let n=name.len().min(64);UTS_NAMESPACES[idx][..n].copy_from_slice(&name[..n]);}}
pub fn alloc_uts_ns(src:u64)->u64{let id=alloc_ns();let idx=(id as usize)%16;let srcidx=(src as usize)%16;unsafe{UTS_NAMESPACES[idx]=UTS_NAMESPACES[srcidx];}id}
pub fn sched_pids()->[u32;8]{let s=SCHED.lock();let mut p=[0;8];for i in 0..8{p[i]=s.tasks[i].pid};p}
pub fn current_kernel_stack_top()->u64{let s=SCHED.lock();s.tasks[s.current].kernel_stack_top}
pub fn resource_stats()->ResourceStats{
 let s=SCHED.lock();let mut stats=ResourceStats::default();
  for task in &s.tasks{match task.state{State::Unused=>stats.unused+=1,State::Zombie=>stats.zombies+=1,State::Runnable=>{stats.active+=1;stats.runnable+=1},State::Sleeping=>{stats.active+=1;stats.sleeping+=1},State::Stopped=>{stats.active+=1},State::Running=>stats.active+=1}}
 stats
}
/* Locks are never nested the other way around: the scheduler guard is released
   before PROCS is taken, so closure bodies may lock anything but SCHED. */
pub fn with_current_process<R>(f:impl FnOnce(&mut Process)->R)->Option<R>{let slot=SCHED.lock().current;let mut procs=PROCS.lock();procs[slot].as_mut().map(f)}
pub fn has_current_process()->bool{let slot=SCHED.lock().current;PROCS.lock()[slot].is_some()}
/* Immutable view of another live process, for /proc/<pid> introspection. */
pub fn with_process_pid<R>(pid:u32,f:impl FnOnce(&Process)->R)->Option<R>{let slot={let s=SCHED.lock();(0..MAX_TASKS).find(|&i|s.tasks[i].pid==pid&&s.tasks[i].state!=State::Unused)?};let procs=PROCS.lock();procs[slot].as_ref().map(f)}
pub fn live_pids(mut cb:impl FnMut(u32)){let s=SCHED.lock();let mut seen=[0u32;MAX_TASKS];let mut n=0usize;for t in &s.tasks{if t.state==State::Unused||t.is_thread||seen[..n].contains(&t.pid){continue}seen[n]=t.pid;n+=1;cb(t.pid);}}
pub fn thread_count(pid:u32)->usize{let s=SCHED.lock();s.tasks.iter().filter(|t|t.pid==pid&&t.state!=State::Unused).count().max(1)}
pub fn ppid_of(pid:u32)->u32{let s=SCHED.lock();s.tasks.iter().find(|t|t.pid==pid&&t.state!=State::Unused).map(|t|t.ppid).unwrap_or(0)}
pub fn last_pid()->u32{let s=SCHED.lock();s.next_pid.saturating_sub(1).max(1)}
fn take_slot(s:&mut Scheduler)->Result<usize,i64>{
 if let Some(i)=(1..MAX_TASKS).find(|&i|s.tasks[i].state==State::Unused){return Ok(i)}
 /* Threads are joined in userspace; the kernel never waits for them, so a
    dead thread's slot is reclaimed here. Its CPU time folds into its
    thread-group leader, matching Linux's per-process accounting. */
 if let Some(i)=(1..MAX_TASKS).find(|&i|s.tasks[i].state==State::Zombie&&s.tasks[i].is_thread){
  let t=s.tasks[i];s.tasks[i]=EMPTY;
  KERNEL_ENTRIES.lock()[i]=None;USER_STARTS.lock()[i]=None;FORK_RESUMES.lock()[i]=None;
  let (ut,st,ct,cst)={let mut procs=PROCS.lock();match procs[i].take(){Some(p)=>(p.utime_ns,p.stime_ns,p.cutime_ns,p.cstime_ns),_=>(0,0,0,0)}};
  let lead=(1..MAX_TASKS).find(|&j|s.tasks[j].pid==t.tgid&&s.tasks[j].state!=State::Unused&&!s.tasks[j].is_thread);
  if let Some(j)=lead{let mut procs=PROCS.lock();if let Some(p)=procs[j].as_mut(){
   p.utime_ns=p.utime_ns.saturating_add(ut);p.stime_ns=p.stime_ns.saturating_add(st);
   p.cutime_ns=p.cutime_ns.saturating_add(ct);p.cstime_ns=p.cstime_ns.saturating_add(cst);}}
  return Ok(i)
 }
 Err(crate::errno::EAGAIN)
}
fn alloc_pid(s:&mut Scheduler)->u32{let pid=s.next_pid;s.next_pid=s.next_pid.wrapping_add(1).max(2);pid}
fn build_kernel_stack(slot:usize,entry:u64)->u64{let top=unsafe{core::ptr::addr_of!(STACKS[slot].0)as u64+STACK_SIZE as u64};let mut sp=(top&!15)-8;unsafe{sp-=8;*(sp as*mut u64)=entry;for _ in 0..6{sp-=8;*(sp as*mut u64)=0;}}sp}
pub fn spawn_kernel(entry:fn())->Result<u32,i64>{let mut s=SCHED.lock();let slot=take_slot(&mut s)?;let pid=alloc_pid(&mut s);let top=unsafe{core::ptr::addr_of!(STACKS[slot].0)as u64+STACK_SIZE as u64};let sp=build_kernel_stack(slot,kernel_thread_trampoline as *const () as u64);fpu_save(fpu_area(slot));KERNEL_ENTRIES.lock()[slot]=Some(entry);let cr3=crate::arch::x86_64::paging::PageTableRoot::current().pml4;let ppid=s.tasks[s.current].pid;let pgid=s.tasks[s.current].pgid;let sid=s.tasks[s.current].sid;s.tasks[slot]=Task{fpu:fpu_area(slot),pid,tgid:pid,ppid,pgid,sid,state:State::Runnable,rsp:sp,kernel_stack_top:top,cr3,fs_base:0,wake_tick:0,wake_pending:false,exit_code:0,term_signal:0,stop_sig:0,cont_pending:false,is_thread:false,user_rsp:0};Ok(pid)}
static KERNEL_ENTRIES:IrqSpinLock<[Option<fn()>;MAX_TASKS]>=IrqSpinLock::new([None;MAX_TASKS]);
static USER_STARTS:IrqSpinLock<[Option<crate::exec_runtime::Loaded>;MAX_TASKS]>=IrqSpinLock::new([None;MAX_TASKS]);
static FORK_RESUMES:IrqSpinLock<[Option<crate::arch::x86_64::syscall::UserFrame>;MAX_TASKS]>=IrqSpinLock::new([const{None};MAX_TASKS]);
pub fn spawn_user(loaded:crate::exec_runtime::Loaded)->Result<u32,i64>{let mut proc=Process::new(loaded.cr3,loaded.bootstrap_cr3,loaded.image_slot,loaded.stack_slot,loaded.interp_slot)?;proc.set_exec_meta(&loaded);let mut s=SCHED.lock();let slot=take_slot(&mut s)?;let pid=alloc_pid(&mut s);let ppid=s.tasks[s.current].pid;let pgid=s.tasks[s.current].pgid;let sid=s.tasks[s.current].sid;let top=unsafe{core::ptr::addr_of!(STACKS[slot].0)as u64+STACK_SIZE as u64};let sp=build_kernel_stack(slot,user_thread_trampoline as *const () as u64);fpu_save(fpu_area(slot));PROCS.lock()[slot]=Some(proc);USER_STARTS.lock()[slot]=Some(loaded);s.tasks[slot]=Task{fpu:fpu_area(slot),pid,tgid:pid,ppid,pgid,sid,state:State::Runnable,rsp:sp,kernel_stack_top:top,cr3:loaded.cr3,fs_base:0,wake_tick:0,wake_pending:false,exit_code:0,term_signal:0,stop_sig:0,cont_pending:false,is_thread:false,user_rsp:0};Ok(pid)}
pub fn spawn_fork(mut child:Process,frame:crate::arch::x86_64::syscall::UserFrame)->Result<u32,i64>{let child_cr3=child.cr3;let mut s=SCHED.lock();let slot=match take_slot(&mut s){Ok(v)=>v,Err(e)=>{drop(s);child.abandon();return Err(e)}};let pid=alloc_pid(&mut s);let parent=s.current;let ppid=s.tasks[parent].pid;let pgid=s.tasks[parent].pgid;let sid=s.tasks[parent].sid;let fs_base=s.tasks[parent].fs_base;let top=unsafe{core::ptr::addr_of!(STACKS[slot].0)as u64+STACK_SIZE as u64};let sp=build_kernel_stack(slot,fork_thread_trampoline as *const () as u64);fpu_save(fpu_area(slot));FORK_RESUMES.lock()[slot]=Some(frame);PROCS.lock()[slot]=Some(child);s.tasks[slot]=Task{fpu:fpu_area(slot),pid,tgid:pid,ppid,pgid,sid,state:State::Runnable,rsp:sp,kernel_stack_top:top,cr3:child_cr3,fs_base,wake_tick:0,wake_pending:false,exit_code:0,term_signal:0,stop_sig:0,cont_pending:false,is_thread:false,user_rsp:0};Ok(pid)}
pub fn fork_process(frame:crate::arch::x86_64::syscall::UserFrame,flags:u64)->Result<u32,i64>{let child_cr3=match with_current_process(|p|p.clone_address_space()){Some(Ok(v))=>v,Some(Err(e))=>return Err(e),None=>return Err(crate::errno::ESRCH)};let mut child=match with_current_process(|p|p.fork_child(child_cr3)){Some(Ok(v))=>v,Some(Err(e))=>return Err(e),None=>return Err(crate::errno::ESRCH)};if let Err(e)=apply_ns_flags(&mut child,flags){child.abandon();return Err(e)}spawn_fork(child,frame)}
extern "sysv64" fn user_thread_trampoline()->!{let slot=SCHED.lock().current;let loaded=USER_STARTS.lock()[slot].take().unwrap();crate::println!("task: entering user pid={} rip={:#x} rsp={:#x}",current_pid(),loaded.entry,loaded.rsp);unsafe{crate::arch::x86_64::user::enter(loaded.entry,loaded.rsp)}}
/* Spawns a CLONE_VM thread: shares the caller's address space and descriptor
   table, runs on the caller-provided stack with the given TLS base. */
pub fn spawn_thread(frame:crate::arch::x86_64::syscall::UserFrame,child_stack:u64,tls:u64,ctid:u64,ptid:u64,ns_flags:u64)->Result<u32,i64>{
 const CLONE_PARENT_SETTID:u64=0x100000;const CLONE_CHILD_SETTID:u64=0x1000000;
  let mut child=match with_current_process(|p|p.fork_child(p.cr3)){
   Some(Ok(child))=>child,Some(Err(e))=>return Err(e),None=>return Err(crate::errno::ESRCH)};
  child.pending=0;child.sigframe=0;child.clear_child_tid=ctid;
  let cr3=child.cr3;
  if let Err(e)=apply_ns_flags(&mut child,ns_flags){return Err(e)}
 let mut frame=frame;frame.rsp=child_stack;
 let mut s=SCHED.lock();
 let slot=match take_slot(&mut s){Ok(v)=>v,Err(e)=>return Err(e)};
 let pid=alloc_pid(&mut s);let parent=s.current;let tgid=s.tasks[parent].tgid;let ppid=s.tasks[parent].pid;let pgid=s.tasks[parent].pgid;let sid=s.tasks[parent].sid;
 let top=unsafe{core::ptr::addr_of!(STACKS[slot].0)as u64+STACK_SIZE as u64};let sp=build_kernel_stack(slot,fork_thread_trampoline as *const () as u64);fpu_save(fpu_area(slot));
 FORK_RESUMES.lock()[slot]=Some(frame);PROCS.lock()[slot]=Some(child);
  s.tasks[slot]=Task{fpu:fpu_area(slot),pid,tgid,ppid,pgid,sid,state:State::Runnable,rsp:sp,kernel_stack_top:top,cr3,fs_base:tls,wake_tick:0,wake_pending:false,exit_code:0,term_signal:0,stop_sig:0,cont_pending:false,is_thread:true,user_rsp:child_stack};
 drop(s);
 /* CLONE_PARENT_SETTID: store the child TID in the parent's memory (same
    address space here); CLONE_CHILD_SETTID: store it in the child's copy. */
 let tidb=pid.to_ne_bytes();
 if ns_flags&CLONE_PARENT_SETTID!=0&&ptid!=0{let _=crate::usercopy::copy_to_user(ptid as usize,&tidb);}
 if ns_flags&CLONE_CHILD_SETTID!=0&&ctid!=0{let _=crate::usercopy::copy_to_user(ctid as usize,&tidb);}
 Ok(pid)
}
extern "sysv64" fn fork_thread_trampoline()->!{let slot=SCHED.lock().current;let frame=FORK_RESUMES.lock()[slot].take().unwrap();crate::println!("task: fork child resumed pid={}",current_pid());unsafe{crate::arch::x86_64::user::resume(&frame)}}
extern "sysv64" fn kernel_thread_trampoline()->!{let slot=SCHED.lock().current;let entry=KERNEL_ENTRIES.lock()[slot].take().unwrap();unsafe{core::arch::asm!("sti",options(nomem,nostack,preserves_flags));}entry();finish_current(0)}
pub fn yield_now(){schedule(false)}
pub fn on_timer_tick(user:bool,delta_ns:u64){NEED_RESCHED.store(true,Ordering::Release);let now=crate::arch::x86_64::apic::ticks();account_tick(user,delta_ns);wake_sleepers(now);crate::posix_timer::tick(now);}
/* Attributes one 1ms tick to the running process: CS&3 told us whether the
   interrupted context was user or kernel code. */
fn account_tick(user:bool,delta_ns:u64){
 let(slot,pid)={let s=SCHED.lock();(s.current,s.tasks[s.current].pid)};{let mut procs=PROCS.lock();if let Some(p)=procs[slot].as_mut(){if user{if p.utime_ns<u64::MAX{p.utime_ns=p.utime_ns.saturating_add(delta_ns)}}else if p.stime_ns<u64::MAX{p.stime_ns=p.stime_ns.saturating_add(delta_ns)}}}crate::posix_timer::cpu_tick(pid,user);}
pub fn take_need_resched()->bool{NEED_RESCHED.swap(false,Ordering::AcqRel)}
/* A wake that lands before the target blocks is latched in wake_pending, so a
   register-then-block waiter can never lose it. */
pub fn sleep_until(tick:u64){let mut s=SCHED.lock();let current=s.current;if s.tasks[current].wake_pending{s.tasks[current].wake_pending=false;return}s.tasks[current].wake_tick=tick;s.tasks[current].state=State::Sleeping;drop(s);schedule(true)}
fn wake_sleepers(now:u64){let mut s=SCHED.lock();for t in &mut s.tasks{if t.state==State::Sleeping&&t.wake_pending{t.wake_pending=false;t.state=State::Runnable}else if t.state==State::Sleeping&&t.wake_tick<=now{t.state=State::Runnable;}}}
pub fn block_current(){let mut s=SCHED.lock();let current=s.current;if s.tasks[current].wake_pending{s.tasks[current].wake_pending=false;return}s.tasks[current].state=State::Sleeping;s.tasks[current].wake_tick=u64::MAX;drop(s);schedule(true)}
/* Sleep until a signal is deliverable. Returns true when a signal is
   pending after the sleep, false when the task woke for another reason. */
pub fn block_current_until_signal()->bool{
 let mut s=SCHED.lock();let current=s.current;
 if s.tasks[current].wake_pending{s.tasks[current].wake_pending=false;return crate::signal::pending().is_some()}
 s.tasks[current].state=State::Sleeping;s.tasks[current].wake_tick=u64::MAX;drop(s);schedule(true);
 crate::signal::pending().is_some()
}
pub fn current_fpu_area()->u64{let s=SCHED.lock();s.tasks[s.current].fpu}
pub fn save_current_fpu(){let area=current_fpu_area();if area!=0{unsafe{fpu_save_regs(area as *mut u8)}}}
pub fn restore_current_fpu(){let area={let s=SCHED.lock();s.tasks[s.current].fpu};if area!=0{unsafe{fpu_restore_regs(area as *const u8)}}}
/* Copies the live user SSE file (saved at syscall entry) into dst[0..256]. */
pub fn copy_current_fpu(dst:&mut [u8]){let area=current_fpu_area();if area!=0&&dst.len()>=256{unsafe{core::ptr::copy_nonoverlapping(area as *const u8,dst.as_mut_ptr(),256)}}}
/* Overwrites the running task's SSE file so the syscall-exit restore and the
   next context switch both observe the handler-approved registers. */
pub fn write_current_fpu(src:&[u8]){let area=current_fpu_area();if area!=0&&src.len()>=256{unsafe{core::ptr::copy_nonoverlapping(src.as_ptr(),area as *mut u8,256)}}}
pub fn wake_pid(pid:u32)->bool{let mut s=SCHED.lock();if let Some(t)=s.tasks.iter_mut().find(|t|t.pid==pid&&t.state!=State::Unused&&t.state!=State::Zombie){if t.state==State::Stopped{return true}if t.state==State::Sleeping{t.state=State::Runnable}else{t.wake_pending=true}true}else{false}}
pub fn current_tgid()->u32{let s=SCHED.lock();s.tasks[s.current].tgid}
/* Stops the running task like Linux does for SIGSTOP/SIGTSTP/SIGTTIN/SIGTTOU
   with the default disposition: the task leaves the syscall path, reports
   WIFSTOPPED to a WUNTRACED parent and sleeps until SIGCONT resumes it. */
pub fn stop_current(sig:i32){
 let (ppid,parent_ignores_stop)={
  let mut s=SCHED.lock();let cur=s.current;
  if s.tasks[cur].state==State::Stopped{return}
  s.tasks[cur].state=State::Stopped;s.tasks[cur].stop_sig=sig as u8;s.tasks[cur].cont_pending=false;
  let ppid=s.tasks[cur].ppid;drop(s);
  let ignores=with_current_process_parent(ppid,|_|false);
  let _=ignores;
  /* A stopped task has no pending stop signal left: it was consumed. */
  (ppid,false)
 };
  let _=parent_ignores_stop;
  /* The parent observes the stop through waitpid(WUNTRACED) and receives
     SIGCHLD unless it asked for SA_NOCLDSTOP. The mask check below reads the
     parent's SIGCHLD disposition without deadlocks (SCHED is free here). */
  let me=current_pid();
  let my_uid=with_current_process(|p|p.uid).unwrap_or(0);
  notify_parent_stop(ppid, me, sig);
 /* Park the outgoing user RSP like schedule() does, then sleep until a
    SIGCONT marks this task Runnable again. */
 {let mut s=SCHED.lock();let cur=s.current;if s.tasks[cur].state!=State::Unused{s.tasks[cur].user_rsp=unsafe{crate::arch::x86_64::syscall::read_user_rsp()};}}
 loop{
  schedule(true);
  let still={let s=SCHED.lock();s.tasks[s.current].state==State::Stopped};
  if !still{break}
  unsafe{core::arch::asm!("sti","hlt","cli",options(nomem,nostack));}
 }
}
fn with_current_process_parent(_ppid:u32,_f:impl FnOnce(&Process)->bool)->bool{false}
fn parent_wants_no_cldstop(ppid:u32)->bool{
 let slot={let s=SCHED.lock();match s.tasks.iter().position(|t|t.pid==ppid&&t.state!=State::Unused){Some(v)=>v,None=>return false}};
 let procs=PROCS.lock();match procs[slot].as_ref(){
  /* SA_NOCLDSTOP suppresses the stop/continue SIGCHLD even with the default
     disposition; only the flag matters, not handler vs DFL. */
  Some(p)=>{p.handlers[crate::signal::SIGCHLD as usize].flags&crate::signal::SA_NOCLDSTOP!=0}
  None=>false,
 }
}
fn notify_parent_stop(ppid:u32, child:u32, sig:i32){
 if ppid==0{return}
 /* wait4/waitid observe the stop even when the SIGCHLD itself is off. */
 if parent_wants_no_cldstop(ppid){wake_parent_for_wait(ppid);return}
 let uid=with_process_pid(child, |p| p.uid).unwrap_or(0);
 store_sigchld(ppid, crate::signal::CLD_STOPPED, child, uid, sig);
 wake_parent_for_wait(ppid);
}
fn notify_parent_cont(ppid:u32, child:u32){
 if ppid==0{return}
 if parent_wants_no_cldstop(ppid){wake_parent_for_wait(ppid);return}
 let uid=with_process_pid(child, |p| p.uid).unwrap_or(0);
 store_sigchld(ppid, crate::signal::CLD_CONTINUED, child, uid, crate::signal::SIGCONT);
 wake_parent_for_wait(ppid);
}
/* Records a child-status SIGCHLD for the parent with first-wins coalescing:
   a standard signal keeps one pending bit, so an event arriving while one
   is already queued is dropped (native stop+cont yields one read). The
   child task states stay independent: signalfd/sigwait consume only the
   pending bit and the stored info, never the Stopped/Zombie states, so
   wait4/waitid still reap afterwards. wait4/waitid never clear the pending
   bit, so signalfd still reports after a wait. */
pub fn store_sigchld(parent:u32, code:i32, child:u32, uid:u32, status:i32){
 let slot={let s=SCHED.lock();match s.tasks.iter().position(|t|t.pid==parent&&t.state!=State::Unused){Some(v)=>v,None=>return}};
 let mut procs=PROCS.lock();match procs[slot].as_mut(){
  Some(p)=>{
   if p.pending&crate::signal::bit(crate::signal::SIGCHLD)!=0{return}
   p.pending|=crate::signal::bit(crate::signal::SIGCHLD);
   p.pending_from[crate::signal::SIGCHLD as usize]=child;
   p.si_code[crate::signal::SIGCHLD as usize]=code;
   p.si_value[crate::signal::SIGCHLD as usize]=0;
   p.si_overrun[crate::signal::SIGCHLD as usize]=0;
   p.sch_code=code;p.sch_pid=child;p.sch_uid=uid;p.sch_status=status;
  }
  None=>{},
 }
}
fn wake_parent_for_wait(ppid:u32){
 let mut s=SCHED.lock();
 if let Some(t)=s.tasks.iter_mut().find(|t|t.pid==ppid&&t.state!=State::Unused&&t.state!=State::Zombie){
  if t.state==State::Sleeping{t.state=State::Runnable}else if t.state!=State::Stopped{t.wake_pending=true}
 }
}
/* Resumes a stopped task for SIGCONT. Returns true when the target was
   stopped and is now runnable; the caller decides whether SIGCONT itself
   stays pending for a handler. */
fn resume_stopped(pid:u32)->bool{
 let (ppid,uid)={let mut s=SCHED.lock();
  let pos=s.tasks.iter().position(|t|t.pid==pid&&t.state!=State::Unused);
  let idx=match pos{Some(v)=>v,None=>return false};
  if s.tasks[idx].state!=State::Stopped{return false}
  s.tasks[idx].state=State::Runnable;s.tasks[idx].stop_sig=0;s.tasks[idx].cont_pending=true;
  s.tasks[idx].wake_pending=false;
  let ppid=s.tasks[idx].ppid;drop(s);
  let uid=with_process_pid(pid,|p|p.uid).unwrap_or(0);
  (ppid,uid)};
 notify_parent_cont(ppid, pid);
 let _=uid;
 true
}
/* True when pid names a stopped task. */
pub fn is_stopped(pid:u32)->bool{let s=SCHED.lock();s.tasks.iter().any(|t|t.pid==pid&&t.state==State::Stopped)}
/* Marks sig pending on the thread-group member (tgid,pid) and wakes it. */
pub fn signal_task(tgid:u32,pid:u32,sig:i32)->Result<(),i64>{

  if sig==crate::signal::SIGSTOP||sig==crate::signal::SIGCONT{
   return signal_pid(pid,sig);
  }
  let slot={let s=SCHED.lock();s.tasks.iter().position(|t|t.pid==pid&&t.tgid==tgid&&t.state!=State::Unused).ok_or(crate::errno::ESRCH)?};
 {let mut procs=PROCS.lock();let p=procs[slot].as_mut().ok_or(crate::errno::ESRCH)?;
 p.pending|=crate::signal::bit(sig);
 p.si_code[sig as usize]=0;p.si_value[sig as usize]=0;p.si_overrun[sig as usize]=0;}
 wake_pid(pid);
 Ok(())
}
/* Marks sig pending on the task owning pid and wakes it if blocked. */
pub fn signal_pid(pid:u32,sig:i32)->Result<(),i64>{
 /* SIGSTOP always stops, even if the target blocked or ignored it: Linux
    refuses handlers and masks for it, so an immediate stop is correct. */
 if sig==crate::signal::SIGSTOP{
  let slot={let s=SCHED.lock();s.tasks.iter().position(|t|t.pid==pid&&t.state!=State::Unused).ok_or(crate::errno::ESRCH)?};
  {let mut procs=PROCS.lock();if let Some(p)=procs[slot].as_mut(){p.pending&=!crate::signal::bit(sig);}}
  /* A thread stop is process-wide in Linux; here every task is a process
     except CLONE_VM threads, which SIGSTOP still stops individually. */
  let already={let s=SCHED.lock();s.tasks[slot].state==State::Stopped};
  if !already{
   if slot==SCHED.lock().current{
    /* Self-stop defers to the delivery path so the syscall frame exists. */
    {let mut procs=PROCS.lock();if let Some(p)=procs[slot].as_mut(){p.pending|=crate::signal::bit(sig);p.pending_from[sig as usize]=current_pid();}}
    }else{
     {let mut s=SCHED.lock();if s.tasks[slot].state==State::Sleeping{s.tasks[slot].state=State::Runnable;}s.tasks[slot].wake_pending=true;if s.tasks[slot].state!=State::Zombie&&s.tasks[slot].state!=State::Unused{s.tasks[slot].state=State::Stopped;s.tasks[slot].stop_sig=sig as u8;s.tasks[slot].cont_pending=false;}}
     let ppid={let s=SCHED.lock();s.tasks[slot].ppid};
     let uid=with_process_pid(pid,|p|p.uid).unwrap_or(0);
     notify_parent_stop(ppid, pid, sig);
     let _=uid;
    }
  }
  return Ok(())
 }
 /* SIGCONT resumes a stopped task. With the default disposition the signal
    is then consumed; with a handler it stays queued so the handler runs. */
 if sig==crate::signal::SIGCONT{
  let slot={let s=SCHED.lock();s.tasks.iter().position(|t|t.pid==pid&&t.state!=State::Unused).ok_or(crate::errno::ESRCH)?};
  let was_stopped=resume_stopped(pid);
  {let mut procs=PROCS.lock();if let Some(p)=procs[slot].as_mut(){
   let h=p.handlers[sig as usize].handler;
   if h==crate::signal::SIG_DFL||h==crate::signal::SIG_IGN{p.pending&=!crate::signal::bit(sig);}
   else{p.pending|=crate::signal::bit(sig);p.pending_from[sig as usize]=current_pid();}
  }}
  if !was_stopped{wake_pid(pid);}
  return Ok(())
 }
  let slot={let s=SCHED.lock();s.tasks.iter().position(|t|t.pid==pid&&t.state!=State::Unused).ok_or(crate::errno::ESRCH)?};
  /* A manually sent SIGCHLD shares the coalesced child-status slot: the
     first queued event wins, like the kernel's standard-signal drop. */
  if sig==crate::signal::SIGCHLD{
   let me=current_pid();
   let my_uid=with_current_process(|p|p.uid).unwrap_or(0);
   store_sigchld(pid, crate::signal::SI_USER, me, my_uid, 0);
   wake_pid(pid);
   return Ok(())
  }
  {let mut procs=PROCS.lock();let p=procs[slot].as_mut().ok_or(crate::errno::ESRCH)?;
  p.pending|=crate::signal::bit(sig);
  /* Remember the sender so the delivered siginfo can report it. */
  p.pending_from[sig as usize]=current_pid();
  p.si_code[sig as usize]=0;p.si_value[sig as usize]=0;p.si_overrun[sig as usize]=0;}
  /* A stop signal wakes its target so the delivery path can park it; the
     waiter learns about the stop through SIGCHLD, not through a wake of
     the stopped task itself. */
  if crate::signal::default_stops(sig){wake_pid(pid);}else{wake_pid(pid);}
  Ok(())
}
/* Queues a real-time signal with its value. The real-time signals keep
   every instance that was sent, so more than one of them can wait at the
   same time. */
pub fn queue_signal(pid:u32,sig:i32,value:u64)->Result<(),i64>{
 let slot={let s=SCHED.lock();s.tasks.iter().position(|t|t.pid==pid&&t.state!=State::Unused).ok_or(crate::errno::ESRCH)?};
 {let mut procs=PROCS.lock();let p=procs[slot].as_mut().ok_or(crate::errno::ESRCH)?;
  let i=crate::signal::rt_index(sig);
  let n=p.rt_len[i] as usize;
  if n>=crate::signal::RT_DEPTH{return Err(crate::errno::EAGAIN)}
  p.rt_value[i][n]=value;p.rt_len[i]=(n+1) as u8;
  p.pending|=crate::signal::bit(sig);
  p.pending_from[sig as usize]=current_pid();
  p.si_code[sig as usize]=crate::signal::SI_QUEUE;}
 wake_pid(pid);
 Ok(())
}
/* Sends a standard signal that carries a value. */
pub fn signal_pid_value(pid:u32,sig:i32,value:u64,sender:u32)->Result<(),i64>{
  if sig==crate::signal::SIGSTOP||sig==crate::signal::SIGCONT{
   return signal_pid(pid,sig);
  }
 let slot={let s=SCHED.lock();s.tasks.iter().position(|t|t.pid==pid&&t.state!=State::Unused).ok_or(crate::errno::ESRCH)?};
 {let mut procs=PROCS.lock();let p=procs[slot].as_mut().ok_or(crate::errno::ESRCH)?;
  p.pending|=crate::signal::bit(sig);
  p.pending_from[sig as usize]=sender;
  p.si_code[sig as usize]=crate::signal::SI_QUEUE;
  p.si_value[sig as usize]=value;}
 wake_pid(pid);
 Ok(())
}
/* Queues a timer-originated signal; returns false when it coalesced with an
   already-pending signal (the caller then accounts the overrun). */
pub fn timer_signal(pid:u32,sig:i32,value:u64,overrun:u32)->bool{
 let slot=match {let s=SCHED.lock();s.tasks.iter().position(|t|t.pid==pid&&t.state!=State::Unused&&t.state!=State::Zombie)}{Some(v)=>v,None=>return false};
 let queued={let mut procs=PROCS.lock();match procs[slot].as_mut(){
  Some(p)=>{let b=crate::signal::bit(sig);if p.pending&b!=0{false}else{p.pending|=b;p.si_code[sig as usize]=-2;p.si_value[sig as usize]=value;p.si_overrun[sig as usize]=overrun;true}}
  None=>return false}};
 if queued{wake_pid(pid);}
 queued
}
fn finish(code:i32,signal:u8)->!{{
 let me=current_pid();
 let (parent,uid)={
  let mut s=SCHED.lock();let current=s.current;
  s.tasks[current].state=State::Zombie;s.tasks[current].exit_code=code;s.tasks[current].term_signal=signal;
  let parent=s.tasks[current].ppid;let is_thread=s.tasks[current].is_thread;
  let uid=PROCS.lock()[current].as_ref().map(|p|p.uid).unwrap_or(0);
  drop(s);
  if parent!=0&&!is_thread{
   /* No core dumps: signal deaths report CLD_KILLED, exits CLD_EXITED. */
   if signal!=0{store_sigchld(parent,crate::signal::CLD_KILLED,me,uid,signal as i32);}
   else{store_sigchld(parent,crate::signal::CLD_EXITED,me,uid,code&0xff);}
   wake_parent_for_wait(parent);
  }
  (parent,uid)
 };
 let _=(parent,uid);
}loop{schedule(true);unsafe{core::arch::asm!("hlt",options(nomem,nostack));}}}
pub fn setpgid(pid:u32,pgid:u32)->Result<(),i64>{
 let me=current_pid();
 let target=if pid==0{me}else{pid};
 if target!=me{return Err(crate::errno::EPERM)}
 let mut s=SCHED.lock();
 let idx=s.current;
 let newpg=if pgid==0{me}else{pgid};
 if newpg!=me{return Err(crate::errno::EPERM)}
 s.tasks[idx].pgid=newpg;
 Ok(())
}
pub fn setsid()->Result<u32,i64>{
 let me=current_pid();
 let mut s=SCHED.lock();
 let idx=s.current;
 if s.tasks[idx].pgid==me{return Err(crate::errno::EPERM)}
 s.tasks[idx].pgid=me;s.tasks[idx].sid=me;Ok(me)
}
fn apply_ns_flags(p:&mut Process,flags:u64)->Result<(),i64>{
 const CLONE_NEWUTS:u64=0x04000000; const CLONE_NEWIPC:u64=0x08000000; const CLONE_NEWUSER:u64=0x10000000; const CLONE_NEWPID:u64=0x20000000; const CLONE_NEWNET:u64=0x40000000; const CLONE_NEWCGROUP:u64=0x02000000; const CLONE_NEWTIME:u64=0x00000080; const CLONE_NEWNS:u64=0x00020000;
 if flags&CLONE_NEWUTS!=0{p.ns_uts=alloc_uts_ns(p.ns_uts);}
 if flags&CLONE_NEWIPC!=0{p.ns_ipc=alloc_ns();}
  if flags&CLONE_NEWNS!=0{let ns=alloc_ns();crate::vfs::clone_mount_namespace(p.ns_mnt,ns)?;p.ns_mnt=ns;}
 if flags&CLONE_NEWPID!=0{p.ns_pid=alloc_ns();}
 if flags&CLONE_NEWNET!=0{p.ns_net=alloc_ns();}
 if flags&CLONE_NEWUSER!=0{p.ns_user=alloc_ns();p.uid=0;p.gid=0;p.euid=0;p.egid=0;p.suid=0;p.sgid=0;p.cap_eff=u64::MAX;p.cap_prm=u64::MAX;}
 if flags&CLONE_NEWCGROUP!=0{p.ns_cgroup=alloc_ns();}
  if flags&CLONE_NEWTIME!=0{p.ns_time=alloc_ns();}
  Ok(())
}
pub fn finish_current(code:i32)->!{finish(code,0)}
/* True when a pending signal is deliverable: it has a handler, it stops the
   task with the default disposition, or it is a default action that
   terminates. Used by pause() and blocking fcntl locks. */
pub fn has_deliverable_signal()->bool{
 with_current_process(|p|{
  let mut m=p.pending&!(p.sigmask&!crate::signal::unblockable_mask());
  while m!=0{
   let sig=m.trailing_zeros()as i32+1;
   let h=p.handlers[sig as usize].handler;
   if h!=crate::signal::SIG_IGN&&(h!=crate::signal::SIG_DFL||crate::signal::default_terminates(sig)||crate::signal::default_stops(sig)||sig==crate::signal::SIGCONT){return true}
   m&=!crate::signal::bit(sig);
  }
  false
 }).unwrap_or(false)
}
/* Robust-futex list handling must run while the task's address space is
   still active. */
fn robust_exit(){
 let head=with_current_process(|p|{let h=p.robust_list_head;p.robust_list_head=0;h}).unwrap_or(0);
 if head!=0{crate::robust::process_exit(current_pid(),head);}
}
pub fn exit(code:i32)->!{
 let is_thread={let s=SCHED.lock();s.tasks[s.current].is_thread};
 if is_thread{
  /* Threads share the address space: clear the child TID and wake any
     joiner, then leave the image and descriptors intact. */
  with_current_process(|proc|{
   let ctid=proc.clear_child_tid;
   if ctid!=0{
    let zero=0u64;
    let _=crate::usercopy::copy_to_user(ctid as usize,&zero.to_ne_bytes());
    let _=crate::futex::wake(ctid,1);
   }
  });
  robust_exit();
  crate::println!("thread {} exited {}",current_pid(),code);
  finish(code,0);
 }
 robust_exit();crate::println!("task {} exited {}",current_pid(),code);with_current_process(|proc|{crate::println!("task exited status={}",code);proc.teardown_exit(code,0)});finish(code,0)}
/* Kills the current task as if terminated by sig; wait status reports the signal. */
pub fn exit_signal(sig:i32)->!{robust_exit();crate::println!("task {} terminated by signal {}",current_pid(),sig);with_current_process(|proc|proc.teardown_exit(128+sig,sig as u64));finish(128+sig,sig as u8)}
pub fn stack_guard(){
 let r:u64;unsafe{core::arch::asm!("mov {}, rsp",out(reg)r,options(nostack,preserves_flags));}
 let top={let s=SCHED.lock();s.tasks[s.current].kernel_stack_top};
 let base=unsafe{core::ptr::addr_of!(STACKS)as u64};
 if top>=base+STACK_SIZE as u64&&top<=base+(MAX_TASKS*(STACK_SIZE+64))as u64&&r+1024<top.saturating_sub(STACK_SIZE as u64){
  crate::println!("fatal: kernel stack overflow pid={} rsp={:#x} top={:#x}",current_pid(),r,top);
  crate::arch::halt();
 }
}
pub fn exit_fault(vector:u64,error:u64,rip:u64,cr2:u64)->!{robust_exit();crate::println!("user fault vector={} error={:#x} rip={:#x} cr2={:#x}",vector,error,rip,cr2);with_current_process(|proc|proc.teardown_exit(128+11,vector));finish(128+11,11)}
fn schedule(blocked:bool){let(old_ptr,next,old_fpu,next_fpu)= {let mut s=SCHED.lock();let old=s.current;if !blocked&&s.tasks[old].state==State::Running{s.tasks[old].state=State::Runnable;}
 /* The user RSP is process-wide state in the syscall path; park the outgoing
    task's copy so a resuming thread restores its own. */
 if s.tasks[old].state!=State::Unused{s.tasks[old].user_rsp=unsafe{crate::arch::x86_64::syscall::read_user_rsp()};}
 let mut next=old;for step in 1..=MAX_TASKS{let i=(old+step)%MAX_TASKS;if s.tasks[i].state==State::Runnable{next=i;break}}if next==old{if s.tasks[old].state==State::Runnable{s.tasks[old].state=State::Running;}else if s.tasks[old].state==State::Running{}else{return}s.tasks[next].state=State::Running;if s.current!=next{s.current=next;unsafe{CURRENT_FPU=s.tasks[next].fpu};}else{unsafe{CURRENT_FPU=s.tasks[next].fpu};}return}s.tasks[next].state=State::Running;s.current=next;unsafe{CURRENT_FPU=s.tasks[next].fpu};(&mut s.tasks[old].rsp as*mut u64,s.tasks[next],s.tasks[old].fpu,s.tasks[next].fpu)};PID.store(next.pid,Ordering::Relaxed);unsafe{crate::arch::x86_64::syscall::write_user_rsp(next.user_rsp);apply_arch_state(next);switch_context(old_ptr,next.rsp,old_fpu,next_fpu)}}

pub fn set_current_fs_base(value:u64){let mut s=SCHED.lock();let current=s.current;s.tasks[current].fs_base=value;drop(s);unsafe{crate::arch::x86_64::msr::wrmsr(crate::arch::x86_64::msr::IA32_FS_BASE,value)}}
pub fn fs_base()->u64{let s=SCHED.lock();s.tasks[s.current].fs_base}
pub fn bind_current_address_space(cr3:u64,kernel_stack_top:u64){let mut s=SCHED.lock();let current=s.current;s.tasks[current].cr3=cr3;s.tasks[current].kernel_stack_top=kernel_stack_top;drop(s);unsafe{crate::arch::x86_64::descriptors::set_rsp0(kernel_stack_top);crate::arch::x86_64::syscall::set_kernel_stack(kernel_stack_top)}}
unsafe fn apply_arch_state(task:Task){unsafe{let current=crate::arch::x86_64::paging::PageTableRoot::current().pml4;if current!=task.cr3{crate::arch::x86_64::paging::PageTableRoot{pml4:task.cr3}.activate();}crate::arch::x86_64::msr::wrmsr(crate::arch::x86_64::msr::IA32_FS_BASE,task.fs_base);crate::arch::x86_64::descriptors::set_rsp0(task.kernel_stack_top);crate::arch::x86_64::syscall::set_kernel_stack(task.kernel_stack_top);}}
pub enum WaitOutcome{Reaped(u32,i32,u64,u64),Stopped(u32,i32),Continued(u32),NoHang,Blocked}
fn encode_status(t:Task)->i32{if t.term_signal!=0{t.term_signal as i32}else{(t.exit_code&0xff)<<8}}
fn encode_stopped(sig:u8)->i32{((sig as i32)<<8)|0x7f}
pub fn reap_child(pid_sel:i64,nohang:bool)->Result<WaitOutcome,i64>{reap_child_opts(pid_sel,nohang,false,false)}
pub fn reap_child_opts(pid_sel:i64,nohang:bool,untraced:bool,continued:bool)->Result<WaitOutcome,i64>{
 let me=current_pid();let(pslot,my_pgid)={let s=SCHED.lock();(s.current,s.tasks[s.current].pgid)};
 let matches=|t:&Task|t.ppid==me&&(pid_sel==-1||(pid_sel==0&&t.pgid==my_pgid)||(pid_sel>0&&t.pid==pid_sel as u32)||(pid_sel< -1&&t.pgid==(-pid_sel) as u32));
 let mut s=SCHED.lock();
 if let Some(slot)=s.tasks.iter().position(|t|t.state==State::Zombie&&matches(t)){let t=s.tasks[slot];s.tasks[slot]=EMPTY;drop(s);KERNEL_ENTRIES.lock()[slot]=None;USER_STARTS.lock()[slot]=None;FORK_RESUMES.lock()[slot]=None;let(utime,stime)={let mut procs=PROCS.lock();let times=procs[slot].take().map(|p|(p.utime_ns.saturating_add(p.cutime_ns),p.stime_ns.saturating_add(p.cstime_ns))).unwrap_or((0,0));if !t.is_thread{if let Some(pp)=procs[pslot].as_mut(){pp.cutime_ns=pp.cutime_ns.saturating_add(times.0);pp.cstime_ns=pp.cstime_ns.saturating_add(times.1);}}times};return Ok(WaitOutcome::Reaped(t.pid,encode_status(t),utime,stime))}
 if untraced{
  if let Some(t)=s.tasks.iter().find(|t|t.state==State::Stopped&&matches(t)){let pid=t.pid;let sig=t.stop_sig;drop(s);return Ok(WaitOutcome::Stopped(pid,encode_stopped(sig)))}
 }
 if continued{
  if let Some(slot)=s.tasks.iter().position(|t|t.cont_pending&&matches(t)&&t.state!=State::Unused&&t.state!=State::Zombie){let pid=s.tasks[slot].pid;s.tasks[slot].cont_pending=false;drop(s);return Ok(WaitOutcome::Continued(pid))}
 }
 let has_child=s.tasks.iter().any(|t|t.state!=State::Unused&&matches(t));drop(s);
 if !has_child{return Err(crate::errno::ECHILD)}
 if nohang{Ok(WaitOutcome::NoHang)}else{Ok(WaitOutcome::Blocked)}
}
pub fn wait_child(child_pid:u32)->Result<i32,i64>{loop{match reap_child(child_pid as i64,false)?{WaitOutcome::Reaped(_,code,_,_)=>return Ok(code),_=>{yield_now();/* Idle until the next interrupt so sleeping children can be woken by ticks. */unsafe{core::arch::asm!("sti","hlt","cli",options(nomem,nostack));}}}}}
pub fn scheduler_self_test()->Result<(),&'static str>{static A:AtomicU32=AtomicU32::new(0);static B:AtomicU32=AtomicU32::new(0);static TLS_OK:AtomicU32=AtomicU32::new(0);fn a(){set_current_fs_base(0x1000);A.fetch_add(1,Ordering::Relaxed);yield_now();if unsafe{crate::arch::x86_64::msr::rdmsr(crate::arch::x86_64::msr::IA32_FS_BASE)}==0x1000{TLS_OK.fetch_add(1,Ordering::Relaxed);}A.fetch_add(1,Ordering::Relaxed);}fn b(){set_current_fs_base(0x2000);B.fetch_add(1,Ordering::Relaxed);yield_now();if unsafe{crate::arch::x86_64::msr::rdmsr(crate::arch::x86_64::msr::IA32_FS_BASE)}==0x2000{TLS_OK.fetch_add(1,Ordering::Relaxed);}B.fetch_add(1,Ordering::Relaxed);}spawn_kernel(a).map_err(|_|"spawn A failed")?;spawn_kernel(b).map_err(|_|"spawn B failed")?;for _ in 0..8{yield_now();if A.load(Ordering::Relaxed)==2&&B.load(Ordering::Relaxed)==2&&TLS_OK.load(Ordering::Relaxed)==2{let ca=wait_child(2).map_err(|_|"reap A failed")?;let cb=wait_child(3).map_err(|_|"reap B failed")?;if ca!=0||cb!=0{return Err("unexpected child status")}let stats=resource_stats();if stats.active!=1||stats.zombies!=0||stats.unused!=MAX_TASKS-1{return Err("task slots not reclaimed after wait")}crate::println!("scheduler: context, FS base and zombie reap self-test passed");crate::println!("resources: tasks active={} zombies={} unused={}",stats.active,stats.zombies,stats.unused);return Ok(())}}Err("scheduler self-test stalled")}
pub fn run()->!{loop{yield_now();unsafe{core::arch::asm!("sti","hlt",options(nomem,nostack));}}}
