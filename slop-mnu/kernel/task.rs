//! Shared run-queue scheduler with per-CPU currents and idle loops (MS93).
//!
//! One global task table is shared by every CPU. Each CPU owns a current
//! slot index plus an idle context; runnable tasks are claimed atomically
//! under the scheduler lock, so the same task can never run on two CPUs at
//! once. A CPU with no runnable task parks in its per-CPU idle loop (sti/hlt)
//! and is woken by the timer tick or by a reschedule IPI from a waker.
use core::sync::atomic::{AtomicBool,AtomicU32,AtomicU64,Ordering};
use crate::process::Process;
use crate::sync::IrqSpinLock;
const MAX_TASKS:usize=32;
pub const MAX_TASKS_PUB:usize=MAX_TASKS;
const STACK_SIZE:usize=131072;
/// A task with this pin runs on any CPU; otherwise only on that MADT slot.
pub const PIN_ANY:u32=u32::MAX;
const NO_OWNER:u32=u32::MAX;
/// Sentinel for "this CPU runs no task" (parked in its idle loop).
const IDLE_USIZE:usize=usize::MAX;
const IDLE_U32:u32=u32::MAX;
const NCPU:usize=crate::smp::MAX_CPUS;
/// Per-CPU idle kernel stack (also hosts the fake switch frame).
const AP_IDLE_STACK_SIZE:usize=32768;
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub struct ResourceStats{pub active:usize,pub zombies:usize,pub runnable:usize,pub sleeping:usize,pub unused:usize}
#[repr(align(16))]struct Stack([u8;STACK_SIZE]);
#[derive(Clone,Copy,PartialEq)]pub enum State{Unused,Running,Runnable,Sleeping,Stopped,Zombie}
#[derive(Clone,Copy)]pub struct Task{pub fpu:u64,pub pid:u32,pub tgid:u32,pub ppid:u32,pub pgid:u32,pub sid:u32,pub state:State,pub rsp:u64,pub kernel_stack_top:u64,pub cr3:u64,pub fs_base:u64,pub wake_tick:u64,pub wake_pending:bool,pub exit_code:i32,pub term_signal:u8,pub stop_sig:u8,pub cont_pending:bool,pub stop_epoch:u64,pub cont_epoch:u64,pub is_thread:bool,pub user_rsp:u64,pub sc_frame:u64,pub sc_exec_rip:u64,pub sc_sigret:u64,pub sc_exec_pending:bool,pub sc_sigret_valid:bool,pub pinned:u32}
const EMPTY:Task=Task{fpu:0,pid:0,tgid:0,ppid:0,pgid:0,sid:0,state:State::Unused,rsp:0,kernel_stack_top:0,cr3:0,fs_base:0,wake_tick:0,wake_pending:false,exit_code:0,term_signal:0,stop_sig:0,cont_pending:false,stop_epoch:0,cont_epoch:0,sc_frame:0,sc_exec_rip:0,sc_sigret:0,sc_exec_pending:false,sc_sigret_valid:false,is_thread:false,user_rsp:0,pinned:PIN_ANY};
/* The FPU area of the running task, published for the syscall entry path so
   user SSE registers survive kernel execution. Updated on every switch. */
#[unsafe(no_mangle)]pub static mut CURRENT_FPU:u64=0;
struct Scheduler{tasks:[Task;MAX_TASKS],cpu_current:[usize;NCPU],next_pid:u32}
static SCHED:IrqSpinLock<Scheduler>=IrqSpinLock::new(Scheduler{tasks:[EMPTY;MAX_TASKS],cpu_current:[IDLE_USIZE;NCPU],next_pid:2});
static NEXT_NS:AtomicU64=AtomicU64::new(2);
static mut UTS_NAMESPACES:[[u8;65];16]=[[0;65];16];
fn uts_init(){unsafe{UTS_NAMESPACES[1][..4].copy_from_slice(b"slop");}}
/* One Process per user task slot; kernel-only tasks leave their entry empty. */
static PROCS:IrqSpinLock<[Option<Process>;MAX_TASKS]>=IrqSpinLock::new([const{None};MAX_TASKS]);
static mut STACKS:[Stack;MAX_TASKS]=[const{Stack([0;STACK_SIZE])};MAX_TASKS];
static NEED_RESCHED:[AtomicBool;NCPU]=[const{AtomicBool::new(false)};NCPU];
static PID:AtomicU32=AtomicU32::new(1);
/* Lock-free mirror of each CPU's current task slot (IDLE_U32 when idle). */
static CPU_CUR:[AtomicU32;NCPU]=[const{AtomicU32::new(IDLE_U32)};NCPU];
/* Which CPU owns a Running task (NO_OWNER otherwise). Set and cleared under
   the scheduler lock; a claim that finds an owner counts a double-exec. */
static TASK_OWNER:[AtomicU32;MAX_TASKS]=[const{AtomicU32::new(NO_OWNER)};MAX_TASKS];
/* CPUs with scheduler state installed (BSP at init, APs at ap_enter). */
static CPU_ACTIVE:[AtomicBool;NCPU]=[const{AtomicBool::new(false)};NCPU];
/* Bitmask of CPUs parked in their idle loop, waiting for work. */
static IDLE_MASK:AtomicU64=AtomicU64::new(0);
/* Resume points of the per-CPU idle loops (primed with a fake switch frame). */
static mut IDLE_CTX_RSP:[u64;NCPU]=[0;NCPU];
static mut AP_IDLE_STACKS:[[u8;AP_IDLE_STACK_SIZE];NCPU]=[[0;AP_IDLE_STACK_SIZE];NCPU];
/* Kernel CR3 used while a CPU idles (all user CR3s map the kernel too). */
static KERNEL_CR3:AtomicU64=AtomicU64::new(0);
/* Run-queue accounting: every transition into Runnable bumps PUSH, every
   claim out of Runnable bumps POP. Quiescent: PUSH-POP == #Runnable. */
static RUNQ_PUSH:AtomicU64=AtomicU64::new(0);
static RUNQ_POP:AtomicU64=AtomicU64::new(0);
static DOUBLE_EXEC:AtomicU64=AtomicU64::new(0);
static AP_USER_RUN:AtomicU64=AtomicU64::new(0);
static CPU_TASKS_RUN:[AtomicU64;NCPU]=[const{AtomicU64::new(0)};NCPU];
static CPU_CTX:[AtomicU64;NCPU]=[const{AtomicU64::new(0)};NCPU];
static CPU_SYSCALL:[AtomicU64;NCPU]=[const{AtomicU64::new(0)};NCPU];
static CPU_IPI_SENT:[AtomicU64;NCPU]=[const{AtomicU64::new(0)};NCPU];
static CPU_IPI_RECV:[AtomicU64;NCPU]=[const{AtomicU64::new(0)};NCPU];
static CPU_IDLE_LOOPS:[AtomicU64;NCPU]=[const{AtomicU64::new(0)};NCPU];
static SWITCH_OLD:[AtomicU32;NCPU]=[const{AtomicU32::new(IDLE_U32)};NCPU];
static SWITCH_YIELD:[AtomicBool;NCPU]=[const{AtomicBool::new(false)};NCPU];
static CPU_HLTS:[AtomicU64;NCPU]=[const{AtomicU64::new(0)};NCPU];
static CPU_TICKS:[AtomicU64;NCPU]=[const{AtomicU64::new(0)};NCPU];
static CPU_TSTAGE:[AtomicU64;NCPU]=[const{AtomicU64::new(0)};NCPU];
pub fn note_timer_tick(){let c=cpu();if c<NCPU{CPU_TICKS[c].fetch_add(1,Ordering::Relaxed);}}
pub fn note_timer_stage(s:u64){let c=cpu();if c<NCPU{CPU_TSTAGE[c].store((CPU_TICKS[c].load(Ordering::Relaxed)<<8)|s,Ordering::Relaxed);}}
unsafe extern "sysv64"{fn switch_context(old:*mut u64,new:u64,old_fpu:u64,new_fpu:u64);fn switch_context_nosave(new:u64,old_fpu:u64,new_fpu:u64);}
/* FXSAVE/FXRSTOR helpers: the FPU context lives in a 512 byte area per task
   so floating point and SSE registers survive a context switch. */
const FPU_AREA:usize=256;
/// Per-slot SSE register file, kept out of the task stacks so saving the
/// registers can never overwrite a live stack.
#[repr(align(16))]
struct FpuArea([u8;FPU_AREA]);
static mut FPU_AREAS:[FpuArea;MAX_TASKS]=[const{FpuArea([0;FPU_AREA])};MAX_TASKS];
static mut IDLE_FPU:[FpuArea;NCPU]=[const{FpuArea([0;FPU_AREA])};NCPU];
/* The FXSAVE area must be 16-byte aligned and must not straddle a page,
   so it sits at the page-aligned bottom of the task stack, which the growing
   stack reaches last. */
fn fpu_area(slot:usize)->u64{unsafe{core::ptr::addr_of!(FPU_AREAS[slot])as u64}}
fn idle_fpu(slot:usize)->u64{unsafe{core::ptr::addr_of!(IDLE_FPU[slot])as u64}}
unsafe extern "sysv64"{fn fpu_save_regs(area:*mut u8);fn fpu_restore_regs(area:*const u8);}
fn fpu_save(addr:u64){let _=addr;}
unsafe fn task_restore_fpu(area:u64){unsafe{fpu_restore_regs(area as *const u8)}}
/* Running CPU as a MADT slot index. Uses the APIC-ID scan (same source as
   smp::current_cpu); GS already pins the per-CPU block, but the slot
   index lives in the MADT table. */
#[inline]fn cpu()->usize{
 let s=crate::smp::current_cpu();
 if s<NCPU{s}else{crate::smp::bsp_slot()}
}
#[inline]fn my(s:&Scheduler)->usize{s.cpu_current[cpu()]}
fn idle_top(slot:usize)->u64{(unsafe{&raw const AP_IDLE_STACKS[slot] as u64}+AP_IDLE_STACK_SIZE as u64)&!15}
fn idle_ctx_ptr(slot:usize)->*mut u64{unsafe{&raw mut IDLE_CTX_RSP[slot]}}
pub fn init(bootstrap_stack_top:u64){
 uts_init();fpu_save(fpu_area(0));
 let bsp=cpu();
 prime_idle(bsp);
 let mut s=SCHED.lock();
 let cr3=crate::arch::x86_64::paging::PageTableRoot::current().pml4;
 KERNEL_CR3.store(cr3,Ordering::Relaxed);
 s.tasks[0]=Task{fpu:fpu_area(0),pid:1,tgid:1,ppid:0,pgid:1,sid:1,state:State::Running,rsp:0,kernel_stack_top:bootstrap_stack_top,cr3,fs_base:0,wake_tick:0,wake_pending:false,exit_code:0,term_signal:0,stop_sig:0,cont_pending:false,stop_epoch:0,cont_epoch:0,sc_frame:0,sc_exec_rip:0,sc_sigret:0,sc_exec_pending:false,sc_sigret_valid:false,is_thread:false,user_rsp:0,pinned:bsp as u32};
 s.cpu_current[bsp]=0;
 TASK_OWNER[0].store(bsp as u32,Ordering::Release);
 CPU_CUR[bsp].store(0,Ordering::Release);
 CPU_ACTIVE[bsp].store(true,Ordering::Release);
 unsafe{(*crate::smp::per_cpu_ptr(bsp)).current_fpu=fpu_area(0);(*crate::smp::per_cpu_ptr(bsp)).cpu_index=bsp as u64;}
 crate::println!("task: scheduler bootstrap pid=1 capacity={} cpus={}",MAX_TASKS,NCPU);
 drop(s);
 crate::smp::set_sched_ready(true);
}
/* Restores the incoming task's parked per-CPU syscall cells (see the park
   in schedule()). Runs on the incoming stack just before the switch. */
