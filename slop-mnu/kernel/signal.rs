//! Linux signal ABI: kill, rt_sigaction, rt_sigprocmask and rt_sigreturn.
//!
//! Per-process pending masks, blocked masks and handler slots live in
//! Process. Signal delivery builds the published x86-64 `rt_sigframe` on the
//! user stack: the restorer address occupies the lowest slot, the
//! `ucontext_t` follows and the `siginfo_t` closes the frame. The layout
//! facts are recorded in `docs/compat-ledger.md`.
use crate::arch::x86_64::syscall::{self,UserFrame};
use crate::task;
pub const SIGHUP:i32=1;pub const SIGINT:i32=2;pub const SIGQUIT:i32=3;pub const SIGILL:i32=4;pub const SIGTRAP:i32=5;pub const SIGABRT:i32=6;pub const SIGBUS:i32=7;pub const SIGFPE:i32=8;pub const SIGKILL:i32=9;pub const SIGUSR1:i32=10;pub const SIGSEGV:i32=11;pub const SIGUSR2:i32=12;pub const SIGPIPE:i32=13;pub const SIGALRM:i32=14;pub const SIGTERM:i32=15;pub const SIGSTKFLT:i32=16;pub const SIGCHLD:i32=17;pub const SIGCONT:i32=18;pub const SIGSTOP:i32=19;pub const SIGTSTP:i32=20;pub const SIGTTIN:i32=21;pub const SIGTTOU:i32=22;pub const SIGURG:i32=23;pub const SIGXCPU:i32=24;pub const SIGXFSZ:i32=25;pub const SIGVTALRM:i32=26;pub const SIGPROF:i32=27;pub const SIGWINCH:i32=28;pub const SIGIO:i32=29;pub const SIGPWR:i32=30;pub const SIGSYS:i32=31;
pub const SIG_DFL:u64=0;pub const SIG_IGN:u64=1;
pub const SA_NOCLDSTOP:u64=1;pub const SA_NOCLDWAIT:u64=2;pub const SA_SIGINFO:u64=4;pub const SA_ONSTACK:u64=0x08000000;pub const SA_RESTORER:u64=0x04000000;pub const SA_RESTART:u64=0x10000000;pub const SA_NODEFER:u64=0x40000000;pub const SA_RESETHAND:u64=0x80000000;
pub const BLOCK:u64=0;pub const UNBLOCK:u64=1;pub const SETMASK:u64=2;
/* The real-time signals run from 32 to 64 and are queued: every one of them
   carries a value and more than one of them can wait at the same time. */
pub const SIGRTMIN:i32=32;pub const SIGRTMAX:i32=64;
const MAX_SIG:usize=65;
/* The number of instances of one real-time signal that can wait. */
pub const RT_DEPTH:usize=4;
/* Linux x86-64 kernel_sigaction layout, used verbatim by rt_sigaction. */
#[derive(Clone,Copy,Debug)]
#[repr(C)]
pub struct Action{pub handler:u64,pub flags:u64,pub restorer:u64,pub mask:u64}
const DFL:Action=Action{handler:SIG_DFL,flags:0,restorer:0,mask:0};
pub fn valid(sig:i32)->bool{sig>=1&&sig<=SIGRTMAX}
/* True for a signal of the real-time range. */
pub fn is_rt(sig:i32)->bool{sig>=SIGRTMIN&&sig<=SIGRTMAX}
/* The index of a real-time signal in the queue tables. */
pub fn rt_index(sig:i32)->usize{(sig-SIGRTMIN) as usize}
pub fn bit(sig:i32)->u64{1u64<<(sig as u64-1)}
/* Signals that cannot be blocked or ignored. */
pub fn unblockable(sig:i32)->bool{sig==SIGKILL||sig==SIGSTOP}
/* Classifies the disposition installed for a signal number. */
pub enum Disposition{Handler(u64,u64,u64),Ignore,Terminate,Stop,Continue}
pub fn task_index(pid:u32)->Option<usize>{
 /* The pid has to name a live scheduler slot. */
 let s=task::sched_pids();
 s.iter().position(|&p|p==pid)
}
/* Delivers a signal to another task by marking its pending mask. */
pub fn send(pid:u32,sig:i32)->Result<(),i64>{
 if !valid(sig){return Err(crate::errno::EINVAL)}
 if !unblockable(sig)&&task_index(pid).is_none(){return Err(crate::errno::ESRCH)}
 task::signal_pid(pid,sig)
}
/* Sends a signal that carries a value. A real-time signal is queued, so
   more than one of them can wait at the same time; the other signals
   coalesce into one. */
