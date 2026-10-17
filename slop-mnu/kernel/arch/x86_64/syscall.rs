use crate::linux_abi;
use super::{descriptors,msr};

unsafe extern "C" { fn syscall_entry(); }
#[unsafe(no_mangle)] pub static mut SYSCALL_KERNEL_RSP:u64=0;
#[unsafe(no_mangle)] pub static mut SYSCALL_USER_RSP:u64=0;
/* Entry point of the saved user register frame, published for fork. */
#[unsafe(no_mangle)] pub static mut SYSCALL_FRAME:u64=0;

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
#[unsafe(no_mangle)]pub extern "sysv64" fn bad_sysret(rip:u64,rsp:u64)->!{crate::println!("rejected SYSRET rip={:#x} rsp={:#x}",rip,rsp);crate::arch::halt()}

pub unsafe fn set_kernel_stack(value:u64){unsafe{SYSCALL_KERNEL_RSP=value}}
#[unsafe(no_mangle)]pub static mut EXEC_PENDING:u8=0;
#[unsafe(no_mangle)]pub static mut EXEC_RIP:u64=0;
pub unsafe fn commit_exec_return(rip:u64,rsp:u64){unsafe{EXEC_RIP=rip;SYSCALL_USER_RSP=rsp;EXEC_PENDING=1;}}

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