unsafe fn restore_task_state(nt:Task){unsafe{
 crate::arch::x86_64::syscall::write_user_rsp(nt.user_rsp);
 crate::arch::x86_64::syscall::write_syscall_frame(nt.sc_frame);
 crate::arch::x86_64::syscall::write_exec_rip(nt.sc_exec_rip);
 crate::arch::x86_64::syscall::write_exec_pending(nt.sc_exec_pending);
 crate::arch::x86_64::syscall::write_sigret_value(nt.sc_sigret);
 crate::arch::x86_64::syscall::write_sigret_valid(nt.sc_sigret_valid);
}}
/* Builds the fake switch frame on a CPU's idle stack so the first
   task->idle switch has a valid resume point (6 zeroed registers and the
   address of idle_resume, matching switch_context's pop sequence). */
fn prime_idle(slot:usize){
 let top=idle_top(slot);
 unsafe{
  core::ptr::write((top-8) as *mut u64,idle_resume as usize as u64);
  core::ptr::write_bytes((top-56) as *mut u8,0,48);
  IDLE_CTX_RSP[slot]=top-56;
 }
}
#[unsafe(no_mangle)]pub extern "sysv64" fn idle_resume()->!{let c=cpu();idle_loop(c)}
/* Per-CPU idle loop: claim runnable tasks or halt until the timer tick or a
   reschedule IPI wakes this CPU. Never returns. */
extern "sysv64" fn idle_loop(slot:usize)->!{
 crate::println!("smp: cpu{} idle_loop={:#x} halt={:#x} sched={:#x} resched={:#x} timer={:#x} rsp={:#x} expect_top={:#x}",slot,idle_loop as usize,crate::arch::halt as usize,schedule as usize,crate::arch::x86_64::apic::apic_resched_rust as usize,crate::arch::x86_64::apic::apic_timer_rust as usize,{
  let r:u64;unsafe{core::arch::asm!("mov {}, rsp",out(reg)r,options(nostack,preserves_flags))};r
 },idle_top(slot));
 crate::println!("smp: cpu{} slot={} current_cpu={} apic={:#x}",slot,slot,crate::smp::current_cpu(),crate::arch::x86_64::apic::apic_id());
  loop{
   CPU_IDLE_LOOPS[slot].fetch_add(1,Ordering::Relaxed);
   IDLE_MASK.fetch_or(1u64<<slot,Ordering::Relaxed);
   /* Claim one runnable task, if any. A task that another CPU is still
      switching away from stays Running and owned until its post-switch
      hook runs, so it is skipped here and picked up on a later pass. */
   let claimed:Option<Task>={let mut s=SCHED.lock();claim_locked(&mut s,slot,slot.wrapping_mul(7)+1)};
   if let Some(nt)=claimed{
   IDLE_MASK.fetch_and(!(1u64<<slot),Ordering::Relaxed);
   PID.store(nt.pid,Ordering::Relaxed);
   unsafe{restore_task_state(nt);apply_arch_state(slot,nt);
    switch_context(idle_ctx_ptr(slot),nt.rsp,idle_fpu(slot),nt.fpu)};
   CPU_CUR[slot].store(IDLE_U32,Ordering::Release);
  }else{
   NEED_RESCHED[slot].store(false,Ordering::Relaxed);
   CPU_HLTS[slot].fetch_add(1,Ordering::Relaxed);
   unsafe{core::arch::asm!("sti","hlt",options(nomem,nostack,preserves_flags));}
  }
 }
}
/* Entry for an application processor leaving the parked state: per-CPU TSS,
   syscall MSRs and timer, then a jump into the idle loop on the per-CPU
   idle stack. Never returns. */
pub fn ap_enter(slot:usize)->!{
 prime_idle(slot);
 let top=idle_top(slot);
 crate::smp::ap_sched_stack(slot,top);
 unsafe{crate::arch::x86_64::syscall::init(top)};
 crate::arch::x86_64::apic::ap_timer_start();
 crate::println!("smp: cpu{} lvt_timer={:#x} apic_id={:#x}",slot,crate::arch::x86_64::apic::lvt_timer(),crate::arch::x86_64::apic::apic_id());
 unsafe{
  let tr:u16;core::arch::asm!("str {o}",o=out(reg)tr,options(nostack,preserves_flags));
  let (r0,i0,i1)=crate::smp::tss_diag(slot);
  let (ta,gb,gl)=crate::smp::gdt_diag(slot);
  crate::println!("smp: cpu{} tr={:#x} tss_rsp0={:#x} tss_ist0={:#x} tss_ist1={:#x} idle_top={:#x}",
   slot,tr,r0,i0,i1,top);
  crate::println!("smp: cpu{} tss_addr={:#x} gdt_base={:#x} gdt_lim={:#x}",slot,ta,gb,gl);
 }
 unsafe{(*crate::smp::per_cpu_ptr(slot)).cpu_index=slot as u64;}
 {let mut s=SCHED.lock();s.cpu_current[slot]=IDLE_USIZE;}
 CPU_CUR[slot].store(IDLE_U32,Ordering::Release);
 CPU_ACTIVE[slot].store(true,Ordering::Release);
 core::sync::atomic::fence(Ordering::SeqCst);
 crate::println!("smp: cpu{} entering scheduler idle",slot);
 unsafe{
  let (cs0,ss0):(u64,u64);
  core::arch::asm!("mov {o}, cs",o=out(reg)cs0,options(nostack,preserves_flags));
  core::arch::asm!("mov {o}, ss",o=out(reg)ss0,options(nostack,preserves_flags));
  crate::println!("smp: cpu{} ap_enter cs={:#x} ss={:#x}",slot,cs0,ss0);
 }
 /* Enter with rsp%16==8 (as if called) so the ABI holds: the prologue's
    pushes then keep every movaps-aligned spill aligned. */
 let entry_sp=top.wrapping_sub(8);
 unsafe{core::arch::asm!("mov rsp, {top}","jmp {idle}",top=in(reg)entry_sp,idle=sym idle_loop,in("rdi")slot,options(noreturn))}
}
/* Claims one runnable task for `cpu` under the scheduler lock: marks it
   Running, records ownership and per-CPU state, bumps run-queue stats. */
fn claim_locked(s:&mut Scheduler,cpu:usize,hint:usize)->Option<Task>{
 for step in 0..MAX_TASKS{
  let i=(hint+step)%MAX_TASKS;
  if s.tasks[i].state!=State::Runnable{continue}
  if s.tasks[i].pinned!=PIN_ANY&&s.tasks[i].pinned!=cpu as u32{continue}
  let own_i=TASK_OWNER[i].load(Ordering::Acquire);
  if own_i!=NO_OWNER{if skip_owned(s,i,own_i){DOUBLE_EXEC.fetch_add(1,Ordering::Relaxed);}continue}
  s.tasks[i].state=State::Running;
  TASK_OWNER[i].store(cpu as u32,Ordering::Release);
  s.cpu_current[cpu]=i;
  CPU_CUR[cpu].store(i as u32,Ordering::Release);
   RUNQ_POP.fetch_add(1,Ordering::Relaxed);
   CPU_TASKS_RUN[cpu].fetch_add(1,Ordering::Relaxed);
   /* DIAG: validate the claimed task's stack pointer (must be a kernel address). */
  let rsp=s.tasks[i].rsp;
  if rsp<0x10000 {
   crate::println!("smp: CORRUPT tslot={} pid={} rsp={:#x} state={} owner={:#x}",
    i,s.tasks[i].pid,rsp,s.tasks[i].state as u8,TASK_OWNER[i].load(Ordering::Relaxed));
   crate::arch::halt();
  }
  unsafe{(*crate::smp::per_cpu_ptr(cpu)).current_fpu=s.tasks[i].fpu};
  return Some(s.tasks[i]);
 }
 None
}
/* Requests a reschedule on one idle, scheduler-active CPU so a freshly woken
   task starts without waiting for that CPU's next timer tick. */
pub fn poke_idle(except:usize){
 let mask=IDLE_MASK.load(Ordering::Relaxed);
 if mask==0{return}
 let from=except.min(NCPU-1);
 for slot in 0..NCPU{
  if slot==except{continue}
  if mask&(1u64<<slot)==0{continue}
  if !CPU_ACTIVE[slot].load(Ordering::Relaxed){continue}
  let apic=crate::smp::apic_id_of_slot(slot);
  if apic==0xFFFF_FFFF{continue}
  CPU_IPI_SENT[from].fetch_add(1,Ordering::Relaxed);
  if !crate::arch::x86_64::apic::send_resched_ipi(apic){
   crate::println!("smp: ipi to cpu{} apic={:#x} failed",slot,apic);
  }
  break;
 }
}
fn any_runnable()->bool{let s=SCHED.lock();s.tasks.iter().any(|t|t.state==State::Runnable)}
/* The identifier of the running task. The scheduler is the single source of
   truth: a cached copy could name a task that ran in between, and a signal
   aimed at the wrong thread would be lost. */
