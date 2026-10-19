//! Linux signal ABI: kill, rt_sigaction, rt_sigprocmask and rt_sigreturn.
//! Per-process pending masks, blocked masks and handler slots live in Process.
//! Frame rewriting for user-mode delivery lives in arch::x86_64::syscall.
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
}
pub fn action_get(sig:i32)->Result<Action,i64>{
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
