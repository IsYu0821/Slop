//! Linux signal ABI: kill, rt_sigaction, rt_sigprocmask and rt_sigreturn.
//!
//! Per-process pending masks, blocked masks and handler slots live in
//! Process. Signal delivery builds the published x86-64 `rt_sigframe` on the
//! user stack: the restorer address occupies the lowest slot, the
//! `ucontext_t` follows and the `siginfo_t` closes the frame. The layout
//! facts are recorded in `docs/compat-ledger.md`.
use crate::arch::x86_64::syscall::{self,UserFrame};
use crate::task;
pub const SIGHUP:i32=1;pub const SIGINT:i32=2;pub const SIGQUIT:i32=3;pub const SIGILL:i32=4;pub const SIGTRAP:i32=5;pub const SIGABRT:i32=6;pub const SIGBUS:i32=7;pub const SIGFPE:i32=8;pub const SIGKILL:i32=9;pub const SIGUSR1:i32=10;pub const SIGSEGV:i32=11;pub const SIGUSR2:i32=12;pub const SIGPIPE:i32=13;pub const SIGALRM:i32=14;pub const SIGTERM:i32=15;pub const SIGCHLD:i32=17;pub const SIGCONT:i32=18;pub const SIGSTOP:i32=19;pub const SIGTSTP:i32=20;pub const SIGTTIN:i32=21;pub const SIGTTOU:i32=22;pub const SIGURG:i32=23;pub const SIGVTALRM:i32=26;pub const SIGPROF:i32=27;pub const SIGWINCH:i32=28;
pub const SIG_DFL:u64=0;pub const SIG_IGN:u64=1;
pub const SA_NOCLDSTOP:u64=1;pub const SA_NOCLDWAIT:u64=2;pub const SA_SIGINFO:u64=4;pub const SA_ONSTACK:u64=0x08000000;pub const SA_RESTORER:u64=0x04000000;pub const SA_RESTART:u64=0x10000000;pub const SA_NODEFER:u64=0x40000000;pub const SA_RESETHAND:u64=0x80000000;
pub const BLOCK:u64=0;pub const UNBLOCK:u64=1;pub const SETMASK:u64=2;
const MAX_SIG:usize=64;
/* Linux x86-64 kernel_sigaction layout, used verbatim by rt_sigaction. */
#[derive(Clone,Copy,Debug)]
#[repr(C)]
pub struct Action{pub handler:u64,pub flags:u64,pub restorer:u64,pub mask:u64}
const DFL:Action=Action{handler:SIG_DFL,flags:0,restorer:0,mask:0};
pub fn valid(sig:i32)->bool{sig>=1&&sig<MAX_SIG as i32}
pub fn bit(sig:i32)->u64{1u64<<(sig as u64-1)}
/* Signals that cannot be blocked or ignored. */
pub fn unblockable(sig:i32)->bool{sig==SIGKILL||sig==SIGSTOP}
/* Classifies the disposition installed for a signal number. */
pub enum Disposition{Handler(u64,u64,u64),Ignore,Terminate,Stop,Continue}
fn task_index(pid:u32)->Option<usize>{let s=task::sched_pids();(0..8).find(|&i|s[i]==pid)}
/* Delivers a signal to another task by marking its pending mask. */
pub fn send(pid:u32,sig:i32)->Result<(),i64>{
 if !valid(sig){return Err(crate::errno::EINVAL)}
 if !unblockable(sig)&&task_index(pid).is_none(){return Err(crate::errno::ESRCH)}
 task::signal_pid(pid,sig)
}
/* Delivers a signal to the calling task itself. */
pub fn send_self(sig:i32)->Result<(),i64>{
 if !valid(sig){return Err(crate::errno::EINVAL)}
 task::with_current_process(|p|{if unblockable(sig){p.sigmask&=!bit(sig)}p.pending|=bit(sig);Ok::<(),i64>(())}).ok_or(crate::errno::ESRCH)?;
 Ok(())
}pub fn debug_mask()->u64{task::with_current_process(|p|p.sigmask).unwrap_or(0)}pub fn action_get(sig:i32)->Result<Action,i64>{
 if !valid(sig){return Err(crate::errno::EINVAL)}
 task::with_current_process(|p|{Ok(p.handlers[sig as usize])}).ok_or(crate::errno::ESRCH)?
}
pub fn action_set(sig:i32,a:Action)->Result<(),i64>{
 if !valid(sig){return Err(crate::errno::EINVAL)}
 if sig==SIGKILL||sig==SIGSTOP{return Err(crate::errno::EINVAL)}
 task::with_current_process(|p|{p.handlers[sig as usize]=a;Ok::<(),i64>(())}).ok_or(crate::errno::ESRCH)?;
 Ok(())
}
pub fn mask_op(how:u64,set:u64)->Result<u64,i64>{
 task::with_current_process(|p|{let old=p.sigmask;match how{BLOCK=>p.sigmask|=set,UNBLOCK=>p.sigmask&=!set,SETMASK=>p.sigmask=set,_=>return Err(crate::errno::EINVAL)};Ok(old)}).ok_or(crate::errno::ESRCH)?
}
/* Next pending, unblocked signal; unblockable signals ignore the mask. */
pub fn pending()->Option<i64>{
 task::with_current_process(|p|{let blocked=p.sigmask|bit(SIGKILL)|bit(SIGSTOP);let m=p.pending&!blocked;if m==0{None}else{Some(m.trailing_zeros()as i64+1)}}).flatten()
}
/* True when a signal is queued and deliverable, so a blocking syscall can
   report EINTR instead of sleeping through the delivery. */