pub fn current_pid()->u32{let s=SCHED.lock();let m=s.cpu_current[cpu()];if m==IDLE_USIZE{return 0}s.tasks[m].pid}
pub fn current_ppid()->u32{let s=SCHED.lock();let m=s.cpu_current[cpu()];if m==IDLE_USIZE{return 0}s.tasks[m].ppid}
pub fn current_pgid()->u32{let s=SCHED.lock();let m=s.cpu_current[cpu()];if m==IDLE_USIZE{return 0}s.tasks[m].pgid}
pub fn current_sid()->u32{let s=SCHED.lock();let m=s.cpu_current[cpu()];if m==IDLE_USIZE{return 0}s.tasks[m].sid}
pub fn alloc_ns()->u64{NEXT_NS.fetch_add(1,Ordering::Relaxed)}
pub fn uts_hostname(ns:u64)->[u8;65]{let idx=(ns as usize)%16;unsafe{if UTS_NAMESPACES[idx][0]==0{let mut d=[0u8;65];d[..4].copy_from_slice(b"slop");d}else{UTS_NAMESPACES[idx]}}}
pub fn set_uts_hostname(ns:u64,name:&[u8]){let idx=(ns as usize)%16;unsafe{UTS_NAMESPACES[idx]=[0;65];let n=name.len().min(64);UTS_NAMESPACES[idx][..n].copy_from_slice(&name[..n]);}}
pub fn alloc_uts_ns(src:u64)->u64{let id=alloc_ns();let idx=(id as usize)%16;let srcidx=(src as usize)%16;unsafe{UTS_NAMESPACES[idx]=UTS_NAMESPACES[srcidx];}id}
pub fn sched_pids()->[u32;MAX_TASKS]{let s=SCHED.lock();let mut p=[0;MAX_TASKS];for i in 0..MAX_TASKS{p[i]=s.tasks[i].pid};p}
pub fn current_kernel_stack_top()->u64{let s=SCHED.lock();let m=s.cpu_current[cpu()];if m==IDLE_USIZE{return idle_top(cpu())}s.tasks[m].kernel_stack_top}
pub fn resource_stats()->ResourceStats{
 let s=SCHED.lock();let mut stats=ResourceStats::default();
  for task in &s.tasks{match task.state{State::Unused=>stats.unused+=1,State::Zombie=>stats.zombies+=1,State::Runnable=>{stats.active+=1;stats.runnable+=1},State::Sleeping=>{stats.active+=1;stats.sleeping+=1},State::Stopped=>{stats.active+=1},State::Running=>stats.active+=1}}
 stats
}
/* Locks are never nested the other way around: the scheduler guard is released
   before PROCS is taken, so closure bodies may lock anything but SCHED. */
pub fn with_current_process<R>(f:impl FnOnce(&mut Process)->R)->Option<R>{let m=SCHED.lock().cpu_current[cpu()];if m==IDLE_USIZE{return None}let mut procs=PROCS.lock();procs[m].as_mut().map(f)}
pub fn has_current_process()->bool{let m=SCHED.lock().cpu_current[cpu()];if m==IDLE_USIZE{return false}PROCS.lock()[m].is_some()}
/* Immutable view of another live process, for /proc/<pid> introspection. */
pub fn with_process_pid<R>(pid:u32,f:impl FnOnce(&Process)->R)->Option<R>{let slot={let s=SCHED.lock();(0..MAX_TASKS).find(|&i|s.tasks[i].pid==pid&&s.tasks[i].state!=State::Unused)?};let procs=PROCS.lock();procs[slot].as_ref().map(f)}
pub fn live_pids(mut cb:impl FnMut(u32)){let s=SCHED.lock();let mut seen=[0u32;MAX_TASKS];let mut n=0usize;for t in &s.tasks{if t.state==State::Unused||t.is_thread||seen[..n].contains(&t.pid){continue}seen[n]=t.pid;n+=1;cb(t.pid);}}
pub fn thread_count(pid:u32)->usize{let s=SCHED.lock();s.tasks.iter().filter(|t|t.pid==pid&&t.state!=State::Unused).count().max(1)}
pub fn ppid_of(pid:u32)->u32{let s=SCHED.lock();s.tasks.iter().find(|t|t.pid==pid&&t.state!=State::Unused).map(|t|t.ppid).unwrap_or(0)}
pub fn last_pid()->u32{let s=SCHED.lock();s.next_pid.saturating_sub(1).max(1)}
/* Lock order: the scheduler lock is a leaf and is never held across any
   other lock. Spawn paths reserve a slot (Sleeping+owned: invisible to
   claimers, which need Runnable, and to slot scans, which need Unused),
   fill the per-slot tables with the lock released, then publish the task.
   The reverse order (exit/teardown holds the process table across
   scheduler reads) is the only allowed nesting; inverting it deadlocked
   SMP fork/exit bursts (all CPUs spinning on the two locks). */
fn try_take_slot(s:&Scheduler)->Option<usize>{
 (1..MAX_TASKS).find(|&i|s.tasks[i].state==State::Unused)
}
/* Reserve one slot for the calling CPU without taking any other lock.
   A dead thread's slot is reclaimed first (reaped with the lock released).
   Returns (slot, pid). The slot stays Sleeping+owned until publish_spawn. */
fn reserve_slot(c:usize)->Result<(usize,u32),i64>{
 if let Some(slot)={let mut s=SCHED.lock();
  if let Some(i)=try_take_slot(&s){
   let pid=alloc_pid(&mut s);
   /* Sleeping+owned+cleared: invisible to claimers (need Runnable), slot
      scans (need Unused) and pid-based wakers. The slot must be cleared
      wholesale: a recycled occupant may leave wake_pending set, and the
      timer would otherwise spuriously wake the reservation mid-fill. */
   s.tasks[i]=EMPTY;s.tasks[i].state=State::Sleeping;s.tasks[i].wake_tick=u64::MAX;TASK_OWNER[i].store(c as u32,Ordering::Release);
   Some((i,pid))
  }else{None}
 }{return Ok(slot)}
 let victim=reap_thread_slot(c)?;
 let mut s=SCHED.lock();
 /* The victim was reserved by reap_thread_slot; claim the reservation. */
 if TASK_OWNER[victim].load(Ordering::Acquire)!=c as u32{return Err(crate::errno::EAGAIN)}
 let pid=alloc_pid(&mut s);
 Ok((victim,pid))
}
/* Reclaims one dead thread slot for reuse (threads are joined in userspace;
   the kernel never waits for them). The victim is reserved under the
   scheduler lock, then its table entries are dropped with the lock
   released. Its CPU time folds into its thread-group leader, matching
   Linux's per-process accounting. */
fn reap_thread_slot(c:usize)->Result<usize,i64>{
 let (victim,tgid)={
  let mut s=SCHED.lock();
  match (1..MAX_TASKS).find(|&i|s.tasks[i].state==State::Zombie&&s.tasks[i].is_thread){
   Some(i)=>{let t=s.tasks[i];s.tasks[i]=EMPTY;s.tasks[i].state=State::Sleeping;s.tasks[i].wake_tick=u64::MAX;TASK_OWNER[i].store(c as u32,Ordering::Release);(i,t.tgid)}
   None=>return Err(crate::errno::EAGAIN),
  }
  };
 KERNEL_ENTRIES.lock()[victim]=None;USER_STARTS.lock()[victim]=None;FORK_RESUMES.lock()[victim]=None;
 let (ut,st,ct,cst)={let mut procs=PROCS.lock();match procs[victim].take(){Some(p)=>(p.utime_ns,p.stime_ns,p.cutime_ns,p.cstime_ns),_=>(0,0,0,0)}};
 {
  let mut procs=PROCS.lock();
  let lead={let s=SCHED.lock();(1..MAX_TASKS).find(|&j|s.tasks[j].pid==tgid&&s.tasks[j].state!=State::Unused&&!s.tasks[j].is_thread)};
  if let Some(j)=lead{if let Some(p)=procs[j].as_mut(){
   p.utime_ns=p.utime_ns.saturating_add(ut);p.stime_ns=p.stime_ns.saturating_add(st);
   p.cutime_ns=p.cutime_ns.saturating_add(ct);p.cstime_ns=p.cstime_ns.saturating_add(cst);}}
 }
 Ok(victim)
}
/* Publishes a reserved slot as a runnable task. Must run after the
   per-slot tables were filled with the scheduler lock released. The slot
   starts unowned (claims assign ownership); the Sleeping+owned reservation
   was only valid until this handoff. */