/* Reads the value of a queued real-time signal, the way the siginfo reports it. */
pub fn send_value_value(p:&mut crate::process::Process,sig:i32)->u64{
 if is_rt(sig){
  let i=rt_index(sig);
  if p.rt_len[i]>0{p.rt_value[i][0]}else{0}
 }else{0}
}
pub fn send_value(pid:u32,sig:i32,value:u64)->Result<(),i64>{
 if !valid(sig){return Err(crate::errno::EINVAL)}
 if is_rt(sig){
  if task_index(pid).is_none(){return Err(crate::errno::ESRCH)}
  return task::queue_signal(pid,sig,value);
 }
 /* The standard signals coalesce: the value is not kept per instance. */
 let sender=crate::task::current_pid();
 task::signal_pid_value(pid,sig,value,sender)
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
 task::with_current_process(|p|{let blocked=p.sigmask&!unblockable_mask();let m=p.pending&!blocked;if m==0{None}else{Some(m.trailing_zeros()as i64+1)}}).flatten()
}
/* True when a signal is queued and deliverable, so a blocking syscall can
   report EINTR instead of sleeping through the delivery. */
pub fn has_pending()->bool{pending().is_some()}
pub fn pop(sig:i64){task::with_current_process(|p|{
 p.pending&=!bit(sig as i32);
 /* Consuming the coalesced SIGCHLD drops the stored child status too, so a
    later event starts from a clean slot. Child task states are untouched:
    wait4/waitid still reap the stopped or zombie child afterwards. */
 if sig as i32==SIGCHLD{p.sch_code=0;p.sch_pid=0;p.sch_uid=0;p.sch_status=0;}
 if is_rt(sig as i32){
  /* A real-time signal keeps the instances that were not delivered yet. */
  let i=rt_index(sig as i32);
  if p.rt_len[i]>0{
   p.rt_len[i]-=1;
   let n=p.rt_len[i] as usize;
   let mut k=0;while k<n{p.rt_value[i][k]=p.rt_value[i][k+1];k+=1;}
   if p.rt_len[i]>0{p.pending|=bit(sig as i32);}
  }
 }
});}
pub fn clear_all(){task::with_current_process(|p|{p.pending=0;p.sigmask=0;});}
pub fn is_blocked(sig:i64)->bool{task::with_current_process(|p|p.sigmask&bit(sig as i32)!=0).unwrap_or(false)}
pub fn set_sigframe(addr:u64){task::with_current_process(|p|p.sigframe=addr);}
pub fn get_sigframe()->u64{task::with_current_process(|p|p.sigframe).unwrap_or(0)}
pub fn unblockable_mask()->u64{bit(SIGKILL)|bit(SIGSTOP)}
pub fn handlers_dfl()->[Action;65]{[DFL;65]}

