//! Linux signal ABI: per-process state, handler frames and delivery.
//!
//! The frame layout follows the published Linux x86-64 rt_sigframe ABI: the
//! restorer address occupies the lowest slot so that the handler returns into
//! it, the ucontext follows and the siginfo closes the frame. Provenance for
//! every layout fact is recorded in `docs/compat-ledger.md`.
use crate::arch::x86_64::syscall::{self,UserFrame};
use crate::errno;
use crate::task;

pub const SIGHUP:i32=1;pub const SIGINT:i32=2;pub const SIGQUIT:i32=3;pub const SIGILL:i32=4;pub const SIGTRAP:i32=5;pub const SIGABRT:i32=6;pub const SIGBUS:i32=7;pub const SIGFPE:i32=8;pub const SIGKILL:i32=9;pub const SIGUSR1:i32=10;pub const SIGSEGV:i32=11;pub const SIGUSR2:i32=12;pub const SIGPIPE:i32=13;pub const SIGALRM:i32=14;pub const SIGTERM:i32=15;pub const SIGCHLD:i32=17;pub const SIGCONT:i32=18;pub const SIGSTOP:i32=19;pub const SIGTSTP:i32=20;
pub const SIG_DFL:u64=0;pub const SIG_IGN:u64=1;
pub const SA_NOCLDSTOP:u64=1;pub const SA_NOCLDWAIT:u64=2;pub const SA_SIGINFO:u64=4;pub const SA_ONSTACK:u64=0x08000000;pub const SA_RESTORER:u64=0x04000000;pub const SA_RESTART:u64=0x10000000;pub const SA_NODEFER:u64=0x40000000;pub const SA_RESETHAND:u64=0x80000000;
pub const BLOCK:u64=0;pub const UNBLOCK:u64=1;pub const SETMASK:u64=2;
pub const SI_USER:i32=0;
const MAX_SIG:usize=64;

/// Linux `struct k_sigaction`. The field order is part of the rt_sigaction ABI:
/// handler, flags, mask and restorer.
#[repr(C)]
#[derive(Clone,Copy,Debug,Default)]
pub struct Action{pub handler:u64,pub flags:u64,pub mask:u64,pub restorer:u64}

/// Linux `struct sigcontext` for x86-64: the register order matches the
/// published `REG_*` index list, so `gregs[i]` is the register named `REG_*`.
#[repr(C)]
#[derive(Clone,Copy,Default)]
pub struct SigContext{
 pub r8:u64,pub r9:u64,pub r10:u64,pub r11:u64,pub r12:u64,pub r13:u64,pub r14:u64,pub r15:u64,
 pub rdi:u64,pub rsi:u64,pub rbp:u64,pub rbx:u64,pub rdx:u64,pub rax:u64,pub rcx:u64,pub rsp:u64,pub rip:u64,
 pub eflags:u64,pub cs:u16,pub gs:u16,pub fs:u16,pub ss:u16,
 pub err:u64,pub trapno:u64,pub oldmask:u64,pub cr2:u64,
 pub fpstate:u64,pub reserved1:[u64;8]}

/// The `stack_t` member of the ucontext.
#[repr(C)]
#[derive(Clone,Copy,Default)]
pub struct StackArea{pub ss_sp:u64,pub ss_flags:u32,pub ss_pad:u32,pub ss_size:u64}

/// The userspace `ucontext_t`: flags, link, stack, machine context, the saved
/// signal mask and the floating-point area.
#[repr(C)]
#[derive(Clone,Copy,Default)]
pub struct UContext{pub uc_flags:u64,pub uc_link:u64,pub uc_stack:StackArea,pub uc_mcontext:SigContext,pub uc_sigmask:u64,pub uc_fpstate:[u8;512]}

/// The `siginfo_t` header plus the `_kill` union member that follows it.
#[repr(C)]
#[derive(Clone,Copy,Default)]
pub struct SigInfo{pub si_signo:i32,pub si_errno:i32,pub si_code:i32,pub si_pid:i32,pub si_uid:i32,pub si_pad:[i32;26]}

/// The user context prepared for a handler entry.
pub struct Delivery{pub rip:u64,pub rsp:u64,pub rflags:u64,pub sig:i32,pub siginfo:u64,pub ucontext:u64}

/// The default action of a signal when no handler is installed.
#[derive(Clone,Copy,PartialEq)]
pub enum DefaultAction{Ignore,Terminate,Stop}

fn valid(sig:i32)->bool{sig>=1&&sig<MAX_SIG as i32}
fn bit(sig:i32)->u64{1u64<<(sig as u64-1)}

