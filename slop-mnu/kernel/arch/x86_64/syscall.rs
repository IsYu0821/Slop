use crate::linux_abi;
use super::{descriptors,msr};

unsafe extern "C" { fn syscall_entry(); }
#[unsafe(no_mangle)] pub static mut SYSCALL_KERNEL_RSP:u64=0;
#[unsafe(no_mangle)] pub static mut SYSCALL_USER_RSP:u64=0;

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