pub fn has_pending()->bool{pending().is_some()}
pub fn pop(sig:i64){task::with_current_process(|p|{p.pending&=!bit(sig as i32);});}
pub fn clear_all(){task::with_current_process(|p|{p.pending=0;p.sigmask=0;});}
pub fn is_blocked(sig:i64)->bool{task::with_current_process(|p|p.sigmask&bit(sig as i32)!=0).unwrap_or(false)}
pub fn set_sigframe(addr:u64){task::with_current_process(|p|p.sigframe=addr);}
pub fn get_sigframe()->u64{task::with_current_process(|p|p.sigframe).unwrap_or(0)}
pub fn unblockable_mask()->u64{bit(SIGKILL)|bit(SIGSTOP)}
pub fn handlers_dfl()->[Action;64]{[DFL;64]}

/* True when the default disposition of sig kills the process. */
pub fn default_terminates(sig:i32)->bool{
 matches!(sig,
  SIGHUP|SIGINT|SIGQUIT|SIGILL|SIGABRT|SIGBUS|SIGFPE|SIGKILL|SIGSEGV|
  SIGPIPE|SIGALRM|SIGTERM|SIGUSR1|SIGUSR2|SIGVTALRM|SIGPROF)
}

/* The published REG_* index list of the x86-64 machine context. */
pub const REG_R8:usize=0;pub const REG_R9:usize=1;pub const REG_R10:usize=2;pub const REG_R11:usize=3;
pub const REG_R12:usize=4;pub const REG_R13:usize=5;pub const REG_R14:usize=6;pub const REG_R15:usize=7;
pub const REG_RDI:usize=8;pub const REG_RSI:usize=9;pub const REG_RBP:usize=10;pub const REG_RBX:usize=11;
pub const REG_RDX:usize=12;pub const REG_RAX:usize=13;pub const REG_RCX:usize=14;pub const REG_RSP:usize=15;
pub const REG_RIP:usize=16;pub const REG_EFL:usize=17;pub const REG_CSGSFS:usize=18;pub const REG_ERR:usize=19;
pub const REG_TRAPNO:usize=20;pub const REG_OLDMASK:usize=21;pub const REG_CR2:usize=22;

/* Linux x86-64 `mcontext_t`: the register array, the floating-point pointer
   and the reserved words. The register array is the `sigcontext` view. */
#[repr(C)]
#[derive(Clone,Copy,Default)]
pub struct MachineContext{pub gregs:[u64;23],pub fpregs:u64,pub reserved1:[u64;8]}

/* Linux x86-64 `stack_t`. */
#[repr(C)]
#[derive(Clone,Copy,Default)]
pub struct StackArea{pub ss_sp:u64,pub ss_flags:u32,pub ss_pad:u32,pub ss_size:u64}

/* Linux x86-64 `ucontext_t`. */
#[repr(C)]
#[derive(Clone,Copy)]
pub struct UContext{pub uc_flags:u64,pub uc_link:u64,pub uc_stack:StackArea,pub uc_mcontext:MachineContext,pub uc_sigmask:u64,pub fpstate:[u8;512]}
impl Default for UContext{fn default()->Self{Self{uc_flags:0,uc_link:0,uc_stack:StackArea::default(),uc_mcontext:MachineContext::default(),uc_sigmask:0,fpstate:[0;512]}}}

/* Linux `siginfo_t`: the header followed by the 128-byte union area. */
#[repr(C)]
#[derive(Clone,Copy,Default)]
pub struct SigInfo{pub si_signo:i32,pub si_errno:i32,pub si_code:i32,pub si_pad:[i32;29]}

/* The sender of a queued signal was a process. */
pub const SI_USER:i32=0;