pub fn default_action(sig:i32)->DefaultAction{
 match sig{
  SIGCHLD|SIGCONT=>DefaultAction::Ignore,
  SIGSTOP|SIGTSTP=>DefaultAction::Stop,
  _=>DefaultAction::Terminate,
 }
}

/// Queues a signal for a target process. `pid` follows the Linux `kill`
/// convention: a positive value selects one process, zero selects the caller's
/// process group and a negative value selects a process group.
pub fn send(pid:i64,sig:i32)->Result<(),i64>{
 if !valid(sig){return Err(errno::EINVAL)}
 let sender=task::current_pid();
 let targets=task::signal_targets(pid);
 if targets.count==0{return Err(errno::ESRCH)}
 let mut queued=0usize;
 for i in 0..targets.count{
  let t=targets.pids[i];
  if task::with_process_by_pid(t,|p|{p.pending|=bit(sig);p.pending_from[sig as usize]=sender;}).is_some(){queued+=1;}
  /* SIGCONT resumes a stopped task. */
  if sig==SIGCONT{task::continue_pid(t);}
 }
 if queued==0{return Err(errno::ESRCH)}
 for i in 0..targets.count{task::wake_pid(targets.pids[i]);}
 Ok(())
}

/// Reads the installed action of a signal.
pub fn action_get(sig:i32)->Result<Action,i64>{
 if !valid(sig){return Err(errno::EINVAL)}
 task::with_current_process(|p|Ok(p.handlers[sig as usize])).ok_or(errno::ESRCH)
}

/// Installs the action of a signal.
pub fn action_set(sig:i32,a:Action)->Result<(),i64>{
 if !valid(sig){return Err(errno::EINVAL)}
 task::with_current_process(|p|{p.handlers[sig as usize]=a;Ok::<(),i64>(())}).ok_or(errno::ESRCH)?;
 Ok(())
}

/// Applies a `rt_sigprocmask` operation and returns the previous mask.
pub fn mask_op(how:u64,set:u64)->Result<u64,i64>{
 task::with_current_process(|p|{
  let old=p.sigmask;
  match how{
   BLOCK=>p.sigmask|=set,
   UNBLOCK=>p.sigmask&=!set,
   SETMASK=>p.sigmask=set,
   _=>return Err(errno::EINVAL),
  }
  Ok(old)
 }).ok_or(errno::ESRCH)?
}

/// The lowest pending signal that is not blocked.
pub fn pending()->Option<i64>{
 task::with_current_process(|p|{
  let m=p.pending&!p.sigmask;
  if m==0{None}else{Some(m.trailing_zeros()as i64+1)}
 }).flatten()
}

/// The raw pending mask of the current process.
pub fn pending_mask()->u64{task::with_current_process(|p|p.pending).unwrap_or(0)}

/// True when a signal is queued for delivery right now.
pub fn has_pending()->bool{pending().is_some()}

/// Removes a signal from the pending set.
pub fn pop(sig:i64){task::with_current_process(|p|{p.pending&=!bit(sig as i32);});}

/// The size of a complete signal frame on the user stack.
pub fn frame_size()->usize{8+core::mem::size_of::<UContext>()+core::mem::size_of::<SigInfo>()}

/// Applies the default action of a signal that has no handler. A terminating
/// action never returns; a stopping action returns when the task continues.
fn apply_default(sig:i32){
 match default_action(sig){
  DefaultAction::Ignore=>{pop(sig);}
  DefaultAction::Stop=>{pop(sig);task::stop_current();}
  DefaultAction::Terminate=>task::exit_with_signal(sig),
 }
}

/// Prepares the next deliverable signal. Returns the handler entry context, or
/// `None` when nothing is pending or the signal was consumed by a default
/// action.
pub fn prepare_delivery()->Option<Delivery>{
 loop{
  let sig=match pending(){Some(s)=>s,None=>return None};
  let action=match action_get(sig as i32){Ok(a)=>a,Err(_)=>{pop(sig);continue}};
  if action.handler==SIG_IGN{pop(sig);continue}
  if action.handler==SIG_DFL{apply_default(sig as i32);continue}
  match build_frame(sig as i32,action){
   Ok(d)=>return Some(d),
   Err(_)=>{task::exit_with_signal(SIGSEGV);None}
  }
 }
}

