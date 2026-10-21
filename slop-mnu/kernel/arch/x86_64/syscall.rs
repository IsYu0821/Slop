use crate::linux_abi;
use super::{descriptors,msr};

unsafe extern "C" { fn syscall_entry(); }
#[unsafe(no_mangle)] pub static mut SYSCALL_KERNEL_RSP:u64=0;
#[unsafe(no_mangle)] pub static mut SYSCALL_USER_RSP:u64=0;
/* Entry point of the saved user register frame, published for fork. */
#[unsafe(no_mangle)] pub static mut SYSCALL_FRAME:u64=0;
/* User selectors used by the IRETQ return path of the syscall entry. */
#[unsafe(no_mangle)] pub static USER_CS:u64=descriptors::UCODE as u64;
#[unsafe(no_mangle)] pub static USER_SS:u64=descriptors::UDATA as u64;

#[unsafe(no_mangle)] pub static mut EXEC_PENDING:u8=0;
#[unsafe(no_mangle)] pub static mut EXEC_RIP:u64=0;

/* Register snapshot taken while a syscall is being serviced. Field order and
   offsets are the on-stack frame layout in syscall.S. */
#[derive(Clone,Copy)]
#[repr(C)]
pub struct UserFrame{pub r15:u64,pub r14:u64,pub r13:u64,pub r12:u64,pub rbp:u64,pub rbx:u64,pub r10:u64,pub r9:u64,pub r8:u64,pub rdx:u64,pub rsi:u64,pub rdi:u64,pub rax:u64,pub rip:u64,pub rflags:u64,pub rsp:u64}

pub unsafe fn init(kernel_stack_top:u64){
  unsafe{
   SYSCALL_KERNEL_RSP=kernel_stack_top;
   let mut efer=msr::rdmsr(msr::IA32_EFER);efer|=1|(1<<11);msr::wrmsr(msr::IA32_EFER,efer);
   let star=((descriptors::UCODE as u64-16)<<48)|((descriptors::KCODE as u64)<<32);
   msr::wrmsr(msr::IA32_STAR,star);msr::wrmsr(msr::IA32_LSTAR,syscall_entry as usize as u64);
   msr::wrmsr(msr::IA32_FMASK,(1<<9)|(1<<10));
  }
}
#[unsafe(no_mangle)]
pub extern "sysv64" fn syscall_dispatch_raw(nr:u64,a1:u64,a2:u64,a3:u64,a4:u64,a5:u64,a6:u64)->i64{linux_abi::dispatch(nr,[a1,a2,a3,a4,a5,a6])}
#[unsafe(no_mangle)]pub extern "sysv64" fn bad_sysret(rip:u64,rsp:u64)->!{crate::println!("rejected syscall return rip={:#x} rsp={:#x}",rip,rsp);crate::arch::halt()}

pub unsafe fn set_kernel_stack(value:u64){unsafe{SYSCALL_KERNEL_RSP=value}}

/* Copies the in-flight syscall frame into a resume snapshot whose RAX is 0,
   the value fork must return in the child. Callers run in syscall context. */
pub fn capture_fork_frame()->Option<UserFrame>{
  let f=unsafe{SYSCALL_FRAME as *const u64};if f.is_null(){return None}
  unsafe{
   Some(UserFrame{
    r15:*f.add(0),r14:*f.add(1),r13:*f.add(2),r12:*f.add(3),rbp:*f.add(4),rbx:*f.add(5),
    r10:*f.add(6),r9:*f.add(7),r8:*f.add(8),rdx:*f.add(9),rsi:*f.add(10),rdi:*f.add(11),
    rax:0,rip:*f.add(13),rflags:*f.add(14),rsp:SYSCALL_USER_RSP,
   })
  }
}

/* Copies the in-flight syscall frame into a snapshot of the interrupted user
   context. Callers run in syscall context. */
pub fn capture_user_frame()->Option<UserFrame>{
  let f=unsafe{SYSCALL_FRAME as *const u64};if f.is_null(){return None}
  unsafe{
   Some(UserFrame{
    r15:*f.add(0),r14:*f.add(1),r13:*f.add(2),r12:*f.add(3),rbp:*f.add(4),rbx:*f.add(5),
    r10:*f.add(6),r9:*f.add(7),r8:*f.add(8),rdx:*f.add(9),rsi:*f.add(10),rdi:*f.add(11),
    rax:*f.add(12),rip:*f.add(13),rflags:*f.add(14),rsp:SYSCALL_USER_RSP,
   })
  }
}

/* Rewrites the user context that the syscall return path restores. The frame
   keeps the interrupted register values unless the caller replaces them. */
pub fn set_return_context(frame:&UserFrame)->bool{
  let f=unsafe{SYSCALL_FRAME as *const u64};if f.is_null(){return false}
  unsafe{
   *f.add(0)=frame.r15;*f.add(1)=frame.r14;*f.add(2)=frame.r13;*f.add(3)=frame.r12;
   *f.add(4)=frame.rbp;*f.add(5)=frame.rbx;*f.add(6)=frame.r10;*f.add(7)=frame.r9;
   *f.add(8)=frame.r8;*f.add(9)=frame.rdx;*f.add(10)=frame.rsi;*f.add(11)=frame.rdi;
   *f.add(12)=frame.rax;*f.add(13)=frame.rip;*f.add(14)=frame.rflags;
   SYSCALL_USER_RSP=frame.rsp;
  }
  true
}

/* Runs before the syscall returns to user mode. Returns 1 when a signal was
   prepared and the user context was rewritten for the handler entry. */
#[unsafe(no_mangle)]
pub extern "sysv64" fn signal_pending_check()->u64{
  match crate::signal::prepare_delivery(){
   None=>0,
   Some(d)=>{
    let mut frame=match capture_user_frame(){Some(f)=>f,None=>return 0};
    frame.rip=d.rip;frame.rsp=d.rsp;frame.rflags=d.rflags;frame.rax=0;
    frame.rdi=d.sig as u64;frame.rsi=d.siginfo;frame.rdx=d.ucontext;
    if set_return_context(&frame){1}else{0}
   }
  }
}

pub unsafe fn commit_exec_return(rip:u64,rsp:u64){unsafe{EXEC_RIP=rip;SYSCALL_USER_RSP=rsp;EXEC_PENDING=1;}}