/* The user context prepared for a handler entry. */
pub struct Delivery{pub rip:u64,pub rsp:u64,pub rflags:u64,pub sig:i32,pub siginfo:u64,pub ucontext:u64}

/* The size of a complete signal frame on the user stack. */
pub fn frame_size()->usize{8+core::mem::size_of::<UContext>()+core::mem::size_of::<SigInfo>()}

/* Prepares the next deliverable signal and returns the handler entry
   context. `None` means that nothing is pending or that the default action
   consumed the signal. */
pub fn prepare_delivery(frame:*mut u64)->Option<Delivery>{
 loop{
  let sig=match pending(){Some(s)=>s,None=>return None};
  let action=match action_get(sig as i32){Ok(a)=>a,Err(_)=>{pop(sig);continue}};
  if action.handler==SIG_IGN{pop(sig);continue}
  if action.handler==SIG_DFL{
   if default_terminates(sig as i32){crate::task::exit_signal(sig as i32);}
   pop(sig);continue;
  }
  match build_frame(sig as i32,action,frame){
   Ok(d)=>return Some(d),
   Err(_)=>{crate::task::exit_signal(SIGSEGV);return None;}
  }
 }
}

/* Builds the published rt_sigframe on the user stack: the restorer address,
   then the ucontext, then the siginfo. The handler is entered with the
   signal number, the siginfo pointer and the ucontext pointer. */
fn build_frame(sig:i32,action:Action,frame:*mut u64)->Result<Delivery,i64>{
 let snap=syscall::capture_user_frame(frame).ok_or(crate::errno::EFAULT)?;
 let size=frame_size() as u64;
 if snap.rsp<0x4000+size{return Err(crate::errno::EFAULT)}
 if action.handler<0x1000||action.handler>=crate::usercopy::USER_MAX as u64{return Err(crate::errno::EFAULT)}
 if action.restorer<0x1000||action.restorer>=crate::usercopy::USER_MAX as u64{return Err(crate::errno::EFAULT)}
 /* The handler entry stack pointer must be 8 modulo 16 so that the handler
    frame keeps the System V alignment. */
 let x=((snap.rsp-size-8)&!15)+8;
 let saved_mask=task::with_current_process(|p|p.sigmask).unwrap_or(0);
 let mut uc=UContext::default();
 let cs=crate::arch::x86_64::descriptors::UCODE as u64;
 let ss=crate::arch::x86_64::descriptors::UDATA as u64;
 uc.uc_mcontext.gregs[REG_R8]=snap.r8;uc.uc_mcontext.gregs[REG_R9]=snap.r9;uc.uc_mcontext.gregs[REG_R10]=snap.r10;
 /* The SYSCALL instruction clobbers RCX and R11, so the interrupted values
     are not available; Linux reports them as zero as well. */
 uc.uc_mcontext.gregs[REG_R11]=0;uc.uc_mcontext.gregs[REG_R12]=snap.r12;uc.uc_mcontext.gregs[REG_R13]=snap.r13;
 uc.uc_mcontext.gregs[REG_R14]=snap.r14;uc.uc_mcontext.gregs[REG_R15]=snap.r15;
 uc.uc_mcontext.gregs[REG_RDI]=snap.rdi;uc.uc_mcontext.gregs[REG_RSI]=snap.rsi;uc.uc_mcontext.gregs[REG_RBP]=snap.rbp;
 uc.uc_mcontext.gregs[REG_RBX]=snap.rbx;uc.uc_mcontext.gregs[REG_RDX]=snap.rdx;uc.uc_mcontext.gregs[REG_RAX]=snap.rax;
 uc.uc_mcontext.gregs[REG_RCX]=0;uc.uc_mcontext.gregs[REG_RSP]=snap.rsp;uc.uc_mcontext.gregs[REG_RIP]=snap.rip;
 /* The machine context reports the result of the interrupted syscall, the
    way Linux fills the ucontext of a handler. */
 uc.uc_mcontext.gregs[REG_RAX]=unsafe{syscall::SYSCALL_RESULT} as i64 as u64;
 uc.uc_mcontext.gregs[REG_EFL]=snap.rflags;
 uc.uc_mcontext.gregs[REG_CSGSFS]=(cs&0xffff)|((ss&0xffff)<<16);
 uc.uc_mcontext.gregs[REG_OLDMASK]=saved_mask;
 uc.uc_stack.ss_sp=snap.rsp;
 uc.uc_sigmask=saved_mask;
 let mut info=SigInfo::default();
 info.si_signo=sig;info.si_errno=0;info.si_code=SI_USER;
 /* The sender of the signal travels in the _kill member, which starts right
    after the three header words. */
 let sender=task::with_current_process(|p|p.pending_from[sig as usize]).unwrap_or(0);
 info.si_pad[0]=sender as i32;
 info.si_pad[1]=0;
 let uc_addr=x+8;
 let info_addr=uc_addr+core::mem::size_of::<UContext>() as u64;
 crate::usercopy::copy_to_user(x as usize,&action.restorer.to_ne_bytes())?;
 crate::usercopy::copy_to_user(uc_addr as usize,unsafe{core::slice::from_raw_parts((&uc as*const UContext).cast::<u8>(),core::mem::size_of::<UContext>())})?;
 crate::usercopy::copy_to_user(info_addr as usize,unsafe{core::slice::from_raw_parts((&info as*const SigInfo).cast::<u8>(),core::mem::size_of::<SigInfo>())})?;
 let mut mask=saved_mask|action.mask;
 if action.flags&SA_NODEFER==0{mask&=!bit(sig)}
 task::with_current_process(|p|p.sigmask=mask);
 /* A handler installed with SA_RESTART restarts the interrupted syscall;
    every other handler leaves the syscall with EINTR. */
 task::with_current_process(|p|{p.sig_restart=action.flags&SA_RESTART!=0;p.sig_syscall=snap.rax;p.sig_result=unsafe{syscall::SYSCALL_RESULT} as i64;});
 /* The signal leaves the pending set when it is delivered: Linux removes it
    before the handler runs, so the handler does not see it again. */
 pop(sig as i64);
 if action.flags&SA_RESETHAND!=0{
  let reset=Action{handler:SIG_DFL,flags:0,restorer:0,mask:0};
  let _=action_set(sig,reset);
 }
 /* A handler without SA_SIGINFO receives no siginfo pointer, the same way a
    Linux handler is called with a single argument. */
 let siginfo=if action.flags&SA_SIGINFO!=0{info_addr}else{0};
 let flags=(snap.rflags&!(1u64<<8))|(1u64<<9);
 Ok(Delivery{rip:action.handler,rsp:x,rflags:flags,sig:sig,siginfo:siginfo,ucontext:uc_addr})
}

