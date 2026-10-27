use crate::linux_abi;
use super::{descriptors,msr};
fn pc()->*mut crate::smp::PerCpuData{crate::smp::per_cpu_ptr(crate::smp::current_cpu())}

unsafe extern "C" { fn syscall_entry(); }
/* Entry point of the saved user register frame, published for fork. */
/* Scratch cell for the dispatcher result between the dispatcher call and the
   user-mode return. */
/* The value rt_sigreturn restores into the user register file. It is
   published separately because the dispatcher writes the scratch cell above
   after the syscall returns. */
/* User selectors used by the IRETQ return path of the syscall entry. */
#[unsafe(no_mangle)] pub static USER_CS:u64=descriptors::UCODE as u64;
#[unsafe(no_mangle)] pub static USER_SS:u64=descriptors::UDATA as u64;

/* Register snapshot taken while a syscall is being serviced. Field order and
   offsets are the on-stack frame layout in syscall.S. */
#[derive(Clone,Copy)]
#[repr(C)]
pub struct UserFrame{pub r15:u64,pub r14:u64,pub r13:u64,pub r12:u64,pub rbp:u64,pub rbx:u64,pub r10:u64,pub r9:u64,pub r8:u64,pub rdx:u64,pub rsi:u64,pub rdi:u64,pub rax:u64,pub rip:u64,pub rflags:u64,pub rsp:u64}

pub unsafe fn init(kernel_stack_top:u64){
 unsafe{
  (*pc()).syscall_kernel_rsp=kernel_stack_top;
  let mut efer=msr::rdmsr(msr::IA32_EFER);efer|=1|(1<<11);msr::wrmsr(msr::IA32_EFER,efer);
  let star=((descriptors::UCODE as u64-16)<<48)|((descriptors::KCODE as u64)<<32);
  msr::wrmsr(msr::IA32_STAR,star);msr::wrmsr(msr::IA32_LSTAR,syscall_entry as usize as u64);
  msr::wrmsr(msr::IA32_FMASK,(1<<9)|(1<<10));
 }
}
#[unsafe(no_mangle)]
pub extern "sysv64" fn syscall_dispatch_raw(nr:u64,a1:u64,a2:u64,a3:u64,a4:u64,a5:u64,a6:u64)->i64{
  if nr>=200||nr==41||nr==42||nr==43||nr==49||nr==50||nr==57{}
 crate::task::stack_guard();
 crate::task::note_syscall();
 /* A SIGSTOP requested while running user code takes effect before the next
    syscall executes, so a SIGSTOP to a syscall-heavy task does not wait for
    its return path. stop_current parks; on SIGCONT the syscall runs. */
 if let Some(sig)=crate::task::stop_requested(){crate::task::stop_current(sig);}
 let r=linux_abi::dispatch(nr,[a1,a2,a3,a4,a5,a6]);
 /* Yield point for explicit reschedule requests (reschedule IPIs and
    lifecycle kicks): hand over when asked. Broadcast preemption on every
    syscall is deliberately avoided; the 1ms timer trap bounds the
    preemption latency for CPU-bound tasks instead. */
 crate::task::yield_if_needed();
 r
}
#[unsafe(no_mangle)]pub extern "sysv64" fn bad_sysret(rip:u64,rsp:u64)->!{crate::println!("rejected SYSRET rip={:#x} rsp={:#x}",rip,rsp);crate::arch::halt()}

pub unsafe fn set_kernel_stack(value:u64){unsafe{(*pc()).syscall_kernel_rsp=value}}
pub unsafe fn commit_exec_return(rip:u64,rsp:u64){unsafe{let p=pc();(*p).exec_rip=rip;(*p).syscall_user_rsp=rsp;(*p).exec_pending=1;}}
/* Offsets of the on-stack user register frame saved by syscall_entry. */
const F_R15:usize=0;const F_R14:usize=1;const F_R13:usize=2;const F_R12:usize=3;const F_RBP:usize=4;const F_RBX:usize=5;
const F_R10:usize=6;const F_R9:usize=7;const F_R8:usize=8;const F_RDX:usize=9;const F_RSI:usize=10;const F_RDI:usize=11;
const F_RAX:usize=12;const F_RCX:usize=13;const F_R11:usize=14;
/* Size and layout of the published Linux rt_sigframe. */
fn canonical_user(v:u64)->bool{v>=crate::usercopy::USER_MIN as u64&&v<crate::usercopy::USER_MAX as u64}
/* Runs before the syscall returns to user mode. Returns 1 when a signal was
   prepared and the user context was rewritten for the handler entry. */
#[unsafe(no_mangle)]
pub extern "sysv64" fn signal_pending_check(frame:*mut u64)->u64{
 if frame.is_null(){return 0}
 match crate::signal::prepare_delivery(frame){
  None=>0,
  Some(d)=>{
   let mut ctx=match capture_user_frame(frame){Some(f)=>f,None=>return 0};
   ctx.rip=d.rip;ctx.rsp=d.rsp;ctx.rflags=d.rflags;
   ctx.rdi=d.sig as u64;ctx.rsi=d.siginfo;ctx.rdx=d.ucontext;
   if set_return_context(frame,&ctx){1}else{0}
  }
 }
}
/* Copies the in-flight syscall frame into a resume snapshot whose RAX is 0,
    the value fork must return in the child. Callers run in syscall context. */