fn publish_spawn(slot:usize,task:Task){
 TASK_OWNER[slot].store(NO_OWNER,Ordering::Release);
 let mut s=SCHED.lock();
 debug_assert!(s.tasks[slot].state==State::Sleeping&&s.tasks[slot].pid==0);
 s.tasks[slot]=task;
 RUNQ_PUSH.fetch_add(1,Ordering::Relaxed);
}
fn alloc_pid(s:&mut Scheduler)->u32{let pid=s.next_pid;s.next_pid=s.next_pid.wrapping_add(1).max(2);pid}
fn build_kernel_stack(slot:usize,entry:u64)->u64{let top=unsafe{core::ptr::addr_of!(STACKS[slot].0)as u64+STACK_SIZE as u64};let mut sp=(top&!15)-8;unsafe{sp-=8;*(sp as*mut u64)=entry;for _ in 0..6{sp-=8;*(sp as*mut u64)=0;}}sp}
pub fn spawn_kernel(entry:fn())->Result<u32,i64>{
 let c=cpu();
 let (ppid,pgid,sid)={let s=SCHED.lock();let me=s.cpu_current[c];if me==IDLE_USIZE{(0,0,0)}else{(s.tasks[me].pid,s.tasks[me].pgid,s.tasks[me].sid)}};
 let (slot,pid)=reserve_slot(c)?;
 let top=unsafe{core::ptr::addr_of!(STACKS[slot].0)as u64+STACK_SIZE as u64};let sp=build_kernel_stack(slot,kernel_thread_trampoline as *const () as u64);fpu_save(fpu_area(slot));
 KERNEL_ENTRIES.lock()[slot]=Some(entry);
 let cr3=crate::arch::x86_64::paging::PageTableRoot::current().pml4;
 publish_spawn(slot,Task{fpu:fpu_area(slot),pid,tgid:pid,ppid,pgid,sid,state:State::Runnable,rsp:sp,kernel_stack_top:top,cr3,fs_base:0,wake_tick:0,wake_pending:false,exit_code:0,term_signal:0,stop_sig:0,cont_pending:false,stop_epoch:0,cont_epoch:0,sc_frame:0,sc_exec_rip:0,sc_sigret:0,sc_exec_pending:false,sc_sigret_valid:false,is_thread:false,user_rsp:0,pinned:crate::smp::bsp_slot() as u32});
 poke_idle(c);Ok(pid)
}
static KERNEL_ENTRIES:IrqSpinLock<[Option<fn()>;MAX_TASKS]>=IrqSpinLock::new([None;MAX_TASKS]);
static USER_STARTS:IrqSpinLock<[Option<crate::exec_runtime::Loaded>;MAX_TASKS]>=IrqSpinLock::new([None;MAX_TASKS]);
static FORK_RESUMES:IrqSpinLock<[Option<crate::arch::x86_64::syscall::UserFrame>;MAX_TASKS]>=IrqSpinLock::new([const{None};MAX_TASKS]);
pub fn spawn_user(loaded:crate::exec_runtime::Loaded)->Result<u32,i64>{
 let c=cpu();
 let mut proc=Process::new(loaded.cr3,loaded.bootstrap_cr3,loaded.image_slot,loaded.stack_slot,loaded.interp_slot)?;proc.set_exec_meta(&loaded);
 let (ppid,pgid,sid)={let s=SCHED.lock();let me=s.cpu_current[c];if me==IDLE_USIZE{(0,0,0)}else{(s.tasks[me].pid,s.tasks[me].pgid,s.tasks[me].sid)}};
 let (slot,pid)=reserve_slot(c)?;
 let top=unsafe{core::ptr::addr_of!(STACKS[slot].0)as u64+STACK_SIZE as u64};let sp=build_kernel_stack(slot,user_thread_trampoline as *const () as u64);fpu_save(fpu_area(slot));
 PROCS.lock()[slot]=Some(proc);USER_STARTS.lock()[slot]=Some(loaded);
 publish_spawn(slot,Task{fpu:fpu_area(slot),pid,tgid:pid,ppid,pgid,sid,state:State::Runnable,rsp:sp,kernel_stack_top:top,cr3:loaded.cr3,fs_base:0,wake_tick:0,wake_pending:false,exit_code:0,term_signal:0,stop_sig:0,cont_pending:false,stop_epoch:0,cont_epoch:0,sc_frame:0,sc_exec_rip:0,sc_sigret:0,sc_exec_pending:false,sc_sigret_valid:false,is_thread:false,user_rsp:0,pinned:PIN_ANY});
 poke_idle(c);Ok(pid)
}
pub fn spawn_fork(mut child:Process,frame:crate::arch::x86_64::syscall::UserFrame)->Result<u32,i64>{
 let c=cpu();
 let child_cr3=child.cr3;
 let (ppid,pgid,sid,fs_base)={let s=SCHED.lock();let me=s.cpu_current[c];if me==IDLE_USIZE{(0,0,0,0)}else{(s.tasks[me].pid,s.tasks[me].pgid,s.tasks[me].sid,s.tasks[me].fs_base)}};
 let (slot,pid)=match reserve_slot(c){Ok(v)=>v,Err(e)=>{child.abandon();return Err(e)}};
 let top=unsafe{core::ptr::addr_of!(STACKS[slot].0)as u64+STACK_SIZE as u64};let sp=build_kernel_stack(slot,fork_thread_trampoline as *const () as u64);fpu_save(fpu_area(slot));
 FORK_RESUMES.lock()[slot]=Some(frame);PROCS.lock()[slot]=Some(child);
 publish_spawn(slot,Task{fpu:fpu_area(slot),pid,tgid:pid,ppid,pgid,sid,state:State::Runnable,rsp:sp,kernel_stack_top:top,cr3:child_cr3,fs_base,wake_tick:0,wake_pending:false,exit_code:0,term_signal:0,stop_sig:0,cont_pending:false,stop_epoch:0,cont_epoch:0,sc_frame:0,sc_exec_rip:0,sc_sigret:0,sc_exec_pending:false,sc_sigret_valid:false,is_thread:false,user_rsp:0,pinned:PIN_ANY});
 poke_idle(c);Ok(pid)
}
pub fn fork_process(frame:crate::arch::x86_64::syscall::UserFrame,flags:u64)->Result<u32,i64>{let child_cr3=match with_current_process(|p|p.clone_address_space()){Some(Ok(v))=>v,Some(Err(e))=>return Err(e),None=>return Err(crate::errno::ESRCH)};let mut child=match with_current_process(|p|p.fork_child(child_cr3)){Some(Ok(v))=>v,Some(Err(e))=>return Err(e),None=>return Err(crate::errno::ESRCH)};if let Err(e)=apply_ns_flags(&mut child,flags){child.abandon();return Err(e)}spawn_fork(child,frame)}
extern "sysv64" fn user_thread_trampoline()->!{
 let c=cpu();
 let slot=SCHED.lock().cpu_current[c];let loaded=USER_STARTS.lock()[slot].take().unwrap();
 if c!=crate::smp::bsp_slot(){AP_USER_RUN.fetch_add(1,Ordering::Relaxed);}
 crate::println!("task: entering user pid={} rip={:#x} rsp={:#x} cpu={} slot={}",current_pid(),loaded.entry,loaded.rsp,c,slot);
 unsafe{crate::arch::x86_64::user::enter(loaded.entry,loaded.rsp)}
}
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
 let c=cpu();
 let (slot,pid)=match reserve_slot(c){Ok(v)=>v,Err(e)=>{child.abandon();return Err(e)}};
 let (tgid,ppid,pgid,sid)={let s=SCHED.lock();let me=s.cpu_current[c];if me==IDLE_USIZE{(pid,0,0,0)}else{let m=&s.tasks[me];(m.tgid,m.pid,m.pgid,m.sid)}};
 let top=unsafe{core::ptr::addr_of!(STACKS[slot].0)as u64+STACK_SIZE as u64};let sp=build_kernel_stack(slot,fork_thread_trampoline as *const () as u64);fpu_save(fpu_area(slot));
 FORK_RESUMES.lock()[slot]=Some(frame);PROCS.lock()[slot]=Some(child);
 publish_spawn(slot,Task{fpu:fpu_area(slot),pid,tgid,ppid,pgid,sid,state:State::Runnable,rsp:sp,kernel_stack_top:top,cr3,fs_base:tls,wake_tick:0,wake_pending:false,exit_code:0,term_signal:0,stop_sig:0,cont_pending:false,stop_epoch:0,cont_epoch:0,sc_frame:0,sc_exec_rip:0,sc_sigret:0,sc_exec_pending:false,sc_sigret_valid:false,is_thread:true,user_rsp:child_stack,pinned:PIN_ANY});
 poke_idle(c);
 /* CLONE_PARENT_SETTID: store the child TID in the parent's memory (same
    address space here); CLONE_CHILD_SETTID: store it in the child's copy. */
 let tidb=pid.to_ne_bytes();
 if ns_flags&CLONE_PARENT_SETTID!=0&&ptid!=0{let _=crate::usercopy::copy_to_user(ptid as usize,&tidb);}
 if ns_flags&CLONE_CHILD_SETTID!=0&&ctid!=0{let _=crate::usercopy::copy_to_user(ctid as usize,&tidb);}
 Ok(pid)
}
extern "sysv64" fn fork_thread_trampoline()->!{
 let c=cpu();
 let slot=SCHED.lock().cpu_current[c];
 let frame_opt=FORK_RESUMES.lock()[slot].take();
 let frame=match frame_opt{Some(f)=>f,None=>{
  let (pid,state,owner)= {let s=SCHED.lock();(s.tasks[slot].pid,s.tasks[slot].state as u8,TASK_OWNER[slot].load(Ordering::Relaxed))};
  crate::println!("smp: DOUBLE-TRAMP cpu={} slot={} pid={} state={} owner={:#x} cur0={} cur1={}",
   c,slot,pid,state,owner,SCHED.lock().cpu_current[0],SCHED.lock().cpu_current[1]);
  crate::arch::halt();
 }};
 if c!=crate::smp::bsp_slot(){AP_USER_RUN.fetch_add(1,Ordering::Relaxed);}
 crate::println!("task: fork child resumed pid={} cpu={}",current_pid(),c);
 unsafe{crate::arch::x86_64::user::resume(&frame)}
}
extern "sysv64" fn kernel_thread_trampoline()->!{
 let c=cpu();let slot=SCHED.lock().cpu_current[c];
 crate::println!("smp: cpu{} kthread-tramp slot={}",c,slot);
 let entry_opt=KERNEL_ENTRIES.lock()[slot].take();
 let entry=match entry_opt{Some(f)=>f,None=>{
  let (pid,state,owner)= {let s=SCHED.lock();(s.tasks[slot].pid,s.tasks[slot].state as u8,TASK_OWNER[slot].load(Ordering::Relaxed))};
  crate::println!("smp: DOUBLE-KTHREAD cpu={} slot={} pid={} state={} owner={:#x} cur0={} cur1={}",
   c,slot,pid,state,owner,SCHED.lock().cpu_current[0],SCHED.lock().cpu_current[1]);
  crate::arch::halt();
 }};
 unsafe{core::arch::asm!("sti",options(nomem,nostack,preserves_flags));}entry();finish_current(0)}
pub fn yield_now(){schedule(false)}
pub fn on_timer_tick(user:bool,delta_ns:u64){
 let c=cpu();
 let now=crate::arch::x86_64::apic::ticks();account_tick(user,delta_ns);wake_sleepers(now);crate::posix_timer::tick(now);
}
/* Attributes one 1ms tick to the running process: CS&3 told us whether the
   interrupted context was user or kernel code. */
fn account_tick(user:bool,delta_ns:u64){
 let(slot,pid)={let s=SCHED.lock();let m=s.cpu_current[cpu()];if m==IDLE_USIZE{return}(m,s.tasks[m].pid)};{let mut procs=PROCS.lock();if let Some(p)=procs[slot].as_mut(){if user{if p.utime_ns<u64::MAX{p.utime_ns=p.utime_ns.saturating_add(delta_ns)}}else if p.stime_ns<u64::MAX{p.stime_ns=p.stime_ns.saturating_add(delta_ns)}}}crate::posix_timer::cpu_tick(pid,user);}
pub fn take_need_resched()->bool{let c=cpu();NEED_RESCHED[c].swap(false,Ordering::AcqRel)}
/* Marks a reschedule pending on the calling CPU (reschedule-IPI handler).
   Idle CPUs observe it after hlt; running tasks observe it at yield points. */
pub fn request_resched(){let c=cpu();NEED_RESCHED[c].store(true,Ordering::Release)}
pub fn request_resched_cpu(slot:usize){if slot<NCPU{NEED_RESCHED[slot].store(true,Ordering::Release)}}
/* Asks every CPU that is currently running a task to yield at its next
   boundary, so a freshly woken task (typically a parent reaping a child,
   or a continued task) is picked up promptly instead of at the next
   timer trap. Cheap atomics, no IPIs; each CPU clears its flag when it
   yields. Used sparingly (process lifecycle events), not for every wakeup,
   to avoid yield storms. */
fn kick_others(){
 let me=cpu();
 for c in 0..NCPU{
  if c!=me&&CPU_ACTIVE[c].load(Ordering::Relaxed)&&CPU_CUR[c].load(Ordering::Relaxed)!=IDLE_U32{
   NEED_RESCHED[c].store(true,Ordering::Release);
  }
 }
}
/* True while any task sits claimable in the shared run queue. Heuristic
   for syscall-return preemption: a pointless yield just re-selects the
   current task, so over-approximation is cheap. */
pub fn runqueue_nonempty()->bool{
 RUNQ_PUSH.load(Ordering::Relaxed).wrapping_sub(RUNQ_POP.load(Ordering::Relaxed))!=0
}
/* Voluntary preemption point for code running on a task's own kernel
   stack (syscall return, #DB trap): hand the CPU over when another task
   can use it. Never call with locks held or from IST interrupt context. */