/* True when the default disposition of sig kills the process. */
pub fn default_terminates(sig:i32)->bool{
 matches!(sig,
  SIGHUP|SIGINT|SIGQUIT|SIGILL|SIGTRAP|SIGABRT|SIGBUS|SIGFPE|SIGKILL|SIGUSR1|SIGSEGV|
  SIGUSR2|SIGPIPE|SIGALRM|SIGTERM|SIGSTKFLT|SIGXCPU|SIGXFSZ|SIGVTALRM|SIGPROF|SIGIO|SIGPWR|SIGSYS)
 ||(sig>=SIGRTMIN&&sig<=SIGRTMAX)
}
/* True when the default disposition stops the task for job control. */
pub fn default_stops(sig:i32)->bool{
 matches!(sig,SIGSTOP|SIGTSTP|SIGTTIN|SIGTTOU)
}
/* True when the default disposition resumes a stopped task. */
pub fn default_continues(sig:i32)->bool{sig==SIGCONT}
/* True when the default disposition ignores the signal. */
pub fn default_ignores(sig:i32)->bool{
 matches!(sig,SIGCHLD|SIGURG|SIGWINCH)
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
/* The signal was sent by sigqueue and carries a value. */
pub const SI_QUEUE:i32=-2;
/* Child-status codes for SIGCHLD/waitid, matching the Linux CLD_* list. */
pub const CLD_EXITED:i32=1;pub const CLD_KILLED:i32=2;pub const CLD_DUMPED:i32=3;
pub const CLD_TRAPPED:i32=4;pub const CLD_STOPPED:i32=5;pub const CLD_CONTINUED:i32=6;

/* The user context prepared for a handler entry. */
pub struct Delivery{pub rip:u64,pub rsp:u64,pub rflags:u64,pub sig:i32,pub siginfo:u64,pub ucontext:u64}

/* The size of a complete signal frame on the user stack. */
pub fn frame_size()->usize{8+core::mem::size_of::<UContext>()+core::mem::size_of::<SigInfo>()}
/* The flags of the alternate stack. */
pub const SS_ONSTACK:u64=1;pub const SS_DISABLE:u64=2;
/* The smallest alternate stack a program may ask for. */
pub const MINSIGSTKSZ:u64=2048;

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
   if default_stops(sig as i32){
    /* A blocked stop stays pending; the unblockable SIGSTOP and an
       unblocked SIGTSTP/SIGTTIN/SIGTTOU park the task here. stop_current
       consumes the signal after the Stopped store (set-first ordering). */
    crate::task::stop_current(sig as i32);
    continue;
   }
   /* SIGCONT with the default disposition only resumes; a stopped task is
      resumed by the sender path, so a running task just consumes it. */
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
 if action.handler<0x1000||action.handler>=crate::usercopy::USER_MAX as u64{return Err(crate::errno::EFAULT)}
 if action.restorer<0x1000||action.restorer>=crate::usercopy::USER_MAX as u64{return Err(crate::errno::EFAULT)}
 /* A handler that asked for it runs on the alternate stack, and the frame is
    built there. The handler entry stack pointer must be 8 modulo 16 so
    that the handler frame keeps the System V alignment. */
 let alt=task::with_current_process(|p|(p.altstack_addr,p.altstack_size,p.altstack_flags)).unwrap_or((0,0,0));
 let on_alt=action.flags&SA_ONSTACK!=0&&alt.0!=0&&alt.1>=size;
 let (x,stack_base)=if on_alt{
  let top=alt.0+alt.1;
  (((top-size-8)&!15)+8,alt.0)
 }else{
  if snap.rsp<0x4000+size{return Err(crate::errno::EFAULT)}
  (((snap.rsp-size-8)&!15)+8,snap.rsp)
 };
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
 uc.uc_mcontext.gregs[REG_RAX]=unsafe{(*crate::smp::per_cpu_ptr(crate::smp::current_cpu())).syscall_result} as i64 as u64;
 uc.uc_mcontext.gregs[REG_EFL]=snap.rflags;
 uc.uc_mcontext.gregs[REG_CSGSFS]=(cs&0xffff)|((ss&0xffff)<<16);
 uc.uc_mcontext.gregs[REG_OLDMASK]=saved_mask;
 uc.uc_stack.ss_sp=stack_base;
 uc.uc_sigmask=saved_mask;
 let mut info=SigInfo::default();
 info.si_signo=sig;info.si_errno=0;
 /* The sender of the signal travels in the _kill member, which starts right
    after the three header words. */
 let sender=task::with_current_process(|p|p.pending_from[sig as usize]).unwrap_or(0);
 info.si_pad[0]=sender as i32;
 info.si_pad[1]=0;
 if is_rt(sig){
  /* A queued signal carries its value in the _rt member, which starts
     thirty two bytes into the union: the two time fields, then the value. */
  let value=task::with_current_process(|p|{
   let i=rt_index(sig);
   if p.rt_len[i]>0{p.rt_value[i][0]}else{0}
  }).unwrap_or(0);
  info.si_code=SI_QUEUE;
  /* The value sits at the offset the C library reads as si_value. */
  info.si_pad[2]=value as u32 as i32;
  info.si_pad[3]=(value>>32) as u32 as i32;
 }else{
  let code=task::with_current_process(|p|p.si_code[sig as usize]).unwrap_or(0);
  info.si_code=code;
 }
 let uc_addr=x+8;
 let info_addr=uc_addr+core::mem::size_of::<UContext>() as u64;
 /* The floating-point state travels in the 512-byte FXSAVE area at the end
    of the ucontext. The kernel keeps only the sixteen SSE registers; they
    sit at offset 160 like Linux, with MXCSR set to its reset value. */
 {
  task::save_current_fpu();
  let mut area=[0u8;256];
  task::copy_current_fpu(&mut area);
  uc.fpstate[160..160+256].copy_from_slice(&area);
  /* FXSAVE bytes 24..28 hold MXCSR and its mask. */
  uc.fpstate[24]=0x80;uc.fpstate[25]=0x1f;uc.fpstate[28]=0xbf;uc.fpstate[29]=0xff;
  let off=core::mem::offset_of!(UContext,fpstate) as u64;
  uc.uc_mcontext.fpregs=uc_addr+off;
 }
 crate::usercopy::copy_to_user(x as usize,&action.restorer.to_ne_bytes())?;
 crate::usercopy::copy_to_user(uc_addr as usize,unsafe{core::slice::from_raw_parts((&uc as*const UContext).cast::<u8>(),core::mem::size_of::<UContext>())})?;
 crate::usercopy::copy_to_user(info_addr as usize,unsafe{core::slice::from_raw_parts((&info as*const SigInfo).cast::<u8>(),core::mem::size_of::<SigInfo>())})?;
 if on_alt{
  /* The alternate stack is in use while the handler runs, which the
     program sees when it asks for the stack. */
  task::with_current_process(|p|p.altstack_flags|=SS_ONSTACK as u32);
 }
 let mut mask=saved_mask|action.mask;
 if action.flags&SA_NODEFER==0{mask&=!bit(sig)}
 task::with_current_process(|p|p.sigmask=mask);
 /* A handler installed with SA_RESTART restarts the interrupted syscall;
    every other handler leaves the syscall with EINTR. */
 task::with_current_process(|p|{p.sig_restart=action.flags&SA_RESTART!=0;p.sig_syscall=snap.rax;p.sig_result=unsafe{(*crate::smp::per_cpu_ptr(crate::smp::current_cpu())).syscall_result} as i64;});
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
 let sp=unsafe{(*crate::smp::per_cpu_ptr(crate::smp::current_cpu())).syscall_user_rsp};
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
 task::with_current_process(|p|{p.sigmask=uc.uc_sigmask;p.altstack_flags&=!(SS_ONSTACK as u32);});
 /* A handler may have changed the SSE registers: the FXSAVE area carries
    them back into the running task so the syscall-exit restore and the next
    context switch both observe the handler-approved values. */
 {
  let mut area=[0u8;256];
  area.copy_from_slice(&uc.fpstate[160..160+256]);
  task::write_current_fpu(&area);
 }
 if !syscall::set_return_context(unsafe{(*crate::smp::per_cpu_ptr(crate::smp::current_cpu())).syscall_frame as *mut u64},&frame){return Err(crate::errno::EFAULT)}
 /* The return path takes the user-mode result from this cell, so the
    restored value has to be published here. */
 unsafe{let p=crate::smp::per_cpu_ptr(crate::smp::current_cpu());(*p).sigret_value=rax;(*p).sigret_valid=1;}
 Ok(())
}
