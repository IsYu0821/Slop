//! Linux signal ABI: kill, rt_sigaction, rt_sigprocmask and rt_sigreturn.
//! Per-process pending masks, blocked masks and handler slots live in Process.
use crate::{sync::SpinLock,task};
pub const SIGHUP:i32=1;pub const SIGINT:i32=2;pub const SIGQUIT:i32=3;pub const SIGILL:i32=4;pub const SIGTRAP:i32=5;pub const SIGABRT:i32=6;pub const SIGBUS:i32=7;pub const SIGFPE:i32=8;pub const SIGKILL:i32=9;pub const SIGUSR1:i32=10;pub const SIGSEGV:i32=11;pub const SIGUSR2:i32=12;pub const SIGPIPE:i32=13;pub const SIGALRM:i32=14;pub const SIGTERM:i32=15;pub const SIGCHLD:i32=17;pub const SIGCONT:i32=18;pub const SIGSTOP:i32=19;pub const SIGTSTP:i32=20;
pub const SIG_DFL:u64=0;pub const SIG_IGN:u64=1;
pub const SA_NOCLDSTOP:u64=1;pub const SA_NOCLDWAIT:u64=2;pub const SA_SIGINFO:u64=4;pub const SA_ONSTACK:u64=0x08000000;pub const SA_RESTORER:u64=0x04000000;pub const SA_RESTART:u64=0x10000000;pub const SA_NODEFER:u64=0x40000000;pub const SA_RESETHAND:u64=0x80000000;
pub const BLOCK:u64=0;pub const UNBLOCK:u64=1;pub const SETMASK:u64=2;
const MAX_SIG:usize=64;
#[derive(Clone,Copy)]pub struct Action{pub handler:u64,pub mask:u64,pub flags:u64}
const DFL:Action=Action{handler:SIG_DFL,mask:0,flags:0};
static PENDING:SpinLock<[u64;8]>=SpinLock::new([0;8]);
static BLOCKED:SpinLock<[u64;8]>=SpinLock::new([0;8]);
static ACTIONS:SpinLock<[Option<Action>;MAX_SIG]>=SpinLock::new([None;MAX_SIG]);
pub fn slot(pid:u32)->usize{(pid as usize)%8}
fn valid(sig:i32)->bool{sig>=1&&sig<MAX_SIG as i32}
fn bit(sig:i32)->u64{1u64<<(sig as u64-1)}
fn find(pid:u32)->Option<usize>{let s=task::sched_pids();(0..8).find(|&i|s[i]==pid)}
fn deliver(pid:u32,sig:i32){
 if !valid(sig){return}
 if sig==SIGKILL||sig==SIGSTOP{let mut b=BLOCKED.lock();b[slot(pid)]&=!bit(sig);}
 PENDING.lock()[slot(pid)]|=bit(sig);
 task::wake_pid(pid);
}
pub fn send(pid:u32,sig:i32)->Result<(),i64>{
 if !valid(sig){return Err(crate::errno::EINVAL)}
 if pid==0||pid==task::current_pid()||find(pid).is_some(){deliver(pid,sig);Ok(())}else{Err(crate::errno::ESRCH)}
}
pub fn action_get(sig:i32)->Result<Action,i64>{if !valid(sig){return Err(crate::errno::EINVAL)}Ok(ACTIONS.lock()[sig as usize].unwrap_or(DFL))}
pub fn action_set(sig:i32,a:Action)->Result<(),i64>{if !valid(sig){return Err(crate::errno::EINVAL)}ACTIONS.lock()[sig as usize]=Some(a);Ok(())}
pub fn mask_op(how:u64,set:u64)->Result<u64,i64>{
 let old={let mut b=BLOCKED.lock();let o=b[slot(task::current_pid())];match how{BLOCK=>b[slot(task::current_pid())]=o|set,UNBLOCK=>b[slot(task::current_pid())]=o&!set,SETMASK=>b[slot(task::current_pid())]=set,_=>return Err(crate::errno::EINVAL)};o};
 Ok(old)
}
pub fn pending()->Option<i64>{
 /* Signal delivery is not yet wired into the syscall exit path; report none. */
 None
}
pub fn pop(sig:i64){PENDING.lock()[slot(task::current_pid())]&=!bit(sig as i32)}
pub fn is_blocked(sig:i64)->bool{BLOCKED.lock()[slot(task::current_pid())]&bit(sig as i32)!=0}
pub fn clear_all(pid:u32){PENDING.lock()[slot(pid)]=0;BLOCKED.lock()[slot(pid)]=0;}
pub fn on_exit(pid:u32){clear_all(pid);if let Some(i)=find(pid){PENDING.lock()[i]&=!bit(SIGCHLD);BLOCKED.lock()[i]&=!bit(SIGCHLD);}}