pub fn yield_if_needed(){
 if take_need_resched()||runqueue_nonempty(){yield_now();}
}
pub fn note_ipi_recv(){let c=cpu();CPU_IPI_RECV[c].fetch_add(1,Ordering::Relaxed);}
pub fn note_syscall(){let c=cpu();if c<NCPU{CPU_SYSCALL[c].fetch_add(1,Ordering::Relaxed);}}
/* A wake that lands before the target blocks is latched in wake_pending, so a
   register-then-block waiter can never lose it. */
pub fn sleep_until(tick:u64){let mut s=SCHED.lock();let current=s.cpu_current[cpu()];if current==IDLE_USIZE{return}if s.tasks[current].wake_pending{s.tasks[current].wake_pending=false;return}s.tasks[current].wake_tick=tick;s.tasks[current].state=State::Sleeping;drop(s);schedule(true)}
fn wake_sleepers(now:u64){
 let c=cpu();
 let woke={let mut s=SCHED.lock();let mut w=false;for t in &mut s.tasks{if t.state==State::Sleeping&&t.wake_pending{t.wake_pending=false;t.state=State::Runnable;RUNQ_PUSH.fetch_add(1,Ordering::Relaxed);w=true}else if t.state==State::Sleeping&&t.wake_tick<=now{t.state=State::Runnable;RUNQ_PUSH.fetch_add(1,Ordering::Relaxed);w=true}}w};
  if woke{poke_idle(c)}
}
pub fn block_current(){let mut s=SCHED.lock();let current=s.cpu_current[cpu()];if current==IDLE_USIZE{return}if s.tasks[current].wake_pending{s.tasks[current].wake_pending=false;return}s.tasks[current].state=State::Sleeping;s.tasks[current].wake_tick=u64::MAX;drop(s);schedule(true)}
/* Sleep until a signal is deliverable. Returns true when a signal is
   pending after the sleep, false when the task woke for another reason. */
pub fn block_current_until_signal()->bool{
 let mut s=SCHED.lock();let current=s.cpu_current[cpu()];
 if current==IDLE_USIZE{return crate::signal::pending().is_some()}
 if s.tasks[current].wake_pending{s.tasks[current].wake_pending=false;return crate::signal::pending().is_some()}
 s.tasks[current].state=State::Sleeping;s.tasks[current].wake_tick=u64::MAX;drop(s);schedule(true);
 crate::signal::pending().is_some()
}
pub fn current_fpu_area()->u64{let s=SCHED.lock();let m=s.cpu_current[cpu()];if m==IDLE_USIZE{return 0}s.tasks[m].fpu}
pub fn save_current_fpu(){let area=current_fpu_area();if area!=0{unsafe{fpu_save_regs(area as *mut u8)}}}
pub fn restore_current_fpu(){let area={let s=SCHED.lock();let m=s.cpu_current[cpu()];if m==IDLE_USIZE{return}s.tasks[m].fpu};if area!=0{unsafe{fpu_restore_regs(area as *const u8)}}}
/* Copies the live user SSE file (saved at syscall entry) into dst[0..256]. */
pub fn copy_current_fpu(dst:&mut [u8]){let area=current_fpu_area();if area!=0&&dst.len()>=256{unsafe{core::ptr::copy_nonoverlapping(area as *const u8,dst.as_mut_ptr(),256)}}}
/* Overwrites the running task's SSE file so the syscall-exit restore and the
   next context switch both observe the handler-approved registers. */
pub fn write_current_fpu(src:&[u8]){let area=current_fpu_area();if area!=0&&src.len()>=256{unsafe{core::ptr::copy_nonoverlapping(src.as_ptr(),area as *mut u8,256)}}}
pub fn wake_pid(pid:u32)->bool{
 let c=cpu();
 let r={let mut s=SCHED.lock();if let Some(t)=s.tasks.iter_mut().find(|t|t.pid==pid&&t.state!=State::Unused&&t.state!=State::Zombie){if t.state==State::Stopped{return true}if t.state==State::Sleeping{t.state=State::Runnable;RUNQ_PUSH.fetch_add(1,Ordering::Relaxed);}else{t.wake_pending=true}true}else{false}};
 if r{poke_idle(c)}
 r
}
pub fn current_tgid()->u32{let s=SCHED.lock();let m=s.cpu_current[cpu()];if m==IDLE_USIZE{return 0}s.tasks[m].tgid}
/* Stops the running task like Linux does for SIGSTOP/SIGTSTP/SIGTTIN/SIGTTOU
   with the default disposition: the task leaves the syscall path, reports
   WIFSTOPPED to a WUNTRACED parent and sleeps until SIGCONT resumes it. */
pub fn stop_current(sig:i32){
 /* Park-epoch: a SIGCONT that lands anywhere after this stop took effect
    must break the park even if its resume found no Stopped task yet (the
    resume ran before the Stopped store). CONT bumps cont_epoch; the loop
    breaks on any bump newer than the mark recorded here. */
 let (me_slot,mark)={
  let mut s=SCHED.lock();let cur=s.cpu_current[cpu()];
  if cur==IDLE_USIZE{return}
  if s.tasks[cur].state==State::Stopped{return}
  s.tasks[cur].state=State::Stopped;s.tasks[cur].stop_sig=sig as u8;s.tasks[cur].cont_pending=false;
  unsafe{(*crate::smp::per_cpu_ptr(cpu())).stop_trap=0};
  let m=s.tasks[cur].cont_epoch;
  (cur,m)
 };
 /* Consume the acted stop signal after the Stopped store (set-first
    ordering): a CONT racing in between still resumes or bumps the epoch,
    so the park below can never lose it. */
 {let mut procs=PROCS.lock();if let Some(p)=procs[me_slot].as_mut(){p.pending&=!crate::signal::bit(sig);}}
 let ppid={let s=SCHED.lock();s.tasks[me_slot].ppid};
  /* The parent observes the stop through waitpid(WUNTRACED) and receives
     SIGCHLD unless it asked for SA_NOCLDSTOP. The mask check below reads the
     parent's SIGCHLD disposition without deadlocks (SCHED is free here). */
  let me=current_pid();
  let my_uid=with_current_process(|p|p.uid).unwrap_or(0);
  notify_parent_stop(ppid, me, sig);
 /* Park the outgoing user RSP like schedule() does, then sleep until a
    SIGCONT marks this task Runnable again. */
 {let mut s=SCHED.lock();let cur=s.cpu_current[cpu()];if cur!=IDLE_USIZE&&s.tasks[cur].state!=State::Unused{s.tasks[cur].user_rsp=unsafe{crate::arch::x86_64::syscall::read_user_rsp()};}}
 loop{
  schedule(true);
  /* Resumed by the SIGCONT path (state changed), or by a CONT that raced
     the stop itself (epoch bumped after the mark): consume nothing from
     the run queue in the latter case, just keep running. */
  let done={let mut s=SCHED.lock();
   if s.tasks[me_slot].state!=State::Stopped{1}
   else if s.tasks[me_slot].cont_epoch!=mark{
    s.tasks[me_slot].stop_sig=0;s.tasks[me_slot].state=State::Running;2
   }else{0}};
  if done==2{
   /* The stop never became observable: drop its stale SIGCHLD so a later
      real event is not suppressed by first-wins coalescing, then report
      the cont that did arrive. */
   let me=current_pid();
   with_current_process(|p|{
    if p.sch_code==crate::signal::CLD_STOPPED&&p.sch_pid==me{
     p.pending&=!crate::signal::bit(crate::signal::SIGCHLD);
     p.sch_code=0;p.sch_pid=0;p.sch_uid=0;p.sch_status=0;
    }});
   let ppid={let s=SCHED.lock();s.tasks[me_slot].ppid};
   notify_parent_cont(ppid, me);
   break
  }
  if done==1{break}
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
 let c=cpu();
 let w={let mut s=SCHED.lock();
 if let Some(t)=s.tasks.iter_mut().find(|t|t.pid==ppid&&t.state!=State::Unused&&t.state!=State::Zombie){
  if t.state==State::Sleeping{t.state=State::Runnable;RUNQ_PUSH.fetch_add(1,Ordering::Relaxed);}else if t.state!=State::Stopped{t.wake_pending=true}
  true
 }else{false}};
 if w{kick_others();poke_idle(c)}
}
/* A stop signal the current task has not acted on yet. SIGSTOP always
   stops (its handler, if any, is ignored like Linux); TSTP/TTIN/TTOU stop
   when unblocked with the default disposition. Timer, IPI and syscall
   boundaries consult this to preempt tasks that rarely enter the kernel. */
pub fn stop_requested()->Option<i32>{
 with_current_process(|p|{
  if p.pending&crate::signal::bit(crate::signal::SIGSTOP)!=0{return Some(crate::signal::SIGSTOP)}
  for &s in &[crate::signal::SIGTSTP,crate::signal::SIGTTIN,crate::signal::SIGTTOU]{
   if p.pending&crate::signal::bit(s)!=0&&(p.sigmask&crate::signal::bit(s))==0
    &&crate::signal::action_get(s).map(|a|a.handler==crate::signal::SIG_DFL).unwrap_or(false){
    return Some(s)
   }
  }
  None
 }).flatten()
}
/* Arms a single-step trap so a user task is preempted at the next interrupt
   return. IST handlers (timer/resched) cannot schedule directly, so they set
   this flag and the asm return path sets TF; the resulting #DB runs on the
   task's own kernel stack where stopping (or yielding) is safe. Only
   meaningful when interrupting user mode. Traps only for a pending stop, an
   explicit reschedule request, or other work claimable by this CPU, so idle
   spinning never single-steps. */
pub fn request_stop_trap(){
 let c=cpu();
 if stop_requested().is_some(){unsafe{(*crate::smp::per_cpu_ptr(c)).stop_trap=1;}return}
 if take_need_resched(){unsafe{(*crate::smp::per_cpu_ptr(c)).stop_trap=1;}return}
 let other={
  let s=SCHED.lock();
  let cur=s.cpu_current[c];
  (0..MAX_TASKS).any(|i|i!=cur&&s.tasks[i].state==State::Runnable
   &&(s.tasks[i].pinned==PIN_ANY||s.tasks[i].pinned==c as u32)
   &&TASK_OWNER[i].load(Ordering::Acquire)==NO_OWNER)
 };
 if other{unsafe{(*crate::smp::per_cpu_ptr(c)).stop_trap=1;}}
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
  TASK_OWNER[idx].store(NO_OWNER,Ordering::Release);
  RUNQ_PUSH.fetch_add(1,Ordering::Relaxed);
  let ppid=s.tasks[idx].ppid;drop(s);
  let uid=with_process_pid(pid,|p|p.uid).unwrap_or(0);
  (ppid,uid)};
 notify_parent_cont(ppid, pid);
 kick_others();
 poke_idle(cpu());
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
  /* A stop signal for a task running on another CPU needs a kick so the
     target notices at its next boundary instead of only at its next
     voluntary kernel entry. */
  if crate::signal::default_stops(sig){
   let (owner,remote)={let s=SCHED.lock();
    (TASK_OWNER[slot].load(Ordering::Acquire),
     s.tasks[slot].state==State::Running&&slot!=s.cpu_current[cpu()])};
   if remote&&owner!=NO_OWNER&&(owner as usize)<NCPU&&(owner as usize)!=cpu(){
    crate::arch::x86_64::apic::send_resched_ipi(crate::smp::apic_id_of_slot(owner as usize));
   }
  }
  wake_pid(pid);
  Ok(())
 }