/// Builds the handler frame on the user stack and rewrites the signal mask.
fn build_frame(sig:i32,action:Action)->Result<Delivery,i64>{
 let snap=syscall::capture_user_frame().ok_or(errno::EFAULT)?;
 let size=frame_size() as u64;
 if snap.rsp<0x2000+size{return Err(errno::EFAULT)}
 if action.handler<0x1000||action.handler>=crate::usercopy::USER_MAX as u64{return Err(errno::EFAULT)}
 if action.restorer<0x1000||action.restorer>=crate::usercopy::USER_MAX as u64{return Err(errno::EFAULT)}
 /* The handler entry stack pointer must be 8 modulo 16 so that the handler
    frame keeps the System V alignment. */
 let x=((snap.rsp-size-8)&!15)+8;
 let saved_mask=task::with_current_process(|p|p.sigmask).unwrap_or(0);
 let sender=task::with_current_process(|p|p.pending_from[sig as usize]).unwrap_or(0);
 let mut uc=UContext::default();
 /* The syscall entry keeps the interrupted r8 but not the interrupted r11 and
    rcx; those two slots report zero. */
 uc.uc_mcontext=SigContext{
  r8:snap.r8,r9:snap.r9,r10:snap.r10,r11:0,r12:snap.r12,r13:snap.r13,r14:snap.r14,r15:snap.r15,
  rdi:snap.rdi,rsi:snap.rsi,rbp:snap.rbp,rbx:snap.rbx,rdx:snap.rdx,rax:snap.rax,
  rcx:0,rsp:snap.rsp,rip:snap.rip,eflags:snap.rflags,
  cs:crate::arch::x86_64::descriptors::UCODE as u16,gs:0,fs:0,ss:crate::arch::x86_64::descriptors::UDATA as u16,
  err:0,trapno:0,oldmask:saved_mask,cr2:0,fpstate:0,reserved1:[0;8]};
 uc.uc_sigmask=saved_mask;
 let mut info=SigInfo::default();
 info.si_signo=sig;info.si_errno=0;info.si_code=SI_USER;info.si_pid=sender as i32;info.si_uid=0;
 let uc_addr=x+8;
 let info_addr=uc_addr+core::mem::size_of::<UContext>() as u64;
 crate::usercopy::copy_to_user(x as usize,&action.restorer.to_ne_bytes())?;
 crate::usercopy::copy_to_user(uc_addr as usize,unsafe{core::slice::from_raw_parts((&uc as*const UContext).cast::<u8>(),core::mem::size_of::<UContext>())})?;
 crate::usercopy::copy_to_user(info_addr as usize,unsafe{core::slice::from_raw_parts((&info as*const SigInfo).cast::<u8>(),core::mem::size_of::<SigInfo>())})?;
 let mut mask=saved_mask|action.mask;
 if action.flags&SA_NODEFER==0{mask&=!bit(sig)}
 task::with_current_process(|p|p.sigmask=mask);
 if action.flags&SA_RESETHAND!=0{
  let reset=Action{handler:SIG_DFL,flags:0,mask:0,restorer:0};
  let _=action_set(sig,reset);
 }
 let flags=(snap.rflags&!(1u64<<8))|(1u64<<9);
 /* A handler without SA_SIGINFO receives no siginfo pointer, the same way a
    Linux handler is called with a single argument. */
 let siginfo=if action.flags&SA_SIGINFO!=0{info_addr}else{0};
 Ok(Delivery{rip:action.handler,rsp:x,rflags:flags,sig:sig,siginfo:siginfo,ucontext:uc_addr})
}

/// Restores the interrupted context from a handler frame. The frame pointer is
/// the value the user stack held when the restorer entered the kernel.
pub fn restore_frame()->Result<(),i64>{
 let sp=unsafe{syscall::SYSCALL_USER_RSP};
 if sp<0x2000{return Err(errno::EFAULT)}
 let base=sp-8;
 let uc_addr=base+8;
 let mut uc=UContext::default();
 crate::usercopy::copy_from_user(unsafe{core::slice::from_raw_parts_mut((&mut uc as*mut UContext).cast::<u8>(),core::mem::size_of::<UContext>())},uc_addr as usize)?;
 let m=uc.uc_mcontext;
 if m.rip<0x1000||m.rip>=crate::usercopy::USER_MAX as u64{return Err(errno::EFAULT)}
 if m.rsp<0x1000||m.rsp>=crate::usercopy::USER_MAX as u64{return Err(errno::EFAULT)}
 let frame=UserFrame{
  r15:m.r15,r14:m.r14,r13:m.r13,r12:m.r12,rbp:m.rbp,rbx:m.rbx,r10:m.r10,r9:m.r9,r8:m.r8,
  rdx:m.rdx,rsi:m.rsi,rdi:m.rdi,rax:m.rax,rip:m.rip,rflags:m.eflags,rsp:m.rsp};
 task::with_current_process(|p|p.sigmask=uc.uc_sigmask);
 if !syscall::set_return_context(&frame){return Err(errno::EFAULT)}
 Ok(())
}