/* Restores the interrupted context from the handler frame. The restorer
   enters the kernel with the stack pointer pointing just above the frame, so
   the frame starts one word below. */
pub fn restore_frame()->Result<(),i64>{
 let sp=unsafe{syscall::SYSCALL_USER_RSP};
 if sp<0x2000{return Err(crate::errno::EFAULT)}
 let base=sp-8;
 let uc_addr=base+8;
 let mut uc=UContext::default();
 crate::usercopy::copy_from_user(unsafe{core::slice::from_raw_parts_mut((&mut uc as*mut UContext).cast::<u8>(),core::mem::size_of::<UContext>())},uc_addr as usize)?;
 let g=uc.uc_mcontext.gregs;
 let rip=g[REG_RIP];let rsp=g[REG_RSP];let rflags=g[REG_EFL];
 if rip<0x1000||rip>=crate::usercopy::USER_MAX as u64{return Err(crate::errno::EFAULT)}
 if rsp<0x1000||rsp>=crate::usercopy::USER_MAX as u64{return Err(crate::errno::EFAULT)}
 /* Without SA_RESTART the interrupted syscall ends with EINTR: the user
    sees the error instead of the syscall running again. With SA_RESTART the
    syscall number stays in RAX and the user repeats the call. */
 let (restart,syscall_no,result)=task::with_current_process(|p|(p.sig_restart,p.sig_syscall,p.sig_result)).unwrap_or((false,0,0));
 /* A handler with SA_RESTART repeats the interrupted syscall: the syscall
    number stays in RAX so the user repeats the call. Every other handler
    leaves the syscall result in RAX, which is EINTR when the syscall was
    interrupted. */
 let rax=if restart{syscall_no}else{result as u64};
 let frame=UserFrame{
  r15:g[REG_R15],r14:g[REG_R14],r13:g[REG_R13],r12:g[REG_R12],rbp:g[REG_RBP],rbx:g[REG_RBX],
  r10:g[REG_R10],r9:g[REG_R9],r8:g[REG_R8],rdx:g[REG_RDX],rsi:g[REG_RSI],rdi:g[REG_RDI],
  rax:rax,rip:rip,rflags:rflags,rsp:rsp};
 task::with_current_process(|p|p.sigmask=uc.uc_sigmask);
 if !syscall::set_return_context(unsafe{syscall::SYSCALL_FRAME as *mut u64},&frame){return Err(crate::errno::EFAULT)}
 /* The return path takes the user-mode result from this cell, so the
    restored value has to be published here. */
 unsafe{syscall::SIGRET_VALUE=rax;syscall::SIGRET_VALID=1;}
 Ok(())
}