/* Marks sig pending on the task owning pid and wakes it if blocked. */
pub fn signal_pid(pid:u32,sig:i32)->Result<(),i64>{
 let me_cpu=cpu();
  /* SIGSTOP always stops, even if the target blocked or ignored it: Linux
     refuses handlers and masks for it, so an immediate stop is correct. */
  if sig==crate::signal::SIGSTOP{
   let slot={let s=SCHED.lock();s.tasks.iter().position(|t|t.pid==pid&&t.state!=State::Unused&&t.state!=State::Zombie).ok_or(crate::errno::ESRCH)?};
   /* A thread stop is process-wide in Linux; here every task is a process
      except CLONE_VM threads, which SIGSTOP still stops individually. */
   let already={let s=SCHED.lock();s.tasks[slot].state==State::Stopped};
   if !already{
    let running_elsewhere={let s=SCHED.lock();
     s.tasks[slot].state==State::Running&&slot!=s.cpu_current[me_cpu]};
    if !running_elsewhere{
     /* Self-stop defers to the delivery path so the syscall frame exists;
        a queued (Sleeping/Runnable) target stops right here since it can
        never run before the SIGCONT. Stopped directly: no run-queue push,
        and a consumed queue slot is paid back. */
     if slot==SCHED.lock().cpu_current[me_cpu]{
      {let mut procs=PROCS.lock();if let Some(p)=procs[slot].as_mut(){p.pending|=crate::signal::bit(sig);p.pending_from[sig as usize]=current_pid();}}
     }else{
      /* A queued (Sleeping/Runnable) target stops right here: it can never
         run before the SIGCONT. No run-queue push (it never becomes
         claimable); a consumed queue slot is paid back to keep push==pop. */
      let (did_stop,was_runnable)={let mut s=SCHED.lock();
       let was_runnable=s.tasks[slot].state==State::Runnable;
       let queued=s.tasks[slot].state==State::Sleeping||was_runnable;
       if queued{
        s.tasks[slot].state=State::Stopped;
        s.tasks[slot].stop_sig=sig as u8;s.tasks[slot].cont_pending=false;
       }
       (queued,was_runnable)};
      if did_stop{
       if was_runnable{RUNQ_POP.fetch_add(1,Ordering::Relaxed);}
       {let mut procs=PROCS.lock();if let Some(p)=procs[slot].as_mut(){p.pending&=!crate::signal::bit(sig);}}
       let ppid={let s=SCHED.lock();s.tasks[slot].ppid};
       let uid=with_process_pid(pid,|p|p.uid).unwrap_or(0);
       notify_parent_stop(ppid, pid, sig);
       poke_idle(me_cpu);
       let _=uid;
      }
     }
    }else{
     /* Running on another CPU: flag it and kick that CPU. The target stops
        itself at the next syscall/trap/timer boundary; state and ownership
        stay untouched so nothing can claim a live stack. */
     {let mut procs=PROCS.lock();if let Some(p)=procs[slot].as_mut(){p.pending|=crate::signal::bit(sig);p.pending_from[sig as usize]=current_pid();}}
     let owner=TASK_OWNER[slot].load(Ordering::Acquire);
     if owner!=NO_OWNER&&(owner as usize)<NCPU&&(owner as usize)!=me_cpu{
      crate::arch::x86_64::apic::send_resched_ipi(crate::smp::apic_id_of_slot(owner as usize));
     }
    }
   }
   return Ok(())
  }
  /* SIGCONT resumes a stopped task. With the default disposition the signal
     is then consumed; with a handler it stays queued so the handler runs.
     A CONT also discards pending stop signals (Linux): a stop that has not
     acted yet is cancelled instead of parking the task later. */
  if sig==crate::signal::SIGCONT{
   let slot={let s=SCHED.lock();s.tasks.iter().position(|t|t.pid==pid&&t.state!=State::Unused).ok_or(crate::errno::ESRCH)?};
   let had_stop={let mut procs=PROCS.lock();match procs[slot].as_mut(){
    Some(p)=>{
     let m=p.pending&(crate::signal::bit(crate::signal::SIGSTOP)|crate::signal::bit(crate::signal::SIGTSTP)|crate::signal::bit(crate::signal::SIGTTIN)|crate::signal::bit(crate::signal::SIGTTOU));
     if m!=0{p.pending&=!m;true}else{false}}
    None=>false}};
   let was_stopped=resume_stopped(pid);
   {let mut procs=PROCS.lock();if let Some(p)=procs[slot].as_mut(){
    let h=p.handlers[sig as usize].handler;
    if h==crate::signal::SIG_DFL||h==crate::signal::SIG_IGN{p.pending&=!crate::signal::bit(sig);}
    else{p.pending|=crate::signal::bit(sig);p.pending_from[sig as usize]=current_pid();}
   }}
   if !was_stopped{
    /* No parked stop to resume, but one may be in flight (its stop bit was
       just cleared above, or it was already consumed and the task is about
       to park): bump the cont epoch so the stop loop breaks instead of
       parking forever on a lost wakeup. */
    if had_stop{let mut s=SCHED.lock();if s.tasks[slot].state!=State::Unused&&s.tasks[slot].state!=State::Zombie{s.tasks[slot].cont_epoch=s.tasks[slot].cont_epoch.wrapping_add(1);}}
    wake_pid(pid);
   }
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
 /* Read uid with no scheduler lock held: finish() holds SCHED across the
    Zombie transition below, and the scheduler lock is a leaf (never held
    across the process table). */
 let uid=with_current_process(|p|p.uid).unwrap_or(0);
 let parent={
  let mut s=SCHED.lock();let current=s.cpu_current[cpu()];
  if current==IDLE_USIZE{crate::arch::halt()}
  s.tasks[current].state=State::Zombie;s.tasks[current].exit_code=code;s.tasks[current].term_signal=signal;
  let parent=s.tasks[current].ppid;let is_thread=s.tasks[current].is_thread;
  drop(s);
  if parent!=0&&!is_thread{
   /* No core dumps: signal deaths report CLD_KILLED, exits CLD_EXITED. */
   if signal!=0{store_sigchld(parent,crate::signal::CLD_KILLED,me,uid,signal as i32);}
   else{store_sigchld(parent,crate::signal::CLD_EXITED,me,uid,code&0xff);}
   wake_parent_for_wait(parent);
  }
  parent
 };
}loop{schedule(true);unsafe{core::arch::asm!("hlt",options(nomem,nostack));}}}
pub fn setpgid(pid:u32,pgid:u32)->Result<(),i64>{
 let me=current_pid();
 let target=if pid==0{me}else{pid};
 if target!=me{return Err(crate::errno::EPERM)}
 let mut s=SCHED.lock();
 let idx=s.cpu_current[cpu()];
 if idx==IDLE_USIZE{return Err(crate::errno::ESRCH)}
 let newpg=if pgid==0{me}else{pgid};
 if newpg!=me{return Err(crate::errno::EPERM)}
 s.tasks[idx].pgid=newpg;
 Ok(())
}
pub fn setsid()->Result<u32,i64>{
 let me=current_pid();
 let mut s=SCHED.lock();
 let idx=s.cpu_current[cpu()];
 if idx==IDLE_USIZE{return Err(crate::errno::ESRCH)}
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
 let is_thread={let s=SCHED.lock();let m=s.cpu_current[cpu()];if m==IDLE_USIZE{crate::arch::halt()}s.tasks[m].is_thread};
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
 let top=current_kernel_stack_top();
 let base=unsafe{core::ptr::addr_of!(STACKS)as u64};
 if top>=base&&top<=base+(MAX_TASKS*(STACK_SIZE+64))as u64&&r+1024<top.saturating_sub(STACK_SIZE as u64){
  crate::println!("fatal: kernel stack overflow pid={} rsp={:#x} top={:#x}",current_pid(),r,top);
  crate::arch::halt();
 }
}
pub fn exit_fault(vector:u64,error:u64,rip:u64,cr2:u64)->!{robust_exit();crate::println!("user fault vector={} error={:#x} rip={:#x} cr2={:#x}",vector,error,rip,cr2);with_current_process(|proc|proc.teardown_exit(128+11,vector));finish(128+11,11)}
/* Post-switch hook, called from switch_context on the incoming stack right
   after the new rsp is loaded. Completes the outgoing task's transition: a
   yielding task becomes Runnable and ownership is released. Doing this here
   (instead of before the switch) closes the race where another CPU claims a
   task whose old stack is still live: until the hook runs, the outgoing task
   stays Running and owned, so every claimer skips it. Runs with interrupts
   enabled; the scheduler lock itself provides exclusion against timer/IPI
   handlers that also take it. */
#[unsafe(no_mangle)]pub extern "sysv64" fn switch_hook(){
 let c=cpu();
 if c>=NCPU{return}
 /* Load without consuming yet: the stash stays pending until the transition
    below commits, so a concurrent scanner can tell a mid-switch task
    (skip silently) from a genuinely double-owned one (count it). */
 let old=SWITCH_OLD[c].load(Ordering::Acquire);
 if old==IDLE_U32{return}
 let o=old as usize;
 if o>=MAX_TASKS{return}
 let mut s=SCHED.lock();
 if TASK_OWNER[o].load(Ordering::Acquire)!=c as u32{
  SWITCH_OLD[c].store(IDLE_U32,Ordering::Release);
  return
 }
 let y=SWITCH_YIELD[c].load(Ordering::Acquire);
 /* Release ownership before marking Runnable: a scanner in between sees
    Running+unowned (skipped by the state check) or Runnable+unowned
    (safe to claim, the switch already committed), never a stale pair. */
 TASK_OWNER[o].store(NO_OWNER,Ordering::Release);
 if y&&s.tasks[o].state==State::Running{
   s.tasks[o].state=State::Runnable;
   RUNQ_PUSH.fetch_add(1,Ordering::Relaxed);
  }
 SWITCH_YIELD[c].store(false,Ordering::Release);
 SWITCH_OLD[c].store(IDLE_U32,Ordering::Release);
}
/* An owned-but-Runnable task is benign while its owner is mid-switch (the
   post-switch hook resolves it microseconds later; a wake landing in that
   window makes it briefly Runnable+owned) or while it is still the owner's
   current task (the owner will run or switch it itself). Skip those
   silently; count only a genuinely stranded double-ownership. */
