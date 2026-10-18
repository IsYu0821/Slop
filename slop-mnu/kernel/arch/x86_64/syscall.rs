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
pub extern "sysv64" fn syscall_dispatch_raw(nr:u64,a1:u64,a2:u64,a3:u64,a4:u64,a5:u64,a6:u64)->i64{
 if nr>=200||nr==41||nr==42||nr==43||nr==49||nr==50||nr==57{}
 crate::task::stack_guard();
 linux_abi::dispatch(nr,[a1,a2,a3,a4,a5,a6])
}
#[unsafe(no_mangle)]pub extern "sysv64" fn bad_sysret(rip:u64,rsp:u64)->!{crate::println!("rejected SYSRET rip={:#x} rsp={:#x}",rip,rsp);crate::arch::halt()}

pub unsafe fn set_kernel_stack(value:u64){unsafe{SYSCALL_KERNEL_RSP=value}}
#[unsafe(no_mangle)]pub static mut EXEC_PENDING:u8=0;
#[unsafe(no_mangle)]pub static mut EXEC_RIP:u64=0;
pub unsafe fn commit_exec_return(rip:u64,rsp:u64){unsafe{EXEC_RIP=rip;SYSCALL_USER_RSP=rsp;EXEC_PENDING=1;}}
/* Set by rt_sigreturn: the saved-RAX slot holds a value to restore. */
#[unsafe(no_mangle)]pub static mut SIGRET_PENDING:u8=0;
/* Offsets of the on-stack user register frame saved by syscall_entry. */
const F_R15:usize=0;const F_R14:usize=1;const F_R13:usize=2;const F_R12:usize=3;const F_RBP:usize=4;const F_RBX:usize=5;
const F_R10:usize=6;const F_R9:usize=7;const F_R8:usize=8;const F_RDX:usize=9;const F_RSI:usize=10;const F_RDI:usize=11;
const F_RAX:usize=12;const F_RCX:usize=13;const F_R11:usize=14;
/* Size and layout of the user-visible sigframe rt_sigreturn restores. */
const SIGFRAME_LEN:usize=152;
const SIGFRAME_MAGIC:u64=0x53494746_524d4e55;
fn canonical_user(v:u64)->bool{v>=crate::usercopy::USER_MIN as u64&&v<crate::usercopy::USER_MAX as u64}
/* Called from syscall_entry before SYSRET. Returns nonzero when the register
   frame was rewritten so SYSRET enters a user signal handler. */
#[unsafe(no_mangle)]
pub extern "sysv64" fn signal_setup_frame(frame:*mut u64)->u64{
 let f=frame;if f.is_null(){return 0}
 let sig=match crate::signal::pending(){Some(s)=>s,None=>return 0};

  let act=match crate::signal::action_get(sig as i32){Ok(a)=>a,Err(_)=>return 0};
  let (handler,flags,restorer,_mask)=match act.handler{
   crate::signal::SIG_IGN=>{crate::signal::pop(sig);return 0},
   crate::signal::SIG_DFL=>{
    if crate::signal::default_terminates(sig as i32){crate::task::exit_signal(sig as i32)}
    crate::signal::pop(sig);return 0},
   h=>(h,act.flags,act.restorer,act.mask),
  };
  unsafe{
   let user_rsp=SYSCALL_USER_RSP;
   if !canonical_user(user_rsp)||!canonical_user(handler)||!canonical_user(restorer){
    crate::task::exit_signal(crate::signal::SIGSEGV);
   }
   /* Layout, low to high: handler push area, restorer return slot, sigframe.
      The handler runs with rsp at the return slot, so its pushes grow below
      the slot and never touch the sigframe sitting above it. */
   let sf=(user_rsp.wrapping_sub(SIGFRAME_LEN as u64+8))&!0xf;
   let ret_slot=sf;
   let sigframe=sf+8;
   let mut buf=[0u8;SIGFRAME_LEN];
   let mut put=|off:usize,v:u64|{buf[off..off+8].copy_from_slice(&v.to_ne_bytes())};
   for i in 0..F_RAX{put(i*8,*f.add(i))}
   put(F_RAX*8,*f.add(F_RAX));
   put(13*8,*f.add(F_RCX));
   put(14*8,(*f.add(F_R11))|0x200);
   put(15*8,user_rsp);
   put(16*8,sig as u64);
   put(17*8,SIGFRAME_MAGIC);
   /* Save the mask so rt_sigreturn can restore it; handlers block their own
      signal by default, and Linux restores the pre-handler mask on return. */
   let pre_mask=crate::task::with_current_process(|p|p.sigmask).unwrap_or(0);
   put(18*8,pre_mask);
   if crate::usercopy::copy_to_user(sigframe as usize,&buf).is_err(){crate::task::exit_signal(crate::signal::SIGSEGV)}
   if crate::usercopy::copy_to_user(ret_slot as usize,&restorer.to_ne_bytes()).is_err(){crate::task::exit_signal(crate::signal::SIGSEGV)}
   crate::signal::set_sigframe(sigframe);
   crate::signal::pop(sig);
   if flags&crate::signal::SA_RESETHAND!=0{let mut reset=act;reset.handler=crate::signal::SIG_DFL;let _=crate::signal::action_set(sig as i32,reset);}
   /* Block the signal while its handler runs unless SA_NODEFER is set. */
   if flags&crate::signal::SA_NODEFER==0{let _=crate::signal::mask_op(crate::signal::BLOCK,crate::signal::bit(sig as i32));}
  /* Redirect SYSRET into the handler with rdi = sig. */
  *f.add(F_RDI)=sig as u64;
  *f.add(F_RCX)=handler;
   /* Enter the handler with rsp at the restorer return slot. */
   SYSCALL_USER_RSP=ret_slot;
   1
  }
}
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

/* rt_sigreturn: restores the interrupted context from the sigframe the
   kernel wrote before entering the handler. Runs in syscall context, so the
   in-flight frame on the kernel stack is rewritten for SIGRET_PENDING. */
pub unsafe fn signal_restore_frame()->i64{
 let sf=crate::signal::get_sigframe();
 let f=unsafe{SYSCALL_FRAME as *mut u64};
 if f.is_null()||!canonical_user(sf){return crate::errno::neg(crate::errno::EFAULT)}
 let mut buf=[0u8;SIGFRAME_LEN];
 if let Err(e)=unsafe{crate::usercopy::copy_from_user(&mut buf,sf as usize)}{return crate::errno::neg(e)}
 let get=|off:usize|u64::from_ne_bytes(buf[off..off+8].try_into().unwrap());
 if get(17*8)!=SIGFRAME_MAGIC{return crate::errno::neg(crate::errno::EFAULT)}
 let new_rip=get(13*8);let new_rflags=get(14*8);let new_rsp=get(15*8);
 if !canonical_user(new_rip)||!canonical_user(new_rsp){return crate::errno::neg(crate::errno::EFAULT)}
 unsafe{
  for i in 0..F_RAX{*f.add(i)=get(i*8)}
  *f.add(F_RAX)=get(F_RAX*8);
  *f.add(F_RCX)=new_rip;
  *f.add(F_R11)=new_rflags;
  SYSCALL_USER_RSP=new_rsp;
  SIGRET_PENDING=1;
  crate::task::with_current_process(|p|p.sigmask=get(18*8));
 }
 0
}

/* Per-task user RSP accessors used by the scheduler when switching between
   threads that share the global syscall frame slot. */
pub unsafe fn read_user_rsp()->u64{unsafe{SYSCALL_USER_RSP}}
pub unsafe fn write_user_rsp(v:u64){unsafe{SYSCALL_USER_RSP=v}}