pub fn capture_fork_frame()->Option<UserFrame>{
 let f=unsafe{(*pc()).syscall_frame as *const u64};if f.is_null(){return None}
 unsafe{
  Some(UserFrame{
   r15:*f.add(0),r14:*f.add(1),r13:*f.add(2),r12:*f.add(3),rbp:*f.add(4),rbx:*f.add(5),
   r10:*f.add(6),r9:*f.add(7),r8:*f.add(8),rdx:*f.add(9),rsi:*f.add(10),rdi:*f.add(11),
   rax:0,rip:*f.add(13),rflags:*f.add(14),rsp:unsafe{(*pc()).syscall_user_rsp},
  })
 }
}

/* rt_sigreturn: restores the interrupted context from the handler frame the
   kernel wrote before entering the handler. */
pub unsafe fn signal_restore_frame()->i64{
 match crate::signal::restore_frame(){Ok(())=>0,Err(e)=>crate::errno::neg(e)}
}

/* Copies the in-flight syscall frame into a snapshot of the interrupted user
   context. Callers run in syscall context. */
pub fn capture_user_frame(frame:*mut u64)->Option<UserFrame>{
 let f=frame as *const u64;if f.is_null(){return None}
 unsafe{
  Some(UserFrame{
   r15:*f.add(0),r14:*f.add(1),r13:*f.add(2),r12:*f.add(3),rbp:*f.add(4),rbx:*f.add(5),
   r10:*f.add(6),r9:*f.add(7),r8:*f.add(8),rdx:*f.add(9),rsi:*f.add(10),rdi:*f.add(11),
   rax:*f.add(12),rip:*f.add(13),rflags:*f.add(14),rsp:unsafe{(*pc()).syscall_user_rsp},
  })
 }
}

/* Rewrites the user context that the syscall return path restores. */
pub fn set_return_context(frame:*mut u64,ctx:&UserFrame)->bool{
 let f=frame;if f.is_null(){return false}
 unsafe{
  *f.add(0)=ctx.r15;*f.add(1)=ctx.r14;*f.add(2)=ctx.r13;*f.add(3)=ctx.r12;
  *f.add(4)=ctx.rbp;*f.add(5)=ctx.rbx;*f.add(6)=ctx.r10;*f.add(7)=ctx.r9;
  *f.add(8)=ctx.r8;*f.add(9)=ctx.rdx;*f.add(10)=ctx.rsi;*f.add(11)=ctx.rdi;
  *f.add(12)=ctx.rax;*f.add(13)=ctx.rip;*f.add(14)=ctx.rflags;
  unsafe{(*pc()).syscall_user_rsp=ctx.rsp;}
 }
 true
}

/* Per-task user RSP accessors used by the scheduler when switching between
   threads that share the global syscall frame slot. */
pub unsafe fn read_user_rsp()->u64{unsafe{(*pc()).syscall_user_rsp}}
pub unsafe fn write_user_rsp(v:u64){unsafe{(*pc()).syscall_user_rsp=v}}
/* Per-CPU syscall cells that are live across a context switch (set at entry
   or by exec/sigreturn, consumed at return). The scheduler parks them into
   the outgoing task and restores the incoming task's copies, so a switch
   between the write and the read cannot observe another task's values. */
pub unsafe fn read_syscall_frame()->u64{unsafe{(*pc()).syscall_frame}}
pub unsafe fn write_syscall_frame(v:u64){unsafe{(*pc()).syscall_frame=v}}
pub unsafe fn read_exec_rip()->u64{unsafe{(*pc()).exec_rip}}
pub unsafe fn write_exec_rip(v:u64){unsafe{(*pc()).exec_rip=v}}
pub unsafe fn read_exec_pending()->bool{unsafe{(*pc()).exec_pending!=0}}
pub unsafe fn write_exec_pending(v:bool){unsafe{(*pc()).exec_pending=v as u8}}
pub unsafe fn read_sigret_value()->u64{unsafe{(*pc()).sigret_value}}
pub unsafe fn write_sigret_value(v:u64){unsafe{(*pc()).sigret_value=v}}
pub unsafe fn read_sigret_valid()->bool{unsafe{(*pc()).sigret_valid!=0}}
pub unsafe fn write_sigret_valid(v:bool){unsafe{(*pc()).sigret_valid=v as u8}}
/* Whether live XMM is kernel scratch with user state parked (see fpu_parked).
   The scheduler diverts its save to scratch while set. */
pub fn read_fpu_parked()->bool{unsafe{(*crate::smp::per_cpu_ptr(crate::smp::current_cpu())).fpu_parked!=0}}
pub fn write_fpu_parked(v:bool){unsafe{(*crate::smp::per_cpu_ptr(crate::smp::current_cpu())).fpu_parked=v as u8}}