fn skip_owned(s:&Scheduler,i:usize,owner:u32)->bool{
 let o=owner as usize;
 if o>=NCPU{return true}
 if SWITCH_OLD[o].load(Ordering::Acquire)!=IDLE_U32{return false}
 if s.cpu_current[o]==i{return false}
 true
}
fn schedule(blocked:bool){
 let cpu=cpu();
 let mut sw_old:*mut u64=core::ptr::null_mut();
 let mut sw_new_rsp:u64=0;let mut sw_old_fpu:u64=0;let mut sw_new_fpu:u64=0;
 let mut to_idle=false;let mut next_task:Option<Task>=None;let mut abandon_old=false;
 {
  let mut s=SCHED.lock();
  let old=s.cpu_current[cpu];
  /* The idle loop never schedules: it claims directly. */
  if old==IDLE_USIZE{return}
  /* A dead or reaped task never resumes: abandon its stack without saving.
     No hook runs on the nosave path, so ownership is released here; a
     Zombie/Unused slot is never claimable, so this cannot race. */
  if s.tasks[old].state==State::Zombie||s.tasks[old].state==State::Unused{
   abandon_old=true;
   TASK_OWNER[old].store(NO_OWNER,Ordering::Release);
  }
  /* The user RSP is process-wide state in the syscall path; park the outgoing
     task's copy so a resuming thread restores its own. The other per-CPU
     syscall cells (frame, exec redirect, sigreturn) are live across a
     switch the same way: park them too, or a task switched back in would
     observe whatever the intervening task left behind. */
  if s.tasks[old].state!=State::Unused{unsafe{
   s.tasks[old].user_rsp=crate::arch::x86_64::syscall::read_user_rsp();
   s.tasks[old].sc_frame=crate::arch::x86_64::syscall::read_syscall_frame();
   s.tasks[old].sc_exec_rip=crate::arch::x86_64::syscall::read_exec_rip();
   s.tasks[old].sc_exec_pending=crate::arch::x86_64::syscall::read_exec_pending();
   s.tasks[old].sc_sigret=crate::arch::x86_64::syscall::read_sigret_value();
   s.tasks[old].sc_sigret_valid=crate::arch::x86_64::syscall::read_sigret_valid();
  }}
  let mut next=old;let mut found=false;
  for step in 1..=MAX_TASKS{
   let i=(old+step)%MAX_TASKS;
   if s.tasks[i].state!=State::Runnable{continue}
   /* The outgoing task selects itself without tripping the ownership
      alarm: a wake that landed after it started blocking made it Runnable
      again, and continuing it here consumes that wake. */
   if i==old{next=old;found=true;break}
   if s.tasks[i].pinned!=PIN_ANY&&s.tasks[i].pinned!=cpu as u32{continue}
   let own_i=TASK_OWNER[i].load(Ordering::Acquire);
   if own_i!=NO_OWNER{if skip_owned(&s,i,own_i){DOUBLE_EXEC.fetch_add(1,Ordering::Relaxed);}continue}
   next=i;found=true;break;
  }
  if found{
   if next==old{
    /* Nothing else to run: keep the current task. No switch happens, so
       no post-switch hook is needed; Running+owned stays consistent. A
       Runnable old task got here via a wake that landed after it started
       blocking: consume that wake's run-queue push and keep running. */
    if s.tasks[old].state==State::Runnable{
     RUNQ_POP.fetch_add(1,Ordering::Relaxed);
    }
    s.tasks[old].state=State::Running;
    return;
   }
   s.tasks[next].state=State::Running;
   TASK_OWNER[next].store(cpu as u32,Ordering::Release);
   s.cpu_current[cpu]=next;
   CPU_CUR[cpu].store(next as u32,Ordering::Release);
   RUNQ_POP.fetch_add(1,Ordering::Relaxed);
   CPU_TASKS_RUN[cpu].fetch_add(1,Ordering::Relaxed);
   CPU_CTX[cpu].fetch_add(1,Ordering::Relaxed);
   unsafe{(*crate::smp::per_cpu_ptr(cpu)).current_fpu=s.tasks[next].fpu};
   /* Stash the outgoing task for the post-switch hook: it stays Running
      and owned until the switch commits, so no other CPU can claim a
      task whose old stack is still live. The hook (not here) moves a
      yielding task back to Runnable and releases ownership. */
   if !abandon_old{
    SWITCH_OLD[cpu].store(old as u32,Ordering::Release);
    SWITCH_YIELD[cpu].store(!blocked,Ordering::Release);
   }
   sw_old=&mut s.tasks[old].rsp as *mut u64;sw_old_fpu=s.tasks[old].fpu;
   let nt=s.tasks[next];sw_new_rsp=nt.rsp;sw_new_fpu=nt.fpu;next_task=Some(nt);
  }else{
   if s.tasks[old].state==State::Running||s.tasks[old].state==State::Runnable{
    /* Runnable here means a wake landed after the block started (the scan
       above would otherwise have self-selected it): consume that wake. */
    if s.tasks[old].state==State::Runnable{
     RUNQ_POP.fetch_add(1,Ordering::Relaxed);
    }
    s.tasks[old].state=State::Running;s.cpu_current[cpu]=old;return;
   }
   /* Nothing runnable and the outgoing task is parked: switch to this
      CPU's idle loop until work arrives. Ownership is released by the
      post-switch hook (abandoned stacks release it here instead, since
      the nosave path runs no hook). */
   s.cpu_current[cpu]=IDLE_USIZE;
   CPU_CUR[cpu].store(IDLE_U32,Ordering::Release);
   if !abandon_old{
    SWITCH_OLD[cpu].store(old as u32,Ordering::Release);
    SWITCH_YIELD[cpu].store(false,Ordering::Release);
   }
   sw_old=&mut s.tasks[old].rsp as *mut u64;sw_old_fpu=s.tasks[old].fpu;
   to_idle=true;
  }
 }
   if to_idle{
   unsafe{crate::arch::x86_64::syscall::write_user_rsp(0)};
  unsafe{apply_idle_state(cpu)};
  let idle_rsp=unsafe{IDLE_CTX_RSP[cpu]};
  if abandon_old{
   unsafe{switch_context_nosave(idle_rsp,sw_old_fpu,idle_fpu(cpu))};
  }else{
   unsafe{switch_context(sw_old,idle_rsp,sw_old_fpu,idle_fpu(cpu))};
  }
  return;
 }
   if let Some(nt)=next_task{
   PID.store(nt.pid,Ordering::Relaxed);
   unsafe{restore_task_state(nt);apply_arch_state(cpu,nt);
   if abandon_old{switch_context_nosave(nt.rsp,sw_old_fpu,nt.fpu)}
   else{switch_context(sw_old,sw_new_rsp,sw_old_fpu,sw_new_fpu)}}
 }
}

pub fn set_current_fs_base(value:u64){let mut s=SCHED.lock();let current=s.cpu_current[cpu()];if current==IDLE_USIZE{return}s.tasks[current].fs_base=value;drop(s);unsafe{crate::arch::x86_64::msr::wrmsr(crate::arch::x86_64::msr::IA32_FS_BASE,value)}}
pub fn fs_base()->u64{let s=SCHED.lock();let m=s.cpu_current[cpu()];if m==IDLE_USIZE{return 0}s.tasks[m].fs_base}
pub fn bind_current_address_space(cr3:u64,kernel_stack_top:u64){let c=cpu();let mut s=SCHED.lock();let current=s.cpu_current[c];if current==IDLE_USIZE{return}s.tasks[current].cr3=cr3;s.tasks[current].kernel_stack_top=kernel_stack_top;drop(s);unsafe{set_rsp0_cpu(c,kernel_stack_top);crate::arch::x86_64::syscall::set_kernel_stack(kernel_stack_top)}}
/* TSS RSP0 is per-CPU (BSP uses BOOT_TSS, APs their CPU_TSS slot). A task
   switch must update the switching CPU's own TSS, never another CPU's. */
unsafe fn set_rsp0_cpu(cpu:usize,v:u64){unsafe{if cpu==crate::smp::bsp_slot(){crate::arch::x86_64::descriptors::set_rsp0(v)}else{crate::smp::set_tss_rsp0(cpu,v)}}}
unsafe fn apply_arch_state(cpu:usize,task:Task){unsafe{
 /* Always reload CR3 (full TLB flush): PML4 frames are recycled, so the
    incoming tables may live at the same address as the outgoing ones.
    Skipping the reload on address match leaves stale TLB entries behind,
    faulting user text that the page tables map correctly. No PCID here. */
 crate::arch::x86_64::paging::PageTableRoot{pml4:task.cr3}.activate();
 crate::arch::x86_64::msr::wrmsr(crate::arch::x86_64::msr::IA32_FS_BASE,task.fs_base);set_rsp0_cpu(cpu,task.kernel_stack_top);crate::arch::x86_64::syscall::set_kernel_stack(task.kernel_stack_top);}}
/* Idle-arch state: a known-good kernel CR3, no user TLS, and the per-CPU
   idle stack for the TSS and the syscall entry path. */
