pub mod nr; pub mod stack; pub mod types;
use crate::errno; use core::mem::size_of; use types::*;

#[repr(C)] struct UtsName { sysname:[u8;65], nodename:[u8;65], release:[u8;65], version:[u8;65], machine:[u8;65], domainname:[u8;65] }

pub fn dispatch(n:u64,a:[u64;6])->i64 { match n {
    nr::READ=>sys_read(a[0],a[1],a[2]), nr::WRITE=>sys_write(a[0],a[1],a[2]), nr::CLOSE=>sys_close(a[0]),
    nr::FSTAT=>sys_fstat(a[0],a[1]), nr::LSEEK=>sys_lseek(a[0],a[1] as i64,a[2]),
    nr::OPENAT=>sys_openat(a[0] as i64,a[1],a[2]), nr::NEWFSTATAT=>sys_newfstatat(a[0] as i64,a[1],a[2],a[3]),
    nr::MMAP=>proc_result(|p|p.mmap(a[0] as usize,a[1] as usize,a[2],a[3],a[4] as i64,a[5])),
    nr::MPROTECT=>proc_unit(|p|p.mprotect(a[0] as usize,a[1] as usize,a[2])),
    nr::MUNMAP=>proc_unit(|p|p.munmap(a[0] as usize,a[1] as usize)), nr::BRK=>proc_u64(|p|Ok(p.brk(a[0]))),
    nr::SCHED_YIELD=>{crate::task::yield_now();0}, nr::GETPID|nr::GETTID=>crate::task::current_pid() as i64, nr::GETPPID=>crate::task::current_ppid() as i64,
    nr::EXECVE=>sys_execve(a[0],a[1],a[2]),
    nr::GETUID|nr::GETGID|nr::GETEUID|nr::GETEGID=>0, nr::UNAME=>sys_uname(a[0]), nr::ARCH_PRCTL=>sys_arch_prctl(a[0],a[1]),
    nr::SET_TID_ADDRESS=>sys_set_tid_address(a[0]),
    nr::CLONE=>sys_clone(a), nr::WAIT4=>sys_wait4(a[0] as i64,a[1],a[2],a[3]),
    nr::KILL=>sys_kill(a[0] as u32,a[1] as i32),
    nr::RT_SIGACTION=>sys_rt_sigaction(a[0] as i32,a[1],a[2]),
    nr::RT_SIGPROCMASK=>sys_rt_sigprocmask(a[0],a[1],a[2]),
    nr::RT_SIGRETURN=>sys_rt_sigreturn(),
    nr::PIPE=>sys_pipe(a[0],0), nr::PIPE2=>sys_pipe(a[0],a[1]),
    nr::DUP2=>fd_result(crate::fd::dup2(a[0] as usize,a[1] as usize)), nr::DUP3=>sys_dup3(a[0] as usize,a[1] as usize,a[2]),
    nr::FCNTL=>sys_fcntl(a[0] as usize,a[1],a[2]),
    nr::SET_ROBUST_LIST=>if a[1]==24 {0}else{errno::neg(errno::EINVAL)}, nr::PRLIMIT64=>sys_prlimit(a),
    nr::CLOCK_GETTIME=>sys_clock_gettime(a[0],a[1]), nr::NANOSLEEP=>sys_nanosleep(a[0],a[1]),
    nr::GETRANDOM=>sys_getrandom(a[0],a[1],a[2]),
    nr::READV=>sys_readv(a[0],a[1],a[2] as i32),
    nr::WRITEV=>sys_writev(a[0],a[1],a[2] as i32),
    nr::GETDENTS64=>sys_getdents64(a[0],a[1],a[2] as i32),
    nr::FUTEX=>sys_futex(a[0],a[1] as i32,a[2] as i32,a[3],a[4] as u64,a[5] as i32),
    nr::GETCWD=>sys_getcwd(a[0],a[1]),
    nr::IOCTL=>sys_ioctl(a[0] as usize,a[1],a[2]),
    nr::EXIT|nr::EXIT_GROUP=>crate::task::exit(a[0] as i32), _=>errno::neg(errno::ENOSYS),
}}
fn fd_id(fd:u64)->Option<u32>{crate::task::with_current_process(|p|p.fds.get(fd as usize)).flatten()}
fn proc_result(f:impl FnOnce(&mut crate::process::Process)->Result<usize,i64>)->i64{match crate::task::with_current_process(f){Some(Ok(v))=>v as i64,Some(Err(e))=>errno::neg(e),None=>errno::neg(errno::ESRCH)}}
fn proc_unit(f:impl FnOnce(&mut crate::process::Process)->Result<(),i64>)->i64{match crate::task::with_current_process(f){Some(Ok(()))=>0,Some(Err(e))=>errno::neg(e),None=>errno::neg(errno::ESRCH)}}
fn proc_u64(f:impl FnOnce(&mut crate::process::Process)->Result<u64,i64>)->i64{match crate::task::with_current_process(f){Some(Ok(v))=>v as i64,Some(Err(e))=>errno::neg(e),None=>errno::neg(errno::ESRCH)}}
fn fd_result(r:Result<i32,i64>)->i64{match r{Ok(v)=>v as i64,Err(e)=>errno::neg(e)}}
fn sys_read(fd:u64,ptr:u64,len:u64)->i64 {
 if len>isize::MAX as u64{return errno::neg(errno::EFAULT)}
 let id=match fd_id(fd){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 let mut done=0usize;let mut buf=[0u8;256];
 while done<len as usize{let wanted=core::cmp::min(buf.len(),len as usize-done);
  let n=match crate::fd::read(id,&mut buf[..wanted]){Ok(n)=>n,Err(e)=>{return if done==0{errno::neg(e)}else{done as i64}}};
  if n==0{break}
  let dst=match (ptr as usize).checked_add(done){Some(v)=>v,None=>return errno::neg(errno::EFAULT)};
  if let Err(e)=crate::usercopy::copy_to_user(dst,&buf[..n]){return if done==0{errno::neg(e)}else{done as i64}}
  done+=n;
 }
 done as i64
}
fn sys_write(fd:u64,ptr:u64,len:u64)->i64 {
 if len>isize::MAX as u64{return errno::neg(errno::EFAULT)}
 let id=match fd_id(fd){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 let mut done=0usize; let mut buf=[0u8;256]; while done<len as usize {let n=core::cmp::min(buf.len(),len as usize-done);
  let src=match (ptr as usize).checked_add(done){Some(v)=>v,None=>return errno::neg(errno::EFAULT)};
  if let Err(e)=crate::usercopy::copy_from_user(&mut buf[..n],src){return if done==0{errno::neg(e)}else{done as i64}}
  match crate::fd::write(id,&buf[..n]){Ok(m)if m==n=>{}Ok(m)=>return (done+m)as i64,Err(e)=>return if done==0{errno::neg(e)}else{done as i64}}
  done+=n;} done as i64
}
fn sys_close(fd:u64)->i64{unit(crate::fd::close(fd as usize))}
fn sys_fstat(fd:u64,ptr:u64)->i64 {let id=match fd_id(fd){Some(id)=>id,None=>return errno::neg(errno::EBADF)};let mut st=Stat::default();
 match crate::fd::kind(id){
  Some(crate::fd::OpenKind::Inode(n))=>{st.st_ino=n.ino;st.st_mode=n.mode;st.st_nlink=1;st.st_size=n.size as i64;st.st_blocks=((n.size+511)/512)as i64;},
  Some(crate::fd::OpenKind::Console)=>{st.st_mode=S_IFCHR|0o666;st.st_nlink=1;},
  Some(crate::fd::OpenKind::PipeRead(_))|Some(crate::fd::OpenKind::PipeWrite(_))=>{st.st_mode=S_IFIFO|0o600;st.st_nlink=1;},
  None=>return errno::neg(errno::EBADF),
 }
 st.st_blksize=4096;copy_obj(ptr,&st)}
fn sys_lseek(fd:u64,off:i64,whence:u64)->i64{let id=match fd_id(fd){Some(id)=>id,None=>return errno::neg(errno::EBADF)};match crate::fd::seek(id,off,whence){Ok(v)=>v as i64,Err(e)=>errno::neg(e)}}
fn sys_set_tid_address(ptr:u64)->i64{let _=crate::task::with_current_process(|p|p.set_clear_child_tid(ptr));crate::task::current_pid() as i64}
fn sys_clone(a:[u64;6])->i64{
 const SIGCHLD:u64=17;
 if a[1]!=0||a[2]!=0||a[3]!=0||a[4]!=0{return errno::neg(errno::EINVAL)}
 if a[0]&!SIGCHLD!=0{return errno::neg(errno::EINVAL)}
 let frame=match crate::arch::x86_64::syscall::capture_fork_frame(){Some(f)=>f,None=>return errno::neg(errno::EINVAL)};
 match crate::task::fork_process(frame){Ok(pid)=>pid as i64,Err(e)=>errno::neg(e)}
}
fn sys_wait4(pid:i64,status_ptr:u64,options:u64,rusage:u64)->i64{
 const WNOHANG:u64=1;const WUNTRACED:u64=2;
 if options&!(WNOHANG|WUNTRACED)!=0{return errno::neg(errno::EINVAL)}
 loop{match crate::task::reap_child(pid,options&WNOHANG!=0){
  Ok(crate::task::WaitOutcome::Reaped(cpid,status))=>{
   if status_ptr!=0{let s=status as i32;let r=copy_obj(status_ptr,&s);if r<0{return r}}
   if rusage!=0{let zeros=[0u8;144];if let Err(e)=crate::usercopy::copy_to_user(rusage as usize,&zeros){return errno::neg(e)}}
   return cpid as i64;
  }
  Ok(crate::task::WaitOutcome::NoHang)=>return 0,
  Ok(crate::task::WaitOutcome::Blocked)=>crate::task::block_current(),
  Err(e)=>return errno::neg(e),
 }}
}
fn sys_kill(pid:u32,sig:i32)->i64{
 /* sig 0 only probes for the existence of the target. */
 if sig==0{let exists=crate::task::sched_pids().iter().any(|&p|p==pid);return if exists{0}else{errno::neg(errno::ESRCH)}}
 match crate::signal::send(pid,sig){Ok(())=>0,Err(e)=>errno::neg(e)}
}
fn sys_rt_sigaction(sig:i32,act:u64,old:u64)->i64{
 /* Linux reports the previous disposition in oldact before installing the new one. */
 let prev=match crate::signal::action_get(sig){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 if old!=0{let bytes=unsafe{core::slice::from_raw_parts((&prev as*const crate::signal::Action).cast::<u8>(),core::mem::size_of::<crate::signal::Action>())};if let Err(e)=crate::usercopy::copy_to_user(old as usize,bytes){return errno::neg(e)}}
 if act!=0{
  let mut a=crate::signal::Action{handler:0,flags:0,restorer:0,mask:0};
  let b=unsafe{core::slice::from_raw_parts_mut((&mut a as*mut crate::signal::Action).cast::<u8>(),core::mem::size_of::<crate::signal::Action>())};
  if let Err(e)=crate::usercopy::copy_from_user(b,act as usize){return errno::neg(e)}
  if let Err(e)=crate::signal::action_set(sig,a){return errno::neg(e)}
 }
 0
}
fn sys_rt_sigprocmask(how:u64,set:u64,old:u64)->i64{
 let new_mask=if set==0{0}else{match user_u64(set){Ok(v)=>v,Err(e)=>return errno::neg(e)}};
 let prev=match crate::signal::mask_op(how,new_mask){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 if old!=0{let bytes=unsafe{core::slice::from_raw_parts((&prev as*const u64).cast::<u8>(),8)};if let Err(e)=crate::usercopy::copy_to_user(old as usize,bytes){return errno::neg(e)}}0
}
fn sys_rt_sigreturn()->i64{unsafe{crate::arch::x86_64::syscall::signal_restore_frame()}}
fn sys_pipe(ptr:u64,flags:u64)->i64{let(r,w)=match crate::fd::pipe2(flags){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let pair=[r as u32,w as u32];let bytes=unsafe{core::slice::from_raw_parts(pair.as_ptr()as*const u8,8)};
 match crate::usercopy::copy_to_user(ptr as usize,bytes){Ok(())=>0,Err(e)=>{let _=crate::fd::close(r as usize);let _=crate::fd::close(w as usize);errno::neg(e)}}}
fn sys_dup3(oldfd:usize,newfd:usize,flags:u64)->i64{if flags&!0x80000!=0||oldfd==newfd{return errno::neg(errno::EINVAL)}fd_result(crate::fd::dup2(oldfd,newfd))}
fn sys_fcntl(fd:usize,cmd:u64,arg:u64)->i64{match cmd{0=>fd_result(crate::fd::dup_from(fd,arg as usize)),1|2|3|4=>0,_=>errno::neg(errno::EINVAL)}}
fn copy_path<'a>(ptr:u64,out:&'a mut[u8])->Result<&'a str,i64>{for i in 0..out.len(){crate::usercopy::copy_from_user(&mut out[i..i+1],ptr as usize+i)?;if out[i]==0{return core::str::from_utf8(&out[..i]).map_err(|_|errno::EINVAL)}}Err(errno::ENAMETOOLONG)}
fn sys_openat(dirfd:i64,path:u64,flags:u64)->i64{const AT_FDCWD:i64=-100;if dirfd!=AT_FDCWD||flags&!0o77777!=0{return errno::neg(errno::EINVAL)}let mut b=[0u8;128];let p=match copy_path(path,&mut b){Ok(v)=>v,Err(e)=>return errno::neg(e)};match crate::fd::open(p){Ok(fd)=>fd as i64,Err(e)=>errno::neg(e)}}
fn sys_newfstatat(dirfd:i64,path:u64,ptr:u64,flags:u64)->i64{const AT_FDCWD:i64=-100;if dirfd!=AT_FDCWD||flags!=0{return errno::neg(errno::EINVAL)}let mut b=[0u8;128];let p=match copy_path(path,&mut b){Ok(v)=>v,Err(e)=>return errno::neg(e)};let n=match crate::vfs::lookup(p){Ok(n)=>n,Err(e)=>return errno::neg(e)};let mut st=Stat::default();st.st_ino=n.ino;st.st_mode=n.mode;st.st_nlink=1;st.st_size=n.size as i64;st.st_blksize=4096;st.st_blocks=((n.size+511)/512)as i64;copy_obj(ptr,&st)}
fn field(s:&str)->[u8;65]{let mut x=[0;65];let b=s.as_bytes();let n=core::cmp::min(64,b.len());x[..n].copy_from_slice(&b[..n]);x}
fn sys_uname(ptr:u64)->i64{let u=UtsName{sysname:field("Linux"),nodename:field("slop"),release:field("6.6.0-mnu"),version:field("Slop/MNU 0.27.0"),machine:field("x86_64"),domainname:field("localdomain")};copy_obj(ptr,&u)}
fn sys_arch_prctl(code:u64,addr:u64)->i64 {const SET_FS:u64=0x1002;const GET_FS:u64=0x1003;match code{SET_FS=>if crate::process::set_fs_base(addr){0}else{errno::neg(errno::EPERM)},GET_FS=>copy_obj(addr,&crate::task::fs_base()),_=>errno::neg(errno::EINVAL)}}
fn sys_prlimit(a:[u64;6])->i64{if a[0]!=0&&a[0]!=crate::task::current_pid() as u64{return errno::neg(errno::ESRCH)} if a[2]!=0{return errno::neg(errno::EPERM)} if a[3]!=0{copy_obj(a[3],&Rlimit{cur:u64::MAX,max:u64::MAX})}else{0}}
fn copy_obj<T>(dst:u64,obj:&T)->i64{let bytes=unsafe{core::slice::from_raw_parts((obj as *const T).cast::<u8>(),size_of::<T>())};match crate::usercopy::copy_to_user(dst as usize,bytes){Ok(())=>0,Err(e)=>errno::neg(e)}}

fn result(r:Result<usize,i64>)->i64{match r{Ok(v)=>v as i64,Err(e)=>errno::neg(e)}}
fn unit(r:Result<(),i64>)->i64{match r{Ok(())=>0,Err(e)=>errno::neg(e)}}
const EXEC_MAX_VEC:usize=8;const EXEC_MAX_STR:usize=128;
fn user_u64(ptr:u64)->Result<u64,i64>{let mut b=[0u8;8];crate::usercopy::copy_from_user(&mut b,ptr as usize)?;Ok(u64::from_ne_bytes(b))}
fn copy_exec_vec(base:u64,buf:&mut[[u8;EXEC_MAX_STR];EXEC_MAX_VEC],lens:&mut[usize;EXEC_MAX_VEC])->Result<usize,i64>{if base==0{return Ok(0)}for i in 0..EXEC_MAX_VEC{let p=user_u64(base.checked_add((i*8)as u64).ok_or(errno::EFAULT)?)?;if p==0{return Ok(i)}let mut ended=false;for j in 0..EXEC_MAX_STR{crate::usercopy::copy_from_user(&mut buf[i][j..j+1],p as usize+j)?;if buf[i][j]==0{lens[i]=j;ended=true;break}}if !ended{return Err(errno::E2BIG)}}Err(errno::E2BIG)}
fn sys_execve(path_ptr:u64,argv_ptr:u64,envp_ptr:u64)->i64{
 let mut path_buf=[0u8;EXEC_MAX_STR];let path=match copy_path(path_ptr,&mut path_buf){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let image=match crate::vfs::image(path){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let mut ab=[[0u8;EXEC_MAX_STR];EXEC_MAX_VEC];let mut al=[0usize;EXEC_MAX_VEC];let ac=match copy_exec_vec(argv_ptr,&mut ab,&mut al){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let mut eb=[[0u8;EXEC_MAX_STR];EXEC_MAX_VEC];let mut el=[0usize;EXEC_MAX_VEC];let ec=match copy_exec_vec(envp_ptr,&mut eb,&mut el){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let mut av=["";EXEC_MAX_VEC];for i in 0..ac{av[i]=match core::str::from_utf8(&ab[i][..al[i]]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)}}
 let mut ev=["";EXEC_MAX_VEC];for i in 0..ec{ev[i]=match core::str::from_utf8(&eb[i][..el[i]]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)}}
 let fallback=[path];let argv=if ac==0{&fallback[..]}else{&av[..ac]};
 let bootstrap=match crate::task::with_current_process(|p|p.bootstrap_cr3){Some(v)=>v,None=>return errno::neg(errno::ENOEXEC)};
 let loaded=match unsafe{crate::exec_runtime::load_static_args(image,argv,&ev[..ec],path,bootstrap)}{Ok(v)=>v,Err(_)=>return errno::neg(errno::ENOEXEC)};
 match crate::task::with_current_process(|p|p.replace_image(loaded.cr3,loaded.image_slot,loaded.stack_slot)){Some(Ok(()))=>{},Some(Err(e))=>return errno::neg(e),None=>return errno::neg(errno::ENOEXEC)}
 let _=crate::task::with_current_process(|p|p.reset_exec_state());
 crate::task::set_current_fs_base(0);
 unsafe{crate::arch::x86_64::syscall::commit_exec_return(loaded.entry,loaded.rsp)}
 crate::println!("execve: committed {} rip={:#x} rsp={:#x}",path,loaded.entry,loaded.rsp);0
}

fn sys_clock_gettime(clock:u64,ptr:u64)->i64{
 match clock{
  0|1|2|3|4|5|6|7|8|9|10|11=>{
   let ns=crate::time::monotonic_ns();
   let ts=Timespec{tv_sec:(ns/1_000_000_000)as i64,tv_nsec:(ns%1_000_000_000)as i64};
   copy_obj(ptr,&ts)
  }
  _=>errno::neg(errno::EINVAL)
 }
}
fn sys_nanosleep(req:u64,rem:u64)->i64{let mut ts=Timespec::default();let b=unsafe{core::slice::from_raw_parts_mut((&mut ts as*mut Timespec).cast::<u8>(),core::mem::size_of::<Timespec>())};if let Err(e)=crate::usercopy::copy_from_user(b,req as usize){return errno::neg(e)}if ts.tv_sec<0||ts.tv_nsec<0||ts.tv_nsec>=1_000_000_000{return errno::neg(errno::EINVAL)}let ns=(ts.tv_sec as u64).saturating_mul(1_000_000_000).saturating_add(ts.tv_nsec as u64);crate::task::sleep_until(crate::time::ticks_after_ns(ns));if rem!=0{let zero=Timespec::default();let _=copy_obj(rem,&zero);}0}
fn sys_getrandom(ptr:u64,len:u64,flags:u64)->i64{if flags&!3!=0{return errno::neg(errno::EINVAL)}if len>isize::MAX as u64{return errno::neg(errno::EFAULT)}let mut done=0usize;let mut buf=[0u8;64];while done<len as usize{let n=core::cmp::min(buf.len(),len as usize-done);crate::time::fill_random(&mut buf[..n]);if let Err(e)=crate::usercopy::copy_to_user(ptr as usize+done,&buf[..n]){return if done==0{errno::neg(e)}else{done as i64}}done+=n;}done as i64}
fn sys_getcwd(ptr:u64,len:u64)->i64{
 if len==0{return errno::neg(errno::EINVAL)}
 let cwd=b"/";
 if len<cwd.len()as u64{return errno::neg(errno::ERANGE)}
 if let Err(e)=crate::usercopy::copy_to_user(ptr as usize,cwd){return errno::neg(e)}
 0
}
fn sys_ioctl(fd:usize,cmd:u64,arg:u64)->i64{
 let _id=match fd_id(fd as u64){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 let _=cmd;
 let _=arg;
 errno::neg(errno::ENOTTY)
}
fn sys_readv(fd:u64,ptr:u64,count:i32)->i64{
 if count<=0{return errno::neg(errno::EINVAL)}
 let id=match fd_id(fd){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 let mut iov=[Iovec{iov_base:0,iov_len:0};8];
 for i in 0..count.min(8){
  let mut b=[0u8;16];
  if let Err(e)=crate::usercopy::copy_from_user(&mut b,(ptr as usize)+(i as usize*16)){return errno::neg(e)}
  iov[i as usize]=Iovec{iov_base:u64::from_ne_bytes(b[0..8].try_into().unwrap()),iov_len:u64::from_ne_bytes(b[8..16].try_into().unwrap())};
 }
 let mut total=0i64;
 for i in 0..count.min(8){
  let mut buf=[0u8;256];
  let n=core::cmp::min(buf.len(),iov[i as usize].iov_len as usize);
  let got=match crate::fd::read(id,&mut buf[..n]){Ok(g)=>g,Err(e)=>return errno::neg(e)};
  if let Err(e)=crate::usercopy::copy_to_user(iov[i as usize].iov_base as usize,&buf[..got]){return errno::neg(e)}
  total+=got as i64;
 }
 total
}
fn sys_writev(fd:u64,ptr:u64,count:i32)->i64{
 if count<=0{return errno::neg(errno::EINVAL)}
 let id=match fd_id(fd){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 let mut iov=[Iovec{iov_base:0,iov_len:0};8];
 for i in 0..count.min(8){
  let mut b=[0u8;16];
  if let Err(e)=crate::usercopy::copy_from_user(&mut b,(ptr as usize)+(i as usize*16)){return errno::neg(e)}
  iov[i as usize]=Iovec{iov_base:u64::from_ne_bytes(b[0..8].try_into().unwrap()),iov_len:u64::from_ne_bytes(b[8..16].try_into().unwrap())};
 }
 let mut total=0i64;
 for i in 0..count.min(8){
  let mut buf=[0u8;256];
  let n=core::cmp::min(buf.len(),iov[i as usize].iov_len as usize);
  if let Err(e)=crate::usercopy::copy_from_user(&mut buf[..n],iov[i as usize].iov_base as usize){return errno::neg(e)}
  match crate::fd::write(id,&buf[..n]){Ok(m)=>{total+=m as i64},Err(e)=>return errno::neg(e)}
 }
 total
}
fn sys_getdents64(fd:u64,ptr:u64,count:i32)->i64{
 if count<=0{return errno::neg(errno::EINVAL)}
 let id=match fd_id(fd){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 let inode=match crate::fd::kind(id){Some(crate::fd::OpenKind::Inode(n))=>n,_=>return errno::neg(errno::ENOTDIR)};
 let path=core::str::from_utf8(&inode.name[..inode.name_len]).unwrap_or("");
 let entries=crate::vfs::readdir(path).unwrap_or(0);
 if entries==0{return 0}
 let mut buf=[0u8;1024];
 let mut off=0i64;
 for i in 0..entries{
  let reclen=16;
  if off+reclen as i64>count as i64{break}
  buf[off as usize..off as usize+8].copy_from_slice(&(i+2).to_ne_bytes());
  buf[off as usize+8..off as usize+9].copy_from_slice(&0u8.to_ne_bytes());
  buf[off as usize+9..off as usize+16].copy_from_slice(b"test");
  off+=reclen as i64;
 }
 if let Err(e)=crate::usercopy::copy_to_user(ptr as usize,&buf[..off as usize]){return errno::neg(e)}
 off
}
fn sys_futex(uaddr:u64,op:i32,val:i32,timeout:u64,uaddr2:u64,val3:i32)->i64{
 /* Minimal futex stub: FUTEX_WAIT returns 0, FUTEX_WAKE returns 0 */
 match op&0x7f{
  1=>0,
  _=>errno::neg(errno::ENOSYS)
 }
}