unsafe fn apply_idle_state(cpu:usize){
 unsafe{
  let kcr3=KERNEL_CR3.load(Ordering::Relaxed);
  if kcr3!=0&&crate::arch::x86_64::paging::PageTableRoot::current().pml4!=kcr3{
   crate::arch::x86_64::paging::PageTableRoot{pml4:kcr3}.activate();
  }
  crate::arch::x86_64::msr::wrmsr(crate::arch::x86_64::msr::IA32_FS_BASE,0);
  let top=idle_top(cpu);
  set_rsp0_cpu(cpu,top);
  crate::arch::x86_64::syscall::set_kernel_stack(top);
  (*crate::smp::per_cpu_ptr(cpu)).current_fpu=idle_fpu(cpu);
 }
}
pub enum WaitOutcome{Reaped(u32,i32,u64,u64),Stopped(u32,i32),Continued(u32),NoHang,Blocked}
fn encode_status(t:Task)->i32{if t.term_signal!=0{t.term_signal as i32}else{(t.exit_code&0xff)<<8}}
fn encode_stopped(sig:u8)->i32{((sig as i32)<<8)|0x7f}
pub fn reap_child(pid_sel:i64,nohang:bool)->Result<WaitOutcome,i64>{reap_child_opts(pid_sel,nohang,false,false)}
pub fn reap_child_opts(pid_sel:i64,nohang:bool,untraced:bool,continued:bool)->Result<WaitOutcome,i64>{
 let me=current_pid();let(pslot,my_pgid)={let s=SCHED.lock();let m=s.cpu_current[cpu()];if m==IDLE_USIZE{return Err(crate::errno::ECHILD)}(m,s.tasks[m].pgid)};
 let matches=|t:&Task|t.ppid==me&&(pid_sel==-1||(pid_sel==0&&t.pgid==my_pgid)||(pid_sel>0&&t.pid==pid_sel as u32)||(pid_sel< -1&&t.pgid==(-pid_sel) as u32));
 let mut s=SCHED.lock();
 if let Some(slot)=s.tasks.iter().position(|t|t.state==State::Zombie&&matches(t)){let t=s.tasks[slot];s.tasks[slot]=EMPTY;TASK_OWNER[slot].store(NO_OWNER,Ordering::Release);drop(s);KERNEL_ENTRIES.lock()[slot]=None;USER_STARTS.lock()[slot]=None;FORK_RESUMES.lock()[slot]=None;let(utime,stime)={let mut procs=PROCS.lock();let times=procs[slot].take().map(|p|(p.utime_ns.saturating_add(p.cutime_ns),p.stime_ns.saturating_add(p.cstime_ns))).unwrap_or((0,0));if !t.is_thread{if let Some(pp)=procs[pslot].as_mut(){pp.cutime_ns=pp.cutime_ns.saturating_add(times.0);pp.cstime_ns=pp.cstime_ns.saturating_add(times.1);}}times};return Ok(WaitOutcome::Reaped(t.pid,encode_status(t),utime,stime))}
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
pub fn scheduler_self_test()->Result<(),&'static str>{static A:AtomicU32=AtomicU32::new(0);static B:AtomicU32=AtomicU32::new(0);static TLS_OK:AtomicU32=AtomicU32::new(0);static CPU_A:AtomicU32=AtomicU32::new(0xFFFF_FFFF);static CPU_B:AtomicU32=AtomicU32::new(0xFFFF_FFFF);fn a(){CPU_A.store(cpu() as u32,Ordering::Relaxed);set_current_fs_base(0x1000);A.fetch_add(1,Ordering::Relaxed);yield_now();if unsafe{crate::arch::x86_64::msr::rdmsr(crate::arch::x86_64::msr::IA32_FS_BASE)}==0x1000{TLS_OK.fetch_add(1,Ordering::Relaxed);}A.fetch_add(1,Ordering::Relaxed);}fn b(){CPU_B.store(cpu() as u32,Ordering::Relaxed);set_current_fs_base(0x2000);B.fetch_add(1,Ordering::Relaxed);yield_now();if unsafe{crate::arch::x86_64::msr::rdmsr(crate::arch::x86_64::msr::IA32_FS_BASE)}==0x2000{TLS_OK.fetch_add(1,Ordering::Relaxed);}B.fetch_add(1,Ordering::Relaxed);}spawn_kernel(a).map_err(|_|"spawn A failed")?;spawn_kernel(b).map_err(|_|"spawn B failed")?; for _ in 0..8{yield_now();if A.load(Ordering::Relaxed)==2&&B.load(Ordering::Relaxed)==2&&TLS_OK.load(Ordering::Relaxed)==2{let ca=wait_child(2).map_err(|_|"reap A failed")?;let cb=wait_child(3).map_err(|_|"reap B failed")?;if ca!=0||cb!=0{return Err("unexpected child status")}let stats=resource_stats();if stats.active!=1||stats.zombies!=0||stats.unused!=MAX_TASKS-1{return Err("task slots not reclaimed after wait")}crate::println!("scheduler: context, FS base and zombie reap self-test passed cpu_a={} cpu_b={}",CPU_A.load(Ordering::Relaxed),CPU_B.load(Ordering::Relaxed));crate::println!("resources: tasks active={} zombies={} unused={}",stats.active,stats.zombies,stats.unused);return Ok(())}}
 crate::println!("scheduler: STALLED a={} b={} tls={} cpu_a={} cpu_b={}",A.load(Ordering::Relaxed),B.load(Ordering::Relaxed),TLS_OK.load(Ordering::Relaxed),CPU_A.load(Ordering::Relaxed),CPU_B.load(Ordering::Relaxed));
 {let s=SCHED.lock();
  for i in 0..MAX_TASKS{
   let t=&s.tasks[i];
   if t.state!=State::Unused{
    crate::println!("sched slot={} pid={} state={} owner={:#x} rsp={:#x} pinned={:#x}",
     i,t.pid,t.state as u8,TASK_OWNER[i].load(Ordering::Relaxed),t.rsp,t.pinned);
   }
  }
  for c in 0..NCPU{
   if CPU_ACTIVE[c].load(Ordering::Relaxed){
    crate::println!("sched cpu{} cur={} loops={} hlts={} ticks={}",
     c,CPU_CUR[c].load(Ordering::Relaxed),CPU_IDLE_LOOPS[c].load(Ordering::Relaxed),
     CPU_HLTS[c].load(Ordering::Relaxed),CPU_TICKS[c].load(Ordering::Relaxed));
   }
  }}
 Err("scheduler self-test stalled")}
pub fn run()->!{loop{yield_now();unsafe{core::arch::asm!("sti","hlt",options(nomem,nostack,preserves_flags));}}}
/* DIAG-MS93: AP progress counters for crash diagnosis. */
pub fn diag_ap_progress(){
 for c in 0..NCPU{
  if CPU_ACTIVE[c].load(Ordering::Relaxed){
   crate::println!("diag ap{} tasks={} syscalls={} ipi_r={} ticks={} loops={}",
    c,CPU_TASKS_RUN[c].load(Ordering::Relaxed),CPU_SYSCALL[c].load(Ordering::Relaxed),
    CPU_IPI_RECV[c].load(Ordering::Relaxed),CPU_TICKS[c].load(Ordering::Relaxed),
    CPU_IDLE_LOOPS[c].load(Ordering::Relaxed));
  }
 }
}
/* DIAG-MS93: check whether an rsp belongs to a known kernel stack region. */
pub fn diag_stacks(rsp:u64){
 unsafe{
  let s0=core::ptr::addr_of!(STACKS[0].0) as u64;
  let sN=core::ptr::addr_of!(STACKS[MAX_TASKS-1].0) as u64 + STACK_SIZE as u64;
  let i0=core::ptr::addr_of!(AP_IDLE_STACKS[0]) as u64;
  let iN=core::ptr::addr_of!(AP_IDLE_STACKS[NCPU-1]) as u64 + AP_IDLE_STACK_SIZE as u64;
  let in_task=rsp>=s0&&rsp<sN;
  let in_idle=rsp>=i0&&rsp<iN;
  crate::println!("diag stacks rsp={:#x} task=[{:#x},{:#x})={} idle=[{:#x},{:#x})={}",
   rsp,s0,sN,in_task as u8,i0,iN,in_idle as u8);
  /* Task 0 runs on the bootstrap stack (RING0_STACK), not STACKS[]. */
  if let Some(s)=SCHED.try_lock(){
   let t0top=s.tasks[0].kernel_stack_top;
   let t0rsp=s.tasks[0].rsp;
   crate::println!("diag t0top={:#x} t0rsp={:#x} in128k={}",t0top,t0rsp,(rsp<=t0top&&rsp+131072>=t0top) as u8);
  }else{
   crate::println!("diag sched locked");
  }
 }
}
/* DIAG-MS93: task table snapshot for crash diagnostics. Uses try_lock so a
   fault handler never deadlocks if the lock is held. */
pub fn diag_tasks(){
 match SCHED.try_lock(){
  Some(s)=>{
   for i in 0..MAX_TASKS{
    let t=&s.tasks[i];
    if t.state!=State::Unused||t.pid!=0{
     crate::println!("diag task i={} pid={} st={} rsp={:#x} owner={:#x} pinned={:#x}",
      i,t.pid,t.state as u8,t.rsp,TASK_OWNER[i].load(Ordering::Relaxed),t.pinned);
    }
   }
   for c in 0..2usize{
    crate::println!("diag cpu{} cur={} active={}",
     c,CPU_CUR[c].load(Ordering::Relaxed),CPU_ACTIVE[c].load(Ordering::Relaxed));
   }
  }
  None=>{crate::println!("diag sched locked");},
 }
}
/* SMP accounting report and quiescence audit, printed once the user process
   is reaped. Proves per-CPU dispatch, IPI traffic and run-queue balance. */
pub fn report_smp_stats(){
 let online=crate::smp::online_count();
 crate::println!("smp: stats online={} runq_push={} runq_pop={} double_exec={} ap_user_tasks={}",
  online,RUNQ_PUSH.load(Ordering::Relaxed),RUNQ_POP.load(Ordering::Relaxed),
  DOUBLE_EXEC.load(Ordering::Relaxed),AP_USER_RUN.load(Ordering::Relaxed));
 for slot in 0..NCPU{
  if !CPU_ACTIVE[slot].load(Ordering::Relaxed){continue}
  let cur=CPU_CUR[slot].load(Ordering::Relaxed);
  let cur_s=if cur==IDLE_U32{"idle"}else{"task"};
  crate::println!("smp: cpu{} tasks_run={} ctx={} syscalls={} ipi_sent={} ipi_recv={} idle_loops={} hlts={} ticks={} tstage={:#x} cur={}:{}",
   slot,CPU_TASKS_RUN[slot].load(Ordering::Relaxed),CPU_CTX[slot].load(Ordering::Relaxed),
   CPU_SYSCALL[slot].load(Ordering::Relaxed),CPU_IPI_SENT[slot].load(Ordering::Relaxed),
   CPU_IPI_RECV[slot].load(Ordering::Relaxed),CPU_IDLE_LOOPS[slot].load(Ordering::Relaxed),
   CPU_HLTS[slot].load(Ordering::Relaxed),CPU_TICKS[slot].load(Ordering::Relaxed),
   CPU_TSTAGE[slot].load(Ordering::Relaxed),cur,cur_s);
 }
 let s=SCHED.lock();
 let mut runnable=0usize;let mut owned_running=0usize;let mut lost=0usize;
 for i in 0..MAX_TASKS{
  if s.tasks[i].state==State::Runnable{runnable+=1}
  let o=TASK_OWNER[i].load(Ordering::Relaxed);
  if o!=NO_OWNER{
   if s.tasks[i].state==State::Running{owned_running+=1}
   else{lost+=1}
  }
 }
 drop(s);
 let push=RUNQ_PUSH.load(Ordering::Relaxed);let pop=RUNQ_POP.load(Ordering::Relaxed);
 crate::println!("smp: audit runnable={} push_pop_delta={} owned_running={} lost_owner={}",
  runnable,push.wrapping_sub(pop),owned_running,lost);
 let d=crate::arch::x86_64::apic::diag_ap();
 crate::println!("smp: diag ap_entry cs={:#x} ss={:#x} rsp={:#x} cr3={:#x} ist_now={:#x} entry_rsp={:#x} tpr={:#x} svr={:#x} lvt={:#x} me={}",
  d[0],d[1],d[2],d[3],d[4],d[5],d[6],d[7],d[8],d[9]);
}
