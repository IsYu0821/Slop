pub mod nr; pub mod stack; pub mod types;
use crate::errno; use core::mem::size_of; use types::*;

#[repr(C)] struct UtsName { sysname:[u8;65], nodename:[u8;65], release:[u8;65], version:[u8;65], machine:[u8;65], domainname:[u8;65] }

static mut HOSTNAME:[u8;65]=[0;65];
fn hostname_init(){unsafe{HOSTNAME[..4].copy_from_slice(b"slop");}}
fn hostname_bytes()->[u8;65]{let h=unsafe{HOSTNAME}; if h[0]==0 { let mut d=[0u8;65]; d[..4].copy_from_slice(b"slop"); d } else { h } }
pub fn dispatch(n:u64,a:[u64;6])->i64 {
 if crate::task::with_current_process(|p|p.seccomp_mode).unwrap_or(0)==1 {
   match n { nr::READ|nr::WRITE|nr::EXIT|nr::RT_SIGRETURN=>{}, _=>crate::task::exit_signal(9), }
 }
 if crate::task::with_current_process(|p|p.seccomp_mode).unwrap_or(0)==2 {
  let (filter,len)=crate::task::with_current_process(|p|(p.seccomp_filter,p.seccomp_filter_len)).unwrap_or((0,0));
  if filter!=0 && len>0 {
   let data=SeccompData{nr:n as i32,arch:0xc000003e,ip:0,args:a};
   let action=seccomp_eval(crate::arch::x86_64::phys::phys_to_virt(filter as u64)as *const SockFilter,len as usize,&data);
    match action&0xffff0000 {
     0x80000000|0x00000000=>crate::task::exit_signal(9),
     0x00050000=>{let e=((action&0xffff)as i64).min(4095);return errno::neg(if e==0{errno::EPERM}else{e})},
     0x00030000=>crate::task::exit_signal(31),
     0x7fff0000|0x7ffc0000=>{},
     0x7ff00000|0x7fc00000=>return errno::neg(errno::ENOSYS),
     _ => crate::task::exit_signal(9),
   }
  }
 }
 match n {
    nr::READ=>sys_read(a[0],a[1],a[2]), nr::WRITE=>sys_write(a[0],a[1],a[2]), nr::CLOSE=>sys_close(a[0]),
    nr::OPEN=>sys_open(a[0],a[1],a[2]), nr::CREAT=>sys_creat(a[0],a[1]),
    nr::FSYNC=>sys_fsync(a[0] as usize), nr::FDATASYNC=>sys_fsync(a[0] as usize),
    nr::EVENTFD2=>sys_eventfd2(a[0],a[1]), nr::TIMERFD_CREATE=>sys_timerfd_create(a[0] as i32,a[1]),
    nr::TIME=>sys_time(a[0]), nr::TGKILL=>sys_tgkill(a[0] as u32,a[1] as u32,a[2] as i32), nr::TKILL=>sys_tgkill(crate::task::current_tgid(),a[0] as u32,a[1] as i32),
    nr::RT_SIGPENDING=>sys_rt_sigpending(a[0],a[1]), nr::RT_SIGTIMEDWAIT=>sys_rt_sigtimedwait(a[0],a[1],a[2],a[3]),
    nr::GETPRIORITY=>0, nr::SETPRIORITY=>0, nr::PAUSE=>sys_pause(), nr::ALARM=>sys_alarm(a[0]),
    nr::GETITIMER=>sys_getitimer(a[0],a[1]), nr::SETITIMER=>sys_setitimer(a[0],a[1],a[2]),
    nr::STAT=>sys_stat(a[0],a[1]), nr::LSTAT=>sys_lstat(a[0],a[1]), nr::UNLINK=>sys_unlink(a[0]),
    nr::SYMLINK=>sys_symlink(a[0],a[1]), nr::LINK=>sys_link(a[0],a[1]),
    nr::SYMLINKAT=>sys_symlink(a[0],a[2]), nr::LINKAT=>sys_link(a[0],a[2]),
    nr::FSTAT=>sys_fstat(a[0],a[1]), nr::LSEEK=>sys_lseek(a[0],a[1] as i64,a[2]),
    nr::OPENAT=>sys_openat(a[0] as i64,a[1],a[2]), nr::NEWFSTATAT=>sys_newfstatat(a[0] as i64,a[1],a[2],a[3]),
    nr::MMAP=>proc_result(|p|p.mmap(a[0] as usize,a[1] as usize,a[2],a[3],a[4] as i64,a[5])),
    nr::MPROTECT=>proc_unit(|p|p.mprotect(a[0] as usize,a[1] as usize,a[2])),
    nr::MUNMAP=>proc_unit(|p|p.munmap(a[0] as usize,a[1] as usize)), nr::BRK=>proc_u64(|p|Ok(p.brk(a[0]))),
    nr::SCHED_YIELD=>{crate::task::yield_now();0}, nr::GETPID=>crate::task::current_tgid() as i64, nr::GETTID=>crate::task::current_pid() as i64, nr::GETPPID=>crate::task::current_ppid() as i64,
    nr::EXECVE=>sys_execve(a[0],a[1],a[2]),
    nr::GETUID=>sys_getuid(), nr::GETGID=>sys_getgid(), nr::GETEUID=>sys_geteuid(), nr::GETEGID=>sys_getegid(),
    nr::SETUID=>sys_setuid(a[0] as u32), nr::SETGID=>sys_setgid(a[0] as u32), nr::SETREUID=>sys_setreuid(a[0] as u32,a[1] as u32),
    nr::SETREGID=>sys_setregid(a[0] as u32,a[1] as u32), nr::GETGROUPS=>sys_getgroups(a[0] as i32,a[1]),
    nr::SETGROUPS=>sys_setgroups(a[0] as usize,a[1]), nr::SETRESUID=>sys_setresuid(a[0] as u32,a[1] as u32,a[2] as u32),
    nr::GETRESUID=>sys_getresuid(a[0],a[1],a[2]), nr::SETRESGID=>sys_setresgid(a[0] as u32,a[1] as u32,a[2] as u32),
    nr::GETRESGID=>sys_getresgid(a[0],a[1],a[2]), nr::CAPGET=>sys_capget(a[0],a[1]), nr::CAPSET=>sys_capset(a[0],a[1]),
    nr::UMASK=>sys_umask(a[0] as u32), nr::UNSHARE=>sys_unshare(a[0]), nr::SETNS=>0,
    nr::SECCOMP=>sys_seccomp(a[0] as u32,a[1] as u32,a[2]),
    nr::INOTIFY_INIT=>sys_inotify_init(), nr::INOTIFY_INIT1=>sys_inotify_init1(a[0]), nr::INOTIFY_ADD_WATCH=>sys_inotify_add_watch(a[0] as usize,a[1],a[2]),
    nr::MEMFD_CREATE=>sys_memfd_create(a[0],a[1]), nr::TIMERFD_SETTIME=>sys_timerfd_settime(a[0] as usize,a[1],a[2],a[3]), nr::TIMERFD_GETTIME=>sys_timerfd_gettime(a[0] as usize,a[1]),
    nr::INOTIFY_RM_WATCH=>sys_inotify_rm_watch(a[0] as usize,a[1] as i32),
    nr::SIGNALFD4=>sys_signalfd4(a[0] as i32,a[1],a[2],a[3]),
    nr::MOUNT=>sys_mount(a[0],a[1],a[2],a[3],a[4]), nr::UMOUNT2=>sys_umount2(a[0],a[1]),
    nr::PIVOT_ROOT=>sys_pivot_root(a[0],a[1]), nr::CHROOT=>sys_chroot(a[0]),
    nr::SETHOSTNAME=>sys_sethostname(a[0],a[1]), nr::SETDOMAINNAME=>sys_setdomainname(a[0],a[1]),
    nr::REBOOT=>errno::neg(errno::EPERM), nr::IOPERM=>errno::neg(errno::EPERM), nr::IOPL=>errno::neg(errno::EPERM),
    nr::SYSLOG=>0, nr::PERSONALITY=>0,
    nr::UNAME=>sys_uname(a[0]), nr::ARCH_PRCTL=>sys_arch_prctl(a[0],a[1]),
    nr::SET_TID_ADDRESS=>sys_set_tid_address(a[0]),
    nr::CLONE=>sys_clone(a), nr::WAIT4=>sys_wait4(a[0] as i64,a[1],a[2],a[3]),
    nr::KILL=>sys_kill(a[0] as u32,a[1] as i32),
    nr::RT_SIGACTION=>sys_rt_sigaction(a[0] as i32,a[1],a[2]),
    nr::SIGQUEUE=>sys_sigqueue(a[0] as u32,a[1] as i32,a[2]),
    nr::RT_SIGQUEUEINFO=>sys_rt_sigqueueinfo(a[0] as u32,a[1] as i32,a[2],a[3]),
    nr::RT_SIGPROCMASK=>sys_rt_sigprocmask(a[0],a[1],a[2]),
    nr::RT_SIGRETURN=>sys_rt_sigreturn(),
    nr::PIPE=>sys_pipe(a[0],0), nr::PIPE2=>sys_pipe(a[0],a[1]),
    nr::DUP2=>fd_result(crate::fd::dup2(a[0] as usize,a[1] as usize)), nr::DUP3=>sys_dup3(a[0] as usize,a[1] as usize,a[2]),
    nr::FCNTL=>sys_fcntl(a[0] as usize,a[1],a[2]),
    nr::FLOCK=>sys_flock(a[0] as usize,a[1]),
    nr::SET_ROBUST_LIST=>sys_set_robust_list(a[0],a[1]), nr::GET_ROBUST_LIST=>sys_get_robust_list(a[0] as u32,a[1],a[2]), nr::PRLIMIT64=>sys_prlimit(a),
    nr::CLOCK_GETTIME=>sys_clock_gettime(a[0],a[1]), nr::NANOSLEEP=>sys_nanosleep(a[0],a[1]),
    nr::GETRANDOM=>sys_getrandom(a[0],a[1],a[2]),
    nr::READV=>sys_readv(a[0],a[1],a[2] as i32),
    nr::WRITEV=>sys_writev(a[0],a[1],a[2] as i32),
    nr::GETDENTS64=>sys_getdents64(a[0],a[1],a[2] as i32),
    nr::FUTEX=>sys_futex(a[0],a[1] as i32,a[2] as i32,a[3],a[4] as u64,a[5] as i32),
    nr::UNLINKAT=>sys_unlinkat(a[0] as i64,a[1],a[2]),
    nr::STATX=>sys_statx(a[0] as i64,a[1],a[2],a[3],a[4]),
    nr::MREMAP=>proc_result(|p|p.mremap(a[0] as usize,a[1] as usize,a[2] as usize,a[3] as u64,a[4])),
    nr::MADVISE=>proc_result(|p|p.madvise(a[0] as usize,a[1] as usize,a[2])),
    nr::MEMBARRIER=>0,
    /* rseq is not implemented: Linux reports ENOSYS so libc falls back to a
       registration-free thread-local storage path. */
    nr::RSEQ=>errno::neg(errno::ENOSYS),
    nr::PKEY_ALLOC=>0,
    nr::WAITID=>sys_waitid(a[0] as i32,a[1],a[2],a[3] as i32,a[4]),
    nr::READLINKAT=>sys_readlinkat(a[0] as i64,a[1],a[2],a[3]),
    nr::FACCESSAT=>sys_faccessat(a[0] as i64,a[1],a[2],a[3]),
    nr::FACCESSAT2=>sys_faccessat(a[0] as i64,a[1],a[2],a[3]),
    nr::ACCESS=>sys_faccessat(-100,a[0],a[1],0),
    nr::EXECVEAT=>sys_execveat(a[0] as i64,a[1],a[2],a[3],a[4]),
    nr::SCHED_GETAFFINITY=>sys_sched_getaffinity(a[0] as u32,a[1] as usize,a[2]),
    nr::POLL=>sys_poll(a[0],a[1] as i32,a[2] as i32),
    nr::PPOLL=>sys_ppoll(a[0],a[1] as i32,a[2],a[3]),
    nr::RT_SIGSUSPEND=>sys_rt_sigsuspend(a[0]),
    nr::SIGALTSTACK=>sys_sigaltstack(a[0],a[1]),
    nr::GETDENTS=>sys_getdents64(a[0],a[1],a[2] as i32),
    nr::PREAD64=>sys_pread64(a[0],a[1],a[2] as i32,a[3] as i64),
    nr::PWRITE64=>sys_pwrite64(a[0],a[1],a[2] as i32,a[3] as i64),
    nr::EPOLL_CREATE1=>sys_epoll_create1(a[0]),
    nr::EPOLL_CTL=>sys_epoll_ctl(a[0] as usize,a[1] as i32,a[2] as usize,a[3]),
    nr::EPOLL_WAIT=>sys_epoll_wait(a[0] as usize,a[1],a[2] as i32,a[3] as i32),
    nr::EPOLL_PWAIT=>sys_epoll_wait(a[0] as usize,a[1],a[2] as i32,a[3] as i32),
    nr::TIMER_CREATE=>sys_timer_create(a[0] as i32,a[1],a[2]), nr::TIMER_SETTIME=>sys_timer_settime(a[0] as u32,a[1],a[2],a[3]), nr::TIMER_GETTIME=>sys_timer_gettime(a[0] as u32,a[1]), nr::TIMER_GETOVERRUN=>sys_timer_getoverrun(a[0] as u32), nr::TIMER_DELETE=>sys_timer_delete(a[0] as u32),
    nr::CLONE3=>sys_clone3(a[0]),
    nr::FORK=>sys_fork(),
    /* vfork shares the parent's stack in Linux; a copied child is a safe
       equivalent for programs that exec immediately. */
    nr::VFORK=>sys_fork(),
    nr::SOCKET=>sys_socket(a[0] as i32,a[1] as i32,a[2] as i32),
    nr::BIND=>sys_bind(a[0] as usize,a[1],a[2]),
    nr::SETSOCKOPT=>sys_setsockopt(a[0] as usize,a[1] as i32,a[2] as i32,a[3],a[4]),
    nr::GETSOCKOPT=>sys_getsockopt(a[0] as usize,a[1] as i32,a[2] as i32,a[3],a[4]),
    nr::SENDMSG=>sys_sendmsg(a[0] as usize,a[1],a[2] as i32),
    nr::RECVMSG=>sys_recvmsg(a[0] as usize,a[1],a[2] as i32),
    nr::ACCEPT4=>sys_accept4(a[0] as usize,a[1],a[2],a[3] as i32),
    nr::SENDMMSG=>sys_sendmmsg(a[0] as usize,a[1],a[2] as u32,a[3] as i32),
    nr::RECVMMSG=>sys_recvmmsg(a[0] as usize,a[1],a[2] as u32,a[3] as i32,a[4]),
    nr::LISTEN=>sys_listen(a[0] as usize,a[1] as i32),
    nr::ACCEPT=>sys_accept(a[0] as usize,a[1],a[2]),
    nr::CONNECT=>sys_connect(a[0] as usize,a[1],a[2]),
    nr::SENDTO=>sys_sendto(a[0] as usize,a[1],a[2] as usize,a[3],a[4],a[5]),
    nr::RECVFROM=>sys_recvfrom(a[0] as usize,a[1],a[2] as usize,a[3],a[4],a[5]),
    nr::SOCKETPAIR=>sys_socketpair(a[0] as i32,a[1] as i32,a[2] as i32,a[3]),
    nr::SHUTDOWN=>sys_shutdown(a[0] as usize,a[1] as i32),
    nr::GETSOCKNAME=>sys_getsockname(a[0] as usize,a[1],a[2]),
    nr::GETPEERNAME=>sys_getpeername(a[0] as usize,a[1],a[2]),
    nr::GETCWD=>sys_getcwd(a[0],a[1]),
    nr::STATFS=>sys_statfs(a[0],a[1]), nr::FSTATFS=>sys_fstatfs(a[0] as usize,a[1]),
    nr::READLINK=>sys_readlink(a[0],a[1],a[2]),
    nr::CLOSE_RANGE=>sys_close_range(a[0] as usize,a[1] as usize,a[2]),
    nr::FADVISE64=>0, nr::SYNC=>0, nr::GETCPU=>0, nr::SCHED_SETAFFINITY=>0, nr::MINCORE=>sys_mincore(a[0],a[1],a[2]),
    nr::MLOCK2=>0, nr::PERSONALITY=>0, nr::NICE=>0, nr::SYNC_FILE_RANGE=>0, nr::FALLOCATE=>errno::neg(errno::ENOSYS),
    nr::SENDFILE=>sys_sendfile(a[0] as usize,a[1] as usize,a[2],a[3] as usize), nr::SPLICE=>errno::neg(errno::ENOSYS), nr::TEE=>errno::neg(errno::ENOSYS),
    nr::COPY_FILE_RANGE=>errno::neg(errno::ENOSYS), nr::FUTIMESAT=>0, nr::UTIMES=>0,
    nr::GETXATTR=>errno::neg(errno::ENODATA), nr::LGETXATTR=>errno::neg(errno::ENODATA), nr::FGETXATTR=>errno::neg(errno::ENODATA),
    nr::SETXATTR=>errno::neg(errno::EOPNOTSUPP), nr::LSETXATTR=>errno::neg(errno::EOPNOTSUPP), nr::FSETXATTR=>errno::neg(errno::EOPNOTSUPP),
    nr::LISTXATTR=>0, nr::LLISTXATTR=>0, nr::FLISTXATTR=>0,
    nr::REMOVEXATTR=>errno::neg(errno::ENODATA), nr::LREMOVEXATTR=>errno::neg(errno::ENODATA), nr::FREMOVEXATTR=>errno::neg(errno::ENODATA),
    nr::INOTIFY_ADD_WATCH=>errno::neg(errno::ENOSYS), nr::INOTIFY_RM_WATCH=>errno::neg(errno::ENOSYS),
    nr::SIGNALFD4=>errno::neg(errno::ENOSYS), nr::TIMERFD_SETTIME=>errno::neg(errno::ENOSYS), nr::TIMERFD_GETTIME=>errno::neg(errno::ENOSYS),
    nr::USERFAULTFD=>errno::neg(errno::ENOSYS), nr::MEMFD_CREATE=>errno::neg(errno::ENOSYS), nr::MKNODAT=>errno::neg(errno::ENOSYS),
    nr::SETPGID=>sys_setpgid(a[0] as u32,a[1] as u32), nr::GETPGID=>sys_getpgid(a[0] as u32),
    nr::SETSID=>sys_setsid(), nr::GETSID=>sys_getsid(a[0] as u32),
    nr::PRCTL=>sys_prctl(a[0] as i32,a[1],a[2],a[3],a[4]),
    nr::SYSINFO=>sys_sysinfo(a[0]),
    nr::MSYNC=>0, nr::MLOCK=>proc_result(|p|p.mlock(a[0] as usize,a[1] as usize)), nr::MUNLOCK=>proc_result(|p|p.munlock(a[0] as usize,a[1] as usize)), nr::MLOCKALL=>0, nr::MUNLOCKALL=>0,
    nr::CHDIR=>sys_chdir(a[0]), nr::FCHDIR=>sys_fchdir(a[0] as usize),
    nr::MKDIR=>sys_mkdir(a[0],a[1] as u32), nr::MKDIRAT=>sys_mkdirat(a[0] as i64,a[1],a[2] as u32),
    nr::RMDIR=>sys_rmdir(a[0]),
    nr::RENAME=>sys_rename(a[0],a[1]), nr::RENAMEAT=>sys_renameat(a[0] as i64,a[1],a[2] as i64,a[3]),
    nr::CHMOD=>sys_chmod(a[0],a[1] as u32), nr::FCHMOD=>sys_fchmod(a[0] as usize,a[1] as u32), nr::FCHMODAT=>sys_fchmodat(a[0] as i64,a[1],a[2] as u32,a[3]),
    nr::TRUNCATE=>sys_truncate(a[0],a[1]), nr::FTRUNCATE=>sys_ftruncate(a[0] as usize,a[1]),
    nr::DUP=>sys_dup(a[0] as usize),
    nr::GETTIMEOFDAY=>sys_gettimeofday(a[0],a[1]),
    nr::GETRLIMIT=>sys_getrlimit(a[0],a[1]), nr::SETRLIMIT=>sys_setrlimit(a[0],a[1]),
    nr::GETRUSAGE=>sys_getrusage(a[0],a[1]),
    nr::CLOCK_GETRES=>sys_clock_getres(a[0],a[1]), nr::CLOCK_NANOSLEEP=>sys_clock_nanosleep(a[0],a[1],a[2],a[3]), nr::CLOCK_SETTIME=>sys_clock_settime(a[0],a[1]),
    nr::SELECT=>sys_select(a[0] as i32,a[1],a[2],a[3],a[4]),
    nr::PSELECT6=>sys_pselect6(a[0] as i32,a[1],a[2],a[3],a[4],a[5]),
    nr::UTIMENSAT=>sys_utimensat(a[0] as i64,a[1],a[2],a[3]),
    nr::CHOWN=>0, nr::FCHOWN=>0, nr::LCHOWN=>0, nr::FCHOWNAT=>0,
    nr::UTIME=>0,
    nr::IOCTL=>sys_ioctl(a[0] as usize,a[1],a[2]),
    nr::IO_URING_SETUP=>sys_io_uring_setup(a[0] as u32,a[1]), nr::IO_URING_ENTER=>sys_io_uring_enter(a[0] as usize,a[1] as u32,a[2] as u32,a[3] as u32,a[4],a[5]),
    nr::IO_URING_REGISTER=>sys_io_uring_register(a[0] as usize,a[1] as u32,a[2],a[3] as u32),
    nr::PIDFD_OPEN=>sys_pidfd_open(a[0] as u32,a[1]), nr::PIDFD_SEND_SIGNAL=>sys_pidfd_send_signal(a[0] as usize,a[1] as i32,a[2],a[3]),
    nr::PIDFD_GETFD=>errno::neg(errno::ENOSYS),
    nr::EXIT|nr::EXIT_GROUP=>crate::task::exit(a[0] as i32), _=>errno::neg(errno::ENOSYS),
}}
fn fd_id(fd:u64)->Option<u32>{let r=crate::task::with_current_process(|p|p.fds.get(fd as usize)).flatten();r}
fn proc_result(f:impl FnOnce(&mut crate::process::Process)->Result<usize,i64>)->i64{match crate::task::with_current_process(f){Some(Ok(v))=>v as i64,Some(Err(e))=>errno::neg(e),None=>errno::neg(errno::ESRCH)}}
fn proc_unit(f:impl FnOnce(&mut crate::process::Process)->Result<(),i64>)->i64{match crate::task::with_current_process(f){Some(Ok(()))=>0,Some(Err(e))=>errno::neg(e),None=>errno::neg(errno::ESRCH)}}
fn proc_u64(f:impl FnOnce(&mut crate::process::Process)->Result<u64,i64>)->i64{match crate::task::with_current_process(f){Some(Ok(v))=>v as i64,Some(Err(e))=>errno::neg(e),None=>errno::neg(errno::ESRCH)}}
fn fd_result(r:Result<i32,i64>)->i64{match r{Ok(v)=>v as i64,Err(e)=>errno::neg(e)}}
fn sys_read(fd:u64,ptr:u64,len:u64)->i64 {
 if len>isize::MAX as u64{return errno::neg(errno::EFAULT)}
 let id=match fd_id(fd){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 /* Sockets return one recv's worth; looping would block waiting for the
    remainder of a length the peer never promised. */
 let single_shot=matches!(crate::fd::kind(id),Some(crate::fd::OpenKind::Socket(_)));
 let mut done=0usize;let mut buf=[0u8;256];
 while done<len as usize{let wanted=core::cmp::min(buf.len(),len as usize-done);
  let n=match crate::fd::read(id,&mut buf[..wanted]){Ok(n)=>n,Err(e)=>{return if done==0{errno::neg(e)}else{done as i64}}};
  if n==0{break}
  let dst=match (ptr as usize).checked_add(done){Some(v)=>v,None=>return errno::neg(errno::EFAULT)};
  if let Err(e)=crate::usercopy::copy_to_user(dst,&buf[..n]){return if done==0{errno::neg(e)}else{done as i64}}
  done+=n;
  if single_shot||n<wanted{break}
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
  Some(crate::fd::OpenKind::Inode(n))=>{let live=crate::vfs::inode_size(n.ino).unwrap_or(n.size);st.st_ino=n.ino;st.st_mode=n.mode;st.st_nlink=1;st.st_size=live as i64;st.st_blocks=((live+511)/512)as i64;},
  Some(crate::fd::OpenKind::Console)=>{st.st_mode=S_IFCHR|0o666;st.st_nlink=1;},
  Some(crate::fd::OpenKind::PipeRead(_))|Some(crate::fd::OpenKind::PipeWrite(_))=>{st.st_mode=S_IFIFO|0o600;st.st_nlink=1;},
  Some(crate::fd::OpenKind::Socket(_))=>{st.st_mode=0o140000|0o600;st.st_nlink=1;},
  Some(crate::fd::OpenKind::EventFd(_, _))=>{st.st_mode=S_IFCHR|0o666;st.st_nlink=1;},
  Some(crate::fd::OpenKind::Epoll)=>{st.st_mode=S_IFCHR|0o666;st.st_nlink=1;},
  Some(crate::fd::OpenKind::TimerFd(_, _))=>{st.st_mode=S_IFCHR|0o666;st.st_nlink=1;},
  Some(crate::fd::OpenKind::CharDevice(dev))=>{st.st_mode=S_IFCHR|0o666;st.st_nlink=1;st.st_rdev=dev;},
  Some(crate::fd::OpenKind::BlockDevice(dev))=>{st.st_mode=crate::vfs::S_IFBLK|0o666;st.st_nlink=1;st.st_rdev=dev;st.st_size=crate::driver::virtio_blk::capacity() as i64*512;},
  Some(crate::fd::OpenKind::Inotify)|Some(crate::fd::OpenKind::SignalFd(_))|Some(crate::fd::OpenKind::Netlink)|Some(crate::fd::OpenKind::IoUring)|Some(crate::fd::OpenKind::PidFd(_))=>{st.st_mode=S_IFCHR|0o666;st.st_nlink=1;},
  None=>return errno::neg(errno::EBADF),
 }
 st.st_blksize=4096;copy_obj(ptr,&st)}
fn sys_lseek(fd:u64,off:i64,whence:u64)->i64{let id=match fd_id(fd){Some(id)=>id,None=>return errno::neg(errno::EBADF)};match crate::fd::seek(id,off,whence){Ok(v)=>v as i64,Err(e)=>errno::neg(e)}}
fn sys_set_tid_address(ptr:u64)->i64{let _=crate::task::with_current_process(|p|p.set_clear_child_tid(ptr));crate::task::current_pid() as i64}
fn sys_clone(a:[u64;6])->i64{
 const CLONE_VM:u64=0x100;const CLONE_FS:u64=0x200;const CLONE_FILES:u64=0x400;const CLONE_SIGHAND:u64=0x800;
 const CLONE_THREAD:u64=0x10000;const CLONE_SYSVSEM:u64=0x40000;const CLONE_SETTLS:u64=0x80000;
 const CLONE_PARENT_SETTID:u64=0x100000;const CLONE_CHILD_CLEARTID:u64=0x200000;const CLONE_CHILD_SETTID:u64=0x1000000;
 /* musl uses this private flag in place of Linux CLONE_CHILD_SETTID. */
  const CLONE_DETACHED:u64=0x400000;
  const SIGCHLD:u64=17;
  let flags=a[0]&!SIGCHLD;
  const CLONE_NEWUSER:u64=0x10000000;
  const CLONE_NEWNS:u64=0x00020000;
  const PRIVILEGED_NS:u64=0x04000000|0x08000000|0x20000000|0x40000000|0x02000000|0x00000080|CLONE_NEWNS;
  if flags&PRIVILEGED_NS!=0&&flags&CLONE_NEWUSER==0&&!current_has_cap(CAP_SYS_ADMIN){return errno::neg(errno::EPERM)}
 if flags&CLONE_VM!=0{
  let allowed=CLONE_VM|CLONE_FS|CLONE_FILES|CLONE_SIGHAND|CLONE_THREAD|CLONE_SYSVSEM|CLONE_SETTLS|CLONE_PARENT_SETTID|CLONE_CHILD_CLEARTID|CLONE_CHILD_SETTID|CLONE_DETACHED|0x04000000|0x08000000|0x10000000|0x20000000|0x40000000|0x02000000|0x00000080|0x00020000;
  if flags&!allowed!=0{return errno::neg(errno::EINVAL)}
  if a[1]==0{return errno::neg(errno::EINVAL)}
  let frame=match crate::arch::x86_64::syscall::capture_fork_frame(){Some(f)=>f,None=>return errno::neg(errno::EINVAL)};
  match crate::task::spawn_thread(frame,a[1],a[4],a[3],a[2],flags){Ok(pid)=>pid as i64,Err(e)=>errno::neg(e)}
 }else{
  if a[1]!=0||a[2]!=0||a[3]!=0||a[4]!=0{return errno::neg(errno::EINVAL)}
  if flags & !(0x04000000|0x08000000|0x10000000|0x20000000|0x40000000|0x02000000|0x00000080|0x00020000)!=0{return errno::neg(errno::EINVAL)}
  let frame=match crate::arch::x86_64::syscall::capture_fork_frame(){Some(f)=>f,None=>return errno::neg(errno::EINVAL)};
  match crate::task::fork_process(frame,flags){Ok(pid)=>pid as i64,Err(e)=>errno::neg(e)}
 }
}
fn sys_wait4(pid:i64,status_ptr:u64,options:u64,rusage:u64)->i64{
 const WNOHANG:u64=1;const WUNTRACED:u64=2;
 if options&!(WNOHANG|WUNTRACED)!=0{return errno::neg(errno::EINVAL)}
 loop{match crate::task::reap_child(pid,options&WNOHANG!=0){
  Ok(crate::task::WaitOutcome::Reaped(cpid,status,utime,stime))=>{
   if status_ptr!=0{let s=status as i32;let r=copy_obj(status_ptr,&s);if r<0{return r}}
   if rusage!=0{let b=rusage_buf(utime,stime,0);if let Err(e)=crate::usercopy::copy_to_user(rusage as usize,&b){return errno::neg(e)}}
   return cpid as i64;
  }
  Ok(crate::task::WaitOutcome::NoHang)=>return 0,
  Ok(crate::task::WaitOutcome::Blocked)=>{
   /* A queued signal interrupts the wait, exactly like Linux. */
   if crate::task::has_deliverable_signal(){return errno::neg(errno::EINTR)}
   crate::task::block_current();
  }
  Err(e)=>{return errno::neg(e)},
 }}
}
fn sys_kill(pid:u32,sig:i32)->i64{
 /* sig 0 only probes for the existence of the target. */
 if sig==0{let exists=crate::task::sched_pids().iter().any(|&p|p==pid);return if exists{0}else{errno::neg(errno::ESRCH)}}
 match crate::signal::send(pid,sig){Ok(())=>{
  /* A fatal signal aimed at the caller ends the caller before the syscall
     returns, exactly like Linux. A signal with a handler is delivered
     normally, so the check only applies to the default disposition. */
  let blocked=crate::task::with_current_process(|p|p.sigmask&crate::signal::bit(sig)!=0).unwrap_or(false);
  if pid==crate::task::current_pid()&&!blocked&&crate::signal::default_terminates(sig)&&crate::signal::action_get(sig).map(|a|a.handler==crate::signal::SIG_DFL).unwrap_or(true){crate::task::exit_signal(sig);}
  0
 } Err(e)=>errno::neg(e)}
}
/* Sends a signal that carries a value. The value is a union of an integer
   and a pointer; the kernel keeps the integer. */
/* The C library's sigqueue builds the information and calls
   rt_sigqueueinfo, but programs that call the syscall directly use this
   number, so it has to work as well. */
fn sys_sigqueue(pid:u32,sig:i32,value:u64)->i64{
 if !crate::signal::valid(sig){return errno::neg(errno::EINVAL)}
 let r=crate::signal::send_value(pid,sig,value);
 match r{Ok(())=>0,Err(e)=>errno::neg(e)}
}
/* Sends a signal that carries a value. The information is the published
   one: the number of the signal, the code, the sender and the value. This
   is the call the C library makes. */
/* musl calls SYS_rt_sigqueueinfo(pid, sig, &info, sizeof(info)). */
fn sys_rt_sigqueueinfo(pid:u32,sig:i32,info_ptr:u64,_size:u64)->i64{
 if info_ptr==0{return errno::neg(errno::EFAULT)}
 let mut b=[0u8;128];
 if let Err(e)=crate::usercopy::copy_from_user(&mut b,info_ptr as usize){return errno::neg(e)}
 let signo=i32::from_ne_bytes(b[0..4].try_into().unwrap());
 if signo!=sig{return errno::neg(errno::EINVAL)}
 let code=i32::from_ne_bytes(b[8..12].try_into().unwrap());
 let value=u64::from_ne_bytes(b[20..28].try_into().unwrap());
 if !crate::signal::valid(signo){return errno::neg(errno::EINVAL)}
 match crate::signal::send_value(pid,signo,value){Ok(())=>0,Err(e)=>errno::neg(e)}
}
/* Sets or reports the alternate stack a signal handler runs on. The layout
   is the published one: the address, the flags and the size. */
fn sys_sigaltstack(newptr:u64,oldptr:u64)->i64{
 if oldptr!=0{
  let (a,f,sz)=crate::task::with_current_process(|p|(p.altstack_addr,p.altstack_flags,p.altstack_size)).unwrap_or((0,0,0));
  let mut buf=[0u8;24];
  buf[0..8].copy_from_slice(&a.to_ne_bytes());
  buf[8..12].copy_from_slice(&(f as u32).to_ne_bytes());
  buf[16..24].copy_from_slice(&sz.to_ne_bytes());
  if let Err(e)=crate::usercopy::copy_to_user(oldptr as usize,&buf){return errno::neg(e)}
 }
 if newptr!=0{
  let mut buf=[0u8;24];
  if let Err(e)=crate::usercopy::copy_from_user(&mut buf,newptr as usize){return errno::neg(e)}
  let addr=u64::from_ne_bytes(buf[0..8].try_into().unwrap());
  let flags=u32::from_ne_bytes(buf[8..12].try_into().unwrap()) as u64;
  /* ss_size follows ss_flags with padding to an 8-byte boundary. */
  let size=u64::from_ne_bytes(buf[16..24].try_into().unwrap());
  if flags&!(crate::signal::SS_ONSTACK|crate::signal::SS_DISABLE)!=0{return errno::neg(errno::EINVAL)}
  let (addr,size)=if flags&crate::signal::SS_DISABLE!=0{(0,0)}else{
   if size<crate::signal::MINSIGSTKSZ{return errno::neg(errno::ENOMEM)}
   if addr==0{return errno::neg(errno::ENOMEM)}
   if addr.checked_add(size).map_or(true,|e|e>=crate::usercopy::USER_MAX as u64){return errno::neg(errno::ENOMEM)}
   (addr,size)
  };
  crate::task::with_current_process(|p|{p.altstack_addr=addr;p.altstack_size=size;p.altstack_flags=(flags&crate::signal::SS_ONSTACK) as u32;});
 }
 0
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
 let new_mask=if set==0{0}else{match user_u64(set){Ok(v)=>v,Err(e)=>{return errno::neg(e)}}};
  let prev=match crate::signal::mask_op(how,new_mask){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 if old!=0{let bytes=unsafe{core::slice::from_raw_parts((&prev as*const u64).cast::<u8>(),8)};if let Err(e)=crate::usercopy::copy_to_user(old as usize,bytes){return errno::neg(e)}}0
}
/* rt_sigsuspend(sigset_size, newset, oldset): install the new mask, sleep
   until a signal arrives, then restore the previous mask. */
fn sys_rt_sigsuspend(newset:u64)->i64{
 let new_mask=if newset==0{0}else{match user_u64(newset){Ok(v)=>v,Err(e)=>{return errno::neg(e)}}};
 let prev=match crate::signal::mask_op(crate::signal::SETMASK,new_mask){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let _=prev;
 /* Sleep until a signal is deliverable; the signal wakes this task. */
 loop{
  if crate::signal::pending().is_some(){break}
  if !crate::task::block_current_until_signal(){break}
 }
 /* The suspend mask stays in effect: the signal that ends the sleep is
    delivered at the syscall exit, and it must still be unblocked then. */
 errno::neg(errno::EINTR)
}
fn sys_rt_sigreturn()->i64{unsafe{crate::arch::x86_64::syscall::signal_restore_frame()}}
fn sys_unlinkat(dirfd:i64,path:u64,flags:u64)->i64{
 const AT_FDCWD:i64=-100;const AT_REMOVEDIR:u64=0x200;
 if dirfd!=AT_FDCWD||flags&!AT_REMOVEDIR!=0{return errno::neg(errno::EINVAL)}
 let (out,n)=match resolve_path_ptr(path){Ok(v)=>v,Err(e)=>return e};
 let p=match core::str::from_utf8(&out[..n]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
 if flags&AT_REMOVEDIR!=0{match crate::vfs::rmdir(p){Ok(())=>0,Err(e)=>errno::neg(e)}}else{match crate::vfs::unlink(p){Ok(())=>0,Err(e)=>errno::neg(e)}}
}
fn sys_pipe(ptr:u64,flags:u64)->i64{let(r,w)=match crate::fd::pipe2(flags){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let pair=[r as u32,w as u32];let bytes=unsafe{core::slice::from_raw_parts(pair.as_ptr()as*const u8,8)};
 match crate::usercopy::copy_to_user(ptr as usize,bytes){Ok(())=>0,Err(e)=>{let _=crate::fd::close(r as usize);let _=crate::fd::close(w as usize);errno::neg(e)}}}
fn sys_dup3(oldfd:usize,newfd:usize,flags:u64)->i64{
 if flags&!0x80000!=0||oldfd==newfd{return errno::neg(errno::EINVAL)}
 match crate::fd::dup2(oldfd,newfd){
  Ok(fd)=>{if flags&0x80000!=0{let _=crate::task::with_current_process(|p|p.fds.set_close_exec(newfd,true));}fd as i64}
  Err(e)=>errno::neg(e),
 }
}
struct Flock{l_type:i16,l_whence:i16,l_start:i64,l_len:i64,l_pid:i32}
fn read_flock(ptr:u64)->Result<Flock,i64>{
 if ptr==0{return Err(errno::EFAULT)}
 let mut b=[0u8;32];
 crate::usercopy::copy_from_user(&mut b,ptr as usize)?;
 Ok(Flock{l_type:i16::from_ne_bytes(b[0..2].try_into().unwrap()),l_whence:i16::from_ne_bytes(b[2..4].try_into().unwrap()),l_start:i64::from_ne_bytes(b[8..16].try_into().unwrap()),l_len:i64::from_ne_bytes(b[16..24].try_into().unwrap()),l_pid:i32::from_ne_bytes(b[24..28].try_into().unwrap())})
}
fn write_flock(ptr:u64,f:&Flock)->Result<(),i64>{
 if ptr==0{return Err(errno::EFAULT)}
 let mut b=[0u8;32];
 b[0..2].copy_from_slice(&f.l_type.to_ne_bytes());b[2..4].copy_from_slice(&f.l_whence.to_ne_bytes());
 b[8..16].copy_from_slice(&f.l_start.to_ne_bytes());b[16..24].copy_from_slice(&f.l_len.to_ne_bytes());
 b[24..28].copy_from_slice(&f.l_pid.to_ne_bytes());
 crate::usercopy::copy_to_user(ptr as usize,&b)
}
/* Resolves the würfl range against the file size and current offset. */
fn flock_range(f:&Flock,size:u64,cur:u64)->Result<(u64,u64),i64>{
 let base=match f.l_whence{0=>0i128,1=>cur as i128,2=>size as i128,_=>return Err(errno::EINVAL)};
 let start=base+f.l_start as i128;
 if start<0||start>u64::MAX as i128{return Err(errno::EOVERFLOW)}
 if f.l_len==0{return Ok((start as u64,u64::MAX))}
 let end=start+f.l_len as i128;
 if end<0||end>u64::MAX as i128{return Err(errno::EINVAL)}
 if f.l_len<0{Ok((end as u64,start as u64))}else{Ok((start as u64,end as u64))}
}
fn sys_fcntl_locks(fd:u64,cmd:u64,arg:u64)->i64{
 use crate::record_lock::{F_RDLCK,F_WRLCK,F_UNLCK,Owner};
 let ofd=cmd>=36;
 let getlk=cmd==5||cmd==36;
 let blocking=cmd==7||cmd==38;
 let id=match fd_id(fd){Some(i)=>i,None=>return errno::neg(errno::EBADF)};
 let (ino,size)=match crate::fd::kind(id){
  Some(crate::fd::OpenKind::Inode(n))=>(n.ino,crate::vfs::inode_size(n.ino).unwrap_or(n.size)),
  _=>return errno::neg(errno::EINVAL),
 };
 let mut f=match read_flock(arg){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 if f.l_type!=F_RDLCK&&f.l_type!=F_WRLCK&&f.l_type!=F_UNLCK{return errno::neg(errno::EINVAL)}
 if getlk&&f.l_type==F_UNLCK{return errno::neg(errno::EINVAL)}
 let (start,end)=match flock_range(&f,size,crate::fd::offset_get(id)){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let pid=crate::task::current_pid();
 let owner=if ofd{Owner::Ofd(id)}else{Owner::Pid(pid)};
 if getlk{
  let exclusive=f.l_type==F_WRLCK;
  match crate::record_lock::query(ino,owner,exclusive,start,end){
   None=>{f.l_type=F_UNLCK;f.l_pid=0;}
   Some((rpid,rexcl,s,e))=>{f.l_type=if rexcl{F_WRLCK}else{F_RDLCK};f.l_whence=0;f.l_start=s as i64;f.l_len=if e==u64::MAX{0}else{(e-s)as i64};f.l_pid=rpid as i32;}
  }
  return match write_flock(arg,&f){Ok(())=>0,Err(e)=>errno::neg(e)};
 }
 if f.l_type==F_UNLCK{return match crate::record_lock::unlock(ino,owner,start,end){Ok(())=>0,Err(e)=>errno::neg(e)}}
 let exclusive=f.l_type==F_WRLCK;
 match crate::record_lock::set(ino,owner,exclusive,start,end,blocking){Ok(())=>0,Err(e)=>errno::neg(e)}
}
fn sys_fcntl(fd:usize,cmd:u64,arg:u64)->i64{
 let O_ACCMODE:u32=3;
 match cmd{
  0=>fd_result(crate::fd::dup_from(fd,arg as usize)), // F_DUPFD
  1=>match crate::task::with_current_process(|p|p.fds.get_close_exec(fd)){Some(Some(v))=>if v{1}else{0},_=>errno::neg(errno::EBADF)}, // F_GETFD
  2=>match crate::task::with_current_process(|p|p.fds.set_close_exec(fd,arg&1!=0)){Some(Ok(()))=>0,Some(Err(e))=>errno::neg(e),None=>errno::neg(errno::EBADF)}, // F_SETFD
  3=>match fd_id(fd as u64){Some(id)=>i64::from(O_ACCMODE|crate::fd::flags(id)),None=>errno::neg(errno::EBADF)}, // F_GETFL
  4=>match fd_id(fd as u64){
   Some(id)=>{
    let a=(arg as u32)&(crate::fd::O_APPEND|crate::fd::O_NONBLOCK);
    crate::fd::flags_set(id,a,(crate::fd::O_APPEND|crate::fd::O_NONBLOCK)&!a);
    0
   }
   None=>errno::neg(errno::EBADF)
  }, // F_SETFL
  5|6|7|36|37|38=>sys_fcntl_locks(fd as u64,cmd,arg), // F_GETLK/F_SETLK/F_SETLKW and F_OFD_*
  9=>fd_result(crate::fd::dup_from(fd,0)), // F_SETPIPE_SZ (unimplemented; reserve a slot low)
  10=>4096, // F_GETPIPE_SZ
  1030=>{ // F_DUPFD_CLOEXEC
   match crate::fd::dup_from(fd,arg as usize){
    Ok(newfd)=>{let _=crate::task::with_current_process(|p|p.fds.set_close_exec(newfd as usize,true));newfd as i64}
    Err(e)=>errno::neg(e),
   }
  }
  _=>0
 }
}
fn copy_path<'a>(ptr:u64,out:&'a mut[u8])->Result<&'a str,i64>{for i in 0..out.len(){crate::usercopy::copy_from_user(&mut out[i..i+1],ptr as usize+i)?;if out[i]==0{return core::str::from_utf8(&out[..i]).map_err(|_|errno::EINVAL)}}Err(errno::ENAMETOOLONG)}
fn sys_openat(dirfd:i64,path:u64,flags:u64)->i64{
 const AT_FDCWD:i64=-100; if dirfd!=AT_FDCWD||flags&!0o37777777!=0{return errno::neg(errno::EINVAL)}
 let (out,n)=match resolve_path_ptr(path){Ok(v)=>v,Err(e)=>return e};
 let p=match core::str::from_utf8(&out[..n]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
 open_common(p,flags,0o644)
}
fn sys_newfstatat(dirfd:i64,path:u64,ptr:u64,flags:u64)->i64{const AT_FDCWD:i64=-100;if dirfd!=AT_FDCWD||flags&!0x100!=0{return errno::neg(errno::EINVAL)}let (out,n)=match resolve_path_ptr(path){Ok(v)=>v,Err(e)=>return e};let p=match core::str::from_utf8(&out[..n]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};let node=match crate::vfs::lookup(p){Ok(n)=>n,Err(e)=>return errno::neg(e)};let mut st=Stat::default();st.st_ino=node.ino;st.st_mode=node.mode;st.st_nlink=1;st.st_size=node.size as i64;st.st_rdev=node.rdev;st.st_blksize=4096;st.st_blocks=((node.size+511)/512)as i64;copy_obj(ptr,&st)}
fn field(s:&str)->[u8;65]{let mut x=[0;65];let b=s.as_bytes();let n=core::cmp::min(64,b.len());x[..n].copy_from_slice(&b[..n]);x}
fn sys_uname(ptr:u64)->i64{let ns=crate::task::with_current_process(|p|p.ns_uts).unwrap_or(1);let hn=crate::task::uts_hostname(ns);let nodename=core::str::from_utf8(&hn).unwrap_or("slop").trim_end_matches('\0');let u=UtsName{sysname:field("Linux"),nodename:field(nodename),release:field("6.6.0-mnu"),version:field("Slop/MNU 0.51.0"),machine:field("x86_64"),domainname:field("localdomain")};copy_obj(ptr,&u)}
fn sys_arch_prctl(code:u64,addr:u64)->i64 {const SET_FS:u64=0x1002;const GET_FS:u64=0x1003;match code{SET_FS=>if crate::process::set_fs_base(addr){0}else{errno::neg(errno::EPERM)},GET_FS=>copy_obj(addr,&crate::task::fs_base()),_=>errno::neg(errno::EINVAL)}}
fn sys_prlimit(a:[u64;6])->i64{if a[0]!=0&&a[0]!=crate::task::current_pid() as u64{return errno::neg(errno::ESRCH)} if a[2]!=0{return errno::neg(errno::EPERM)} if a[3]!=0{copy_obj(a[3],&Rlimit{cur:u64::MAX,max:u64::MAX})}else{0}}
fn copy_obj<T>(dst:u64,obj:&T)->i64{let bytes=unsafe{core::slice::from_raw_parts((obj as *const T).cast::<u8>(),size_of::<T>())};match crate::usercopy::copy_to_user(dst as usize,bytes){Ok(())=>0,Err(e)=>errno::neg(e)}}

fn result(r:Result<usize,i64>)->i64{match r{Ok(v)=>v as i64,Err(e)=>errno::neg(e)}}
fn unit(r:Result<(),i64>)->i64{match r{Ok(())=>0,Err(e)=>errno::neg(e)}}
const EXEC_MAX_VEC:usize=8;const EXEC_MAX_STR:usize=128;
fn user_u64(ptr:u64)->Result<u64,i64>{let mut b=[0u8;8];crate::usercopy::copy_from_user(&mut b,ptr as usize)?;Ok(u64::from_ne_bytes(b))}
fn copy_exec_vec(base:u64,buf:&mut[[u8;EXEC_MAX_STR];EXEC_MAX_VEC],lens:&mut[usize;EXEC_MAX_VEC])->Result<usize,i64>{if base==0{return Ok(0)}for i in 0..EXEC_MAX_VEC{let p=user_u64(base.checked_add((i*8)as u64).ok_or(errno::EFAULT)?)?;if p==0{return Ok(i)}let mut ended=false;for j in 0..EXEC_MAX_STR{crate::usercopy::copy_from_user(&mut buf[i][j..j+1],p as usize+j)?;if buf[i][j]==0{lens[i]=j;ended=true;break}}if !ended{return Err(errno::E2BIG)}}Err(errno::E2BIG)}
fn sys_execve(path_ptr:u64,argv_ptr:u64,envp_ptr:u64)->i64{ let (out,nr)=match resolve_path_ptr(path_ptr){Ok(v)=>v,Err(e)=>return e};
 let mut path_buf=[0u8;EXEC_MAX_STR];let pn=core::cmp::min(nr,path_buf.len()-1);path_buf[..pn].copy_from_slice(&out[..pn]);path_buf[pn]=0;
 let path=match core::str::from_utf8(&path_buf[..pn]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};

 let image=match crate::vfs::image(path){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let mut ab=[[0u8;EXEC_MAX_STR];EXEC_MAX_VEC];let mut al=[0usize;EXEC_MAX_VEC];let ac=match copy_exec_vec(argv_ptr,&mut ab,&mut al){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let mut eb=[[0u8;EXEC_MAX_STR];EXEC_MAX_VEC];let mut el=[0usize;EXEC_MAX_VEC];let ec=match copy_exec_vec(envp_ptr,&mut eb,&mut el){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let mut av=["";EXEC_MAX_VEC];for i in 0..ac{av[i]=match core::str::from_utf8(&ab[i][..al[i]]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)}}
 let mut ev=["";EXEC_MAX_VEC];for i in 0..ec{ev[i]=match core::str::from_utf8(&eb[i][..el[i]]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)}}
 let fallback=[path];let argv=if ac==0{&fallback[..]}else{&av[..ac]};
 let bootstrap=match crate::task::with_current_process(|p|p.bootstrap_cr3){Some(v)=>v,None=>return errno::neg(errno::ENOEXEC)};
 let loaded=match unsafe{crate::exec_runtime::load_static_args(image,argv,&ev[..ec],path,bootstrap)}{Ok(v)=>v,Err(e)=>{crate::println!("execve: load failed path={} err={}",path,e);return errno::neg(errno::ENOEXEC)}};
 match crate::task::with_current_process(|p|p.replace_image(loaded.cr3,loaded.image_slot,loaded.stack_slot,loaded.interp_slot)){Some(Ok(()))=>{},Some(Err(e))=>return errno::neg(e),None=>return errno::neg(errno::ENOEXEC)}
 let _=crate::task::with_current_process(|p|{p.set_exe_path(path);p.reset_exec_state();p.set_exec_meta(&loaded);p.fds.close_on_exec();});
 crate::posix_timer::release_pid(crate::task::current_pid());
 crate::task::set_current_fs_base(0);
 unsafe{crate::arch::x86_64::syscall::commit_exec_return(loaded.entry,loaded.rsp)}
 crate::println!("execve: committed {} rip={:#x} rsp={:#x}",path,loaded.entry,loaded.rsp);0
}

fn clock_ns(clock:u64)->Option<u64>{
 match clock{
  0|5|8|11=>Some(crate::time::realtime_ns()),
  1|4|6|7|9|10=>Some(crate::time::monotonic_ns()),
  /* Clone tasks are separate processes in this kernel, so the process and
     thread CPU clocks coincide with the caller's own accounted ticks. */
  2|3=>Some(crate::task::with_current_process(|p|p.utime_ns.saturating_add(p.stime_ns)).unwrap_or(0)),
  _=>None
 }
}
fn sys_clock_gettime(clock:u64,ptr:u64)->i64{
 let ns=match clock_ns(clock){Some(v)=>v,None=>return errno::neg(errno::EINVAL)};
 let ts=Timespec{tv_sec:(ns/1_000_000_000)as i64,tv_nsec:(ns%1_000_000_000)as i64};
 copy_obj(ptr,&ts)
}
fn sys_clock_settime(clock:u64,ptr:u64)->i64{
 if clock!=0{return errno::neg(errno::EINVAL)}
 let mut ts=Timespec::default();
 let b=unsafe{core::slice::from_raw_parts_mut((&mut ts as*mut Timespec).cast::<u8>(),core::mem::size_of::<Timespec>())};
 if let Err(e)=crate::usercopy::copy_from_user(b,ptr as usize){return errno::neg(e)}
 if ts.tv_sec<0||ts.tv_nsec<0||ts.tv_nsec>=1_000_000_000{return errno::neg(errno::EINVAL)}
 crate::time::set_realtime_ns((ts.tv_sec as u64).saturating_mul(1_000_000_000).saturating_add(ts.tv_nsec as u64));
 0
}
fn sys_nanosleep(req:u64,rem:u64)->i64{let mut ts=Timespec::default();let b=unsafe{core::slice::from_raw_parts_mut((&mut ts as*mut Timespec).cast::<u8>(),core::mem::size_of::<Timespec>())};if let Err(e)=crate::usercopy::copy_from_user(b,req as usize){return errno::neg(e)}if ts.tv_sec<0||ts.tv_nsec<0||ts.tv_nsec>=1_000_000_000{return errno::neg(errno::EINVAL)}let ns=(ts.tv_sec as u64).saturating_mul(1_000_000_000).saturating_add(ts.tv_nsec as u64);crate::task::sleep_until(crate::time::ticks_after_ns(ns));if rem!=0{let zero=Timespec::default();let _=copy_obj(rem,&zero);}0}
fn sys_getrandom(ptr:u64,len:u64,flags:u64)->i64{if flags&!3!=0{return errno::neg(errno::EINVAL)}if len>isize::MAX as u64{return errno::neg(errno::EFAULT)}let mut done=0usize;let mut buf=[0u8;64];while done<len as usize{let n=core::cmp::min(buf.len(),len as usize-done);crate::time::fill_random(&mut buf[..n]);if let Err(e)=crate::usercopy::copy_to_user(ptr as usize+done,&buf[..n]){return if done==0{errno::neg(e)}else{done as i64}}done+=n;}done as i64}
fn sys_flock(fd:usize,op:u64)->i64{
 let id=match fd_id(fd as u64){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 let ino=match crate::fd::kind(id){
  Some(crate::fd::OpenKind::Inode(n))=>n.ino,
  Some(crate::fd::OpenKind::BlockDevice(_))|Some(crate::fd::OpenKind::CharDevice(_))=>fd as u64+1,
  None=>return errno::neg(errno::EBADF),
  _=>return 0, // pipes, sockets and anon descriptions accept flock as a no-op
 };
 match crate::flock::flock(id,ino,op){Ok(())=>0,Err(e)=>errno::neg(e)}
}
fn sys_getcwd(ptr:u64,len:u64)->i64{
 if len==0{return errno::neg(errno::EINVAL)}
 let (cwd,cwdlen)=crate::task::with_current_process(|pr|(pr.cwd,pr.cwd_len)).unwrap_or(([b'/' ;128],1));
 let n=if cwdlen==0{1}else{cwdlen};
 if len<(n as u64){return errno::neg(errno::ERANGE)}
 if let Err(e)=crate::usercopy::copy_to_user(ptr as usize,&cwd[..n]){return errno::neg(e)}
 // ensure null terminator if space
 if (n as u64)<len{let zero=[0u8;1];let _=crate::usercopy::copy_to_user(ptr as usize+n,&zero);}
 n as i64
}
fn sys_ioctl(fd:usize,cmd:u64,arg:u64)->i64{
 let id=match fd_id(fd as u64){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 match crate::fd::kind(id){
  Some(crate::fd::OpenKind::Console)=>{
   match cmd{
    0x5401=>{ // TCGETS
     if arg!=0{let zeros=[0u8;36];let _=crate::usercopy::copy_to_user(arg as usize,&zeros);}
     0
    }
    0x5413=>{ // TIOCGWINSZ
     if arg!=0{let zeros=[0u8;8];let _=crate::usercopy::copy_to_user(arg as usize,&zeros);}
     0
    }
    0x540F|0x540E=>{ // TIOCGPGRP/TIOCSPGRP
     if cmd==0x540F && arg!=0{let v=crate::task::current_pgid();let _=crate::usercopy::copy_to_user(arg as usize,&v.to_ne_bytes());}
     0
    }
    _=>errno::neg(errno::ENOTTY)
   }
  }
  Some(crate::fd::OpenKind::Socket(_))=>{
   // FIONBIO etc stub
   0
  }
  Some(crate::fd::OpenKind::CharDevice(dev))=>crate::device::ioctl((dev>>8)as u32,(dev&0xff)as u32,cmd,arg),
  _=>errno::neg(errno::ENOTTY)
 }
}
fn sys_readv(fd:u64,ptr:u64,count:i32)->i64{
 if count<=0{return errno::neg(errno::EINVAL)}
 let id=match fd_id(fd){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 let mut total=0i64;
 for i in 0..count{
  let mut hdr=[0u8;16];
  if let Err(e)=crate::usercopy::copy_from_user(&mut hdr,ptr as usize+(i as usize*16)){return errno::neg(e)}
  let base=u64::from_ne_bytes(hdr[0..8].try_into().unwrap());
  let len=u64::from_ne_bytes(hdr[8..16].try_into().unwrap());
  if len==0{continue}
  let mut done=0u64;
  while done<len{
   let mut buf=[0u8;512];
   let n=core::cmp::min(buf.len(),(len-done)as usize);
   let got=match crate::fd::read(id,&mut buf[..n]){Ok(g)=>g,Err(e)=>return if total==0{errno::neg(e)}else{total}};
   if got==0{break}
   if let Err(e)=crate::usercopy::copy_to_user(base as usize+done as usize,&buf[..got]){return if total==0{errno::neg(e)}else{total}}
   done+=got as u64;
   total+=got as i64;
  }
 }
 total
}
fn sys_writev(fd:u64,ptr:u64,count:i32)->i64{
 if count<=0{return errno::neg(errno::EINVAL)}
 let id=match fd_id(fd){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 let mut total=0i64;
 for i in 0..count{
  let mut hdr=[0u8;16];
  if let Err(e)=crate::usercopy::copy_from_user(&mut hdr,ptr as usize+(i as usize*16)){return errno::neg(e)}
  let base=u64::from_ne_bytes(hdr[0..8].try_into().unwrap());
  let len=u64::from_ne_bytes(hdr[8..16].try_into().unwrap());
  if len==0{continue}
  let mut done=0u64;
  while done<len{
   let mut buf=[0u8;512];
   let n=core::cmp::min(buf.len(),(len-done)as usize);
   if let Err(e)=crate::usercopy::copy_from_user(&mut buf[..n],base as usize+done as usize){return errno::neg(e)}
   match crate::fd::write(id,&buf[..n]){Ok(m)=>{done+=m as u64;total+=m as i64;if m<n{return total}},Err(e)=>return if total==0{errno::neg(e)}else{total}}
  }
 }
 total
}
fn sys_getdents64(fd:u64,ptr:u64,count:i32)->i64{
 if count<=0{return errno::neg(errno::EINVAL)}
 let id=match fd_id(fd){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 let inode=match crate::fd::kind(id){Some(crate::fd::OpenKind::Inode(n))=>n,_=>return errno::neg(errno::ENOTDIR)};
 let path=match core::str::from_utf8(&inode.name[..inode.name_len]){Ok(p)=>p,Err(_)=>return errno::neg(errno::EINVAL)};
 let start=crate::fd::offset_get(id);
 /* Collect the directory once so the closure cannot deadlock on ROOT. */
  let mut entries:[(u64,u32,[u8;64],usize);64]=[(0,0,[0;64],0);64];
 let mut n=0usize;
 let _=crate::vfs::readdir(path,|ino,mode,name|{
   if n<64{
   let b=name.as_bytes();
   let l=core::cmp::min(b.len(),63);
   entries[n]=(ino,mode,{let mut a=[0u8;64];a[..l].copy_from_slice(&b[..l]);a},l);
   n+=1;
  }
 });
 if n==0{return 0}
 let mut buf=[0u8;1024];
 let mut off=0usize;
 let mut idx=start as usize;
 let mut emitted=false;
 while idx<n{
  let (ino,mode,nb,nl)=entries[idx];
  let reclen=(19+nl+1+7)&!7;
  if off+reclen>count as usize{break}
  let d_off=(idx+1)as u64;
  buf[off..off+8].copy_from_slice(&ino.to_ne_bytes());
  buf[off+8..off+16].copy_from_slice(&d_off.to_ne_bytes());
  buf[off+16..off+18].copy_from_slice(&(reclen as u16).to_ne_bytes());
  let d_type=match mode&0o170000{0o040000=>4u8,0o100000=>8u8,_=>0};
  buf[off+18]=d_type;
  buf[off+19..off+19+nl].copy_from_slice(&nb[..nl]);
  buf[off+19+nl]=0;
  off+=reclen;
  idx+=1;
  emitted=true;
 }
 if !emitted{return 0}
 crate::fd::offset_set(id,idx as u64);
 if let Err(e)=crate::usercopy::copy_to_user(ptr as usize,&buf[..off]){return errno::neg(e)}
 off as i64
}
fn sys_futex(uaddr:u64,op:i32,val:i32,timeout:u64,uaddr2:u64,val3:i32)->i64{
 let _=uaddr2;let _=val3;
 match crate::futex::dispatch(uaddr,op as u32,val as u32,timeout){Ok(v)=>v,Err(e)=>errno::neg(e)}
}

/* statx: fills a linux statx buffer from the path or fd. */
#[repr(C)] struct Statx{stx_mask:u32,stx_blksize:u32,stx_attributes:u64,stx_nlink:u32,stx_uid:u32,stx_gid:u32,stx_mode:u16,_pad0:[u8;6],stx_ino:u64,stx_size:u64,stx_blocks:u64,stx_attributes_mask:u64,atime:[i64;3],btime:[i64;3],ctime:[i64;3],mtime:[i64;3]}
impl Statx{fn zeroed()->Self{Self{stx_mask:0,stx_blksize:0,stx_attributes:0,stx_nlink:0,stx_uid:0,stx_gid:0,stx_mode:0,_pad0:[0;6],stx_ino:0,stx_size:0,stx_blocks:0,stx_attributes_mask:0,atime:[0;3],btime:[0;3],ctime:[0;3],mtime:[0;3]}}}
fn sys_statx(dirfd:i64,path:u64,flags:u64,mask:u64,buf:u64)->i64{
 const AT_FDCWD:i64=-100;const AT_EMPTY_PATH:u64=0x1000;const AT_SYMLINK_NOFOLLOW:u64=0x100;
 if dirfd!=AT_FDCWD||flags&!0x1000_0100!=0{return errno::neg(errno::EINVAL)}
 let _=mask;
 let inode=if flags&AT_EMPTY_PATH!=0{
  match fd_id(dirfd as u64){Some(id)=>match crate::fd::kind(id){Some(crate::fd::OpenKind::Inode(n))=>n,_=>return errno::neg(errno::EBADF)},None=>return errno::neg(errno::EBADF)}
 }else{
  let (out,nr)=match resolve_path_ptr(path){Ok(v)=>v,Err(e)=>return e};
  let p=match core::str::from_utf8(&out[..nr]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
  match crate::vfs::lookup(p){Ok(n)=>n,Err(e)=>return errno::neg(e)}
 };
 let mut st=Statx::zeroed();
 st.stx_ino=inode.ino;
 st.stx_mode=inode.mode as u16;
 st.stx_nlink=1;
 st.stx_size=inode.size;
 st.stx_blksize=4096;
 st.stx_blocks=((inode.size+511)/512)as u64;
 st.stx_mask=0x1|0x2|0x4|0x8|0x10|0x20|0x40|0x80|0x100|0x200|0x400;
 copy_obj(buf,&st)
}
fn sys_waitid(idtype:i32,id:u64,info:u64,options:i32,rusage:u64)->i64{
 const P_PID:i32=0;const P_PGID:i32=1;const P_ALL:i32=2;const WEXITED:i64=4;const WNOHANG:i64=1;const WNOWAIT:i64=0x01000000;
 let opts=options as i64;
 if opts&!(WEXITED|WNOHANG|WNOWAIT|2|0x80)!=0{return errno::neg(errno::EINVAL)}
 let selector=match idtype{P_PID=>id as i64,P_PGID=>-(id as i64),P_ALL=>-1,_=>return errno::neg(errno::EINVAL)};
 let nohang=opts&WNOHANG!=0;
 loop{
  match crate::task::reap_child(selector,nohang){
   Ok(crate::task::WaitOutcome::Reaped(pid,status,_,_))=>{
    let si_code:u32=1;let si_status:u32=status as u32&0xff;let si_signo:u32=if status&0x7f!=0{status as u32&0x7f}else{17};
    let mut b=[0u8;128];
    b[0..4].copy_from_slice(&si_signo.to_ne_bytes());
    b[4..8].copy_from_slice(&(0i32).to_ne_bytes());
    b[8..12].copy_from_slice(&si_code.to_ne_bytes());
    b[12..16].copy_from_slice(&0u32.to_ne_bytes());
    b[16..20].copy_from_slice(&pid.to_ne_bytes());
    b[20..24].copy_from_slice(&0u32.to_ne_bytes());
    b[24..28].copy_from_slice(&si_status.to_ne_bytes());
    if let Err(e)=crate::usercopy::copy_to_user(info as usize,&b){return errno::neg(e)}
    if rusage!=0{let zeros=[0u8;144];if let Err(e)=crate::usercopy::copy_to_user(rusage as usize,&zeros){return errno::neg(e)}}
    return 0;
   }
   Ok(crate::task::WaitOutcome::NoHang)=>return 0,
   Ok(crate::task::WaitOutcome::Blocked)=>{if crate::task::has_deliverable_signal(){return errno::neg(errno::EINTR)}crate::task::block_current()},
   Err(e)=>return errno::neg(e),
  }
 }
}
fn sys_readlinkat(dirfd:i64,path:u64,buf:u64,bufsiz:u64)->i64{
 const AT_FDCWD:i64=-100;
 if dirfd!=AT_FDCWD{return errno::neg(errno::EINVAL)}
 /* readlinkat reports the link itself: the last component is not followed. */
 let (out,nr)=match resolve_path_ptr_nofollow(path){Ok(v)=>v,Err(e)=>return e};
 let p=match core::str::from_utf8(&out[..nr]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
 let node=match crate::vfs::lookup(p){Ok(n)=>n,Err(e)=>return errno::neg(e)};
 if node.mode & 0o170000 != 0o120000 {return errno::neg(errno::EINVAL)}
 let n=core::cmp::min(node.size as usize,bufsiz as usize);
 if let Err(e)=crate::usercopy::copy_to_user(buf as usize,unsafe{core::slice::from_raw_parts(node.data,n)}){return errno::neg(e)}
 n as i64
}
fn sys_faccessat(dirfd:i64,path:u64,mode:u64,flags:u64)->i64{
 const AT_FDCWD:i64=-100;const AT_EACCESS:u64=0x200;const AT_SYMLINK_NOFOLLOW:u64=0x100;
 if dirfd!=AT_FDCWD||flags&!(AT_EACCESS|AT_SYMLINK_NOFOLLOW)!=0{return errno::neg(errno::EINVAL)}
 if mode!=0&&mode!=1&&mode!=2&&mode!=4&&mode!=7{return errno::neg(errno::EINVAL)}
 let (out,n)=match resolve_path_ptr(path){Ok(v)=>v,Err(e)=>return e};
 let p=match core::str::from_utf8(&out[..n]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
 match crate::vfs::lookup(p){Ok(_)=>0,Err(e)=>errno::neg(e)}
}
fn sys_execveat(dirfd:i64,path:u64,argv:u64,envp:u64,flags:u64)->i64{
 const AT_FDCWD:i64=-100;const AT_EMPTY_PATH:u64=0x1000;
 if flags&!AT_EMPTY_PATH!=0||(flags&AT_EMPTY_PATH!=0&&dirfd==AT_FDCWD){return errno::neg(errno::EINVAL)}
 if flags&AT_EMPTY_PATH!=0{return errno::neg(errno::ENOSYS)}
 sys_execve(path,argv,envp)
}
fn sys_sched_getaffinity(pid:u32,len:usize,mask:u64)->i64{
 if pid!=0&&pid!=crate::task::current_pid(){return errno::neg(errno::ESRCH)}
 if len<8{return errno::neg(errno::EINVAL)}
 let one=1u64;
 if let Err(e)=crate::usercopy::copy_to_user(mask as usize,&one.to_ne_bytes()){return errno::neg(e)}
 8
}
fn sys_poll(fds:u64,nfds:i32,timeout:i32)->i64{
 if nfds<0{return errno::neg(errno::EINVAL)}
 let mut ready=0i64;
 for i in 0..nfds.min(16){
  let mut b=[0u8;8];
  if let Err(e)=crate::usercopy::copy_from_user(&mut b,(fds as usize)+(i as usize*8)){return errno::neg(e)}
  let fd=i32::from_ne_bytes(b[0..4].try_into().unwrap());
  if fd<0{continue}
  match fd_id(fd as u64){
   Some(id)=>{
    /* A descriptor is readable when it has data or is at EOF. */
    let readable=match crate::fd::kind(id){
     Some(crate::fd::OpenKind::Console)=>true,
     Some(crate::fd::OpenKind::PipeRead(_))=>true,
     Some(crate::fd::OpenKind::Inode(_))=>true,
     Some(crate::fd::OpenKind::Socket(_))=>true,
     Some(crate::fd::OpenKind::EventFd(c,_))=>c!=0,
     _=>false,
    };
    let writable=match crate::fd::kind(id){
     Some(crate::fd::OpenKind::Console)|Some(crate::fd::OpenKind::PipeWrite(_))=>true,
     Some(crate::fd::OpenKind::Socket(_))=>true,
     Some(crate::fd::OpenKind::EventFd(_, _))=>true,
     _=>false,
    };
    if readable||writable{
     let mut rev=0u16;
     if readable{rev|=0x1}
     if writable{rev|=0x4}
     let addr=(fds as usize)+(i as usize*8)+6;
     if let Err(e)=crate::usercopy::copy_to_user(addr,&rev.to_ne_bytes()){return errno::neg(e)}
     ready+=1;
    }
   }
   None=>{return errno::neg(errno::EBADF)}
  }
 }
 if ready==0&&timeout>0{
  /* No descriptor is ready: sleep briefly and report none, matching a poll
     that timed out rather than blocking forever. */
  crate::task::sleep_until(crate::time::ticks_after_ns(timeout as u64*1_000_000));
 }
 ready
}
fn sys_ppoll(fds:u64,nfds:i32,timeout:u64,sigmask:u64)->i64{
 let t=if timeout==0{-1i32}else{
  let mut b=[0u8;16];
  if let Err(e)=unsafe{crate::usercopy::copy_from_user(&mut b,timeout as usize)}{return errno::neg(e)}
  let sec=i64::from_ne_bytes(b[0..8].try_into().unwrap());
  let nsec=i64::from_ne_bytes(b[8..16].try_into().unwrap());
  if sec<0||nsec<0{return errno::neg(errno::EINVAL)}
  if sec>i32::MAX as i64{i32::MAX}else{(sec as i32).saturating_mul(1000).saturating_add((nsec/1_000_000)as i32)}
 };
 if sigmask!=0{return errno::neg(errno::EINVAL)}
 sys_poll(fds,nfds,t)
}
fn sys_pread64(fd:u64,ptr:u64,len:i32,off:i64)->i64{
 if len<0||off<0{return errno::neg(errno::EINVAL)}
 let id=match fd_id(fd){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 let mut buf=[0u8;512];
 let mut done=0usize;
 while done<len as usize{
  let n=core::cmp::min(buf.len(),len as usize-done);
  match crate::fd::read_at(id,off as u64+done as u64,&mut buf[..n]){Ok(g)=>{if g==0{break}if let Err(e)=crate::usercopy::copy_to_user(ptr as usize+done,&buf[..g]){return errno::neg(e)}done+=g},Err(e)=>return if done==0{errno::neg(e)}else{done as i64}}
 }
 done as i64
}
fn sys_pwrite64(fd:u64,ptr:u64,len:i32,off:i64)->i64{
 if len<0||off<0{return errno::neg(errno::EINVAL)}
 let id=match fd_id(fd){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 let mut buf=[0u8;512];
 let mut done=0usize;
 while done<len as usize{
  let n=core::cmp::min(buf.len(),len as usize-done);
  if let Err(e)=crate::usercopy::copy_from_user(&mut buf[..n],ptr as usize+done){return errno::neg(e)}
  match crate::fd::write_at(id,off as u64+done as u64,&buf[..n]){Ok(g)=>done+=g,Err(e)=>return if done==0{errno::neg(e)}else{done as i64}}
 }
 done as i64
}
fn sys_fork()->i64{
 let frame=match crate::arch::x86_64::syscall::capture_fork_frame(){Some(f)=>f,None=>return errno::neg(errno::EINVAL)};
 match crate::task::fork_process(frame,0){Ok(pid)=>pid as i64,Err(e)=>errno::neg(e)}
}
fn sys_clone3(args:u64)->i64{
 let mut b=[0u8;64];
 if let Err(e)=unsafe{crate::usercopy::copy_from_user(&mut b,args as usize)}{return errno::neg(e)}
 let flags=u64::from_ne_bytes(b[0..8].try_into().unwrap())&!17;
 let ctid=u64::from_ne_bytes(b[16..24].try_into().unwrap());
 let ptid=u64::from_ne_bytes(b[24..32].try_into().unwrap());
 let stack=u64::from_ne_bytes(b[40..48].try_into().unwrap());
 let tls=u64::from_ne_bytes(b[56..64].try_into().unwrap());
 if flags&0x100!=0{
  if stack==0{return errno::neg(errno::EINVAL)}
  let frame=match crate::arch::x86_64::syscall::capture_fork_frame(){Some(f)=>f,None=>return errno::neg(errno::EINVAL)};
  match crate::task::spawn_thread(frame,stack,tls,ctid,ptid,flags){Ok(pid)=>pid as i64,Err(e)=>errno::neg(e)}
 }else{
  if flags!=0{return errno::neg(errno::EINVAL)}
  let frame=match crate::arch::x86_64::syscall::capture_fork_frame(){Some(f)=>f,None=>return errno::neg(errno::EINVAL)};
  match crate::task::fork_process(frame,flags){Ok(pid)=>pid as i64,Err(e)=>errno::neg(e)}
 }
}

/* Reads a sockaddr_un path into a fixed buffer, requiring the AF_UNIX family. */
fn sockaddr_path(addr:u64,len:u64)->Result<([u8;108],usize),i64>{
  if len<2{return Err(errno::EINVAL)}
 let mut hdr=[0u8;2];
 if let Err(e)=unsafe{crate::usercopy::copy_from_user(&mut hdr,addr as usize)}{return Err(e)}
 if u16::from_ne_bytes(hdr)!=1{return Err(errno::EAFNOSUPPORT)}
 let path_len=core::cmp::min(len as usize-2,108);
 let mut b=[0u8;108];
 if path_len==0{return Ok((b,0))}
 if let Err(e)=unsafe{crate::usercopy::copy_from_user(&mut b[..path_len],addr as usize+2)}{return Err(e)}
 let end=b[..path_len].iter().position(|&c|c==0).unwrap_or(path_len);
 if core::str::from_utf8(&b[..end]).is_err(){return Err(errno::EINVAL)}
 Ok((b,end))
}
fn sockaddr_in_of(addr:u64,len:u64)->Result<(u32,u16),i64>{
 if len<8{return Err(errno::EINVAL)}
 let mut b=[0u8;8];
 if let Err(e)=unsafe{crate::usercopy::copy_from_user(&mut b,addr as usize)}{return Err(e)}
 if u16::from_ne_bytes([b[0],b[1]])!=2{return Err(errno::EAFNOSUPPORT)}
 Ok((u32::from_be_bytes([b[4],b[5],b[6],b[7]]),u16::from_be_bytes([b[2],b[3]])))
}
fn write_sockaddr_in(addr:u64,addrlen:u64,ip:u32,port:u16)->Result<(),i64>{
 if addr==0{return Ok(())}
 let want={let mut b=[0u8;4];if crate::usercopy::copy_from_user(&mut b,addrlen as usize).is_ok(){u32::from_ne_bytes(b)as usize}else{0}};
 let mut sa=[0u8;16];
 sa[0..2].copy_from_slice(&2u16.to_ne_bytes());
 sa[2..4].copy_from_slice(&port.to_be_bytes());
 sa[4..8].copy_from_slice(&ip.to_be_bytes());
 let report=core::cmp::min(want.max(8),16);
 crate::usercopy::copy_to_user(addr as usize,&sa[..report])?;
 if addrlen!=0{let l=16u32;crate::usercopy::copy_to_user(addrlen as usize,&l.to_ne_bytes())?;}
 Ok(())
}
fn sys_socket(domain:i32,ty:i32,_proto:i32)->i64{
 let id=match domain{
  1=>match crate::socket::socket(ty as u32&0xf){Ok(s)=>s,Err(e)=>return errno::neg(e)},
  2=>match ty as u32&0xf{
   crate::socket::SOCK_DGRAM=>match crate::net::udp_open(){Ok(i)=>crate::socket::INET_UDP_BASE+i,Err(e)=>return errno::neg(e)},
   crate::socket::SOCK_STREAM=>match crate::net::tcp_open(){Ok(i)=>crate::socket::INET_TCP_BASE+i,Err(e)=>return errno::neg(e)},
   _=>return errno::neg(errno::EINVAL),
  },
  16=>match ty as u32&0xf{
   3|crate::socket::SOCK_DGRAM=>0u8,
   _=>return errno::neg(errno::EINVAL),
  },
  _=>return errno::neg(errno::EAFNOSUPPORT),
 };
 let desc=if domain==16{
  match crate::fd::alloc_file(crate::fd::OpenKind::Netlink){Ok(d)=>d,Err(e)=>return errno::neg(e)}
 }else{
  match crate::fd::alloc_file(crate::fd::OpenKind::Socket(id)){Ok(d)=>d,Err(e)=>{if domain==2{if ty as u32&0xf==crate::socket::SOCK_DGRAM{crate::net::udp_close(id)}else{crate::net::tcp_close(id)}}else{crate::socket::close(id)};return errno::neg(e)}}
 };
 let ty_flags=ty as u32;
 match crate::task::with_current_process(|p|p.fds.install(desc)){
  Some(Ok(fd))=>{
   if ty_flags&crate::socket::SOCK_NONBLOCK!=0{crate::fd::flags_set(desc,crate::fd::O_NONBLOCK,0);}
   if ty_flags&crate::socket::SOCK_CLOEXEC!=0{let _=crate::task::with_current_process(|p|p.fds.set_close_exec(fd as usize,true));}
   fd as i64
  }
  Some(Err(e))=>{crate::fd::release(desc);errno::neg(e)}
  None=>{crate::fd::release(desc);errno::neg(errno::ESRCH)}
 }
}
fn sys_socketpair(domain:i32,ty:i32,_proto:i32,sv:u64)->i64{
 if domain!=1||(ty as u32!=crate::socket::SOCK_STREAM&&ty as u32!=crate::socket::SOCK_DGRAM){return errno::neg(errno::EAFNOSUPPORT)}
 let (a,b)=match crate::socket::socketpair(){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let da=match crate::fd::alloc_file(crate::fd::OpenKind::Socket(a)){Ok(d)=>d,Err(e)=>{crate::socket::close(a);crate::socket::close(b);return errno::neg(e)}};
 let db=match crate::fd::alloc_file(crate::fd::OpenKind::Socket(b)){Ok(d)=>d,Err(e)=>{crate::fd::release(da);crate::socket::close(a);crate::socket::close(b);return errno::neg(e)}};
 let installed=match crate::task::with_current_process(|p|p.fds.install_pair(da,db)){Some(Ok(pair))=>pair,Some(Err(e))=>{crate::fd::release(da);crate::fd::release(db);crate::socket::close(a);crate::socket::close(b);return errno::neg(e)},None=>{crate::fd::release(da);crate::fd::release(db);crate::socket::close(a);crate::socket::close(b);return errno::neg(errno::ESRCH)}};
 let pair=[installed.0 as u32,installed.1 as u32];
 let bytes=unsafe{core::slice::from_raw_parts(pair.as_ptr()as*const u8,8)};
 match crate::usercopy::copy_to_user(sv as usize,bytes){Ok(())=>0,Err(e)=>{let _=crate::fd::close(installed.0 as usize);let _=crate::fd::close(installed.1 as usize);errno::neg(e)}}
}
fn netlink_fd(fd:usize)->bool{
 crate::task::with_current_process(|p|p.fds.get(fd)).flatten().map(|id|matches!(crate::fd::kind(id),Some(crate::fd::OpenKind::Netlink))).unwrap_or(false)
}
fn sock_fd(fd:usize)->Result<u8,i64>{
 let r=match crate::task::with_current_process(|p|p.fds.get(fd)).flatten(){
  Some(id)=>match crate::fd::kind(id){Some(crate::fd::OpenKind::Socket(s))=>Ok(s),other=>{Err(errno::ENOTSOCK)}},
  None=>{Err(errno::EBADF)},
 };
 r
}
fn sys_bind(fd:usize,addr:u64,len:u64)->i64{
 if netlink_fd(fd){let _=(addr,len);return 0;}
 let sock=match sock_fd(fd){Ok(s)=>s,Err(e)=>return errno::neg(e)};
 if let Some(tcp)=crate::socket::inet_kind(sock){
  let (ip,port)=match sockaddr_in_of(addr,len){Ok(v)=>v,Err(e)=>return errno::neg(e)};
  let r=if tcp{crate::net::tcp_bind(sock-crate::socket::INET_TCP_BASE,ip,port)}else{crate::net::udp_bind(sock-crate::socket::INET_UDP_BASE,ip,port)};
  return match r{Ok(())=>0,Err(e)=>errno::neg(e)};
 }
 let (pb,pl)=match sockaddr_path(addr,len){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let path=match core::str::from_utf8(&pb[..pl]){Ok(p)=>p,Err(_)=>return errno::neg(errno::EINVAL)};
 match crate::socket::bind(sock,path){Ok(())=>0,Err(e)=>errno::neg(e)}
}
fn sys_listen(fd:usize,_backlog:i32)->i64{
 if netlink_fd(fd){return errno::neg(errno::EOPNOTSUPP);}
 let sock=match sock_fd(fd){Ok(s)=>s,Err(e)=>{return errno::neg(e)}};
 if crate::socket::inet_kind(sock).is_some(){return errno::neg(errno::EOPNOTSUPP)}
 match crate::socket::listen(sock){Ok(())=>0,Err(e)=>errno::neg(e)}
}
fn sys_accept(fd:usize,addr:u64,addrlen:u64)->i64{accept_impl(fd,addr,addrlen,0)}
fn accept_impl(fd:usize,addr:u64,addrlen:u64,extra:u32)->i64{
 if netlink_fd(fd){return errno::neg(errno::EOPNOTSUPP);}
 let sock=match sock_fd(fd){Ok(s)=>s,Err(e)=>return errno::neg(e)};
 if crate::socket::inet_kind(sock).is_some(){return errno::neg(errno::EOPNOTSUPP)}
 let nonblock=fd_id(fd as u64).map(crate::fd::is_nonblock).unwrap_or(false)||extra&crate::socket::SOCK_NONBLOCK!=0;
 let server=match crate::socket::accept(sock,nonblock){Ok(s)=>s,Err(e)=>return errno::neg(e)};
 let desc=match crate::fd::alloc_file(crate::fd::OpenKind::Socket(server)){Ok(d)=>d,Err(e)=>{crate::socket::close(server);return errno::neg(e)}};
 if extra&crate::socket::SOCK_NONBLOCK!=0{crate::fd::flags_set(desc,crate::fd::O_NONBLOCK,0);}
 match crate::task::with_current_process(|p|p.fds.install(desc)){
  Some(Ok(newfd))=>{
   if extra&crate::socket::SOCK_CLOEXEC!=0{let _=crate::task::with_current_process(|p|p.fds.set_close_exec(newfd as usize,true));}
   if addr!=0&&addrlen!=0{
    /* Report the peer's bound path as the accepted address. */
    let p=crate::socket::path_of(server).unwrap_or([0;108]);
    let n=crate::socket::path_len_of(server);
    let mut sa=[0u8;110];
    sa[0..2].copy_from_slice(&1u16.to_ne_bytes());
    sa[2..2+n.min(108)].copy_from_slice(&p[..n.min(108)]);
    let want={let mut b=[0u8;4];if crate::usercopy::copy_from_user(&mut b,addrlen as usize).is_ok(){u32::from_ne_bytes(b)as usize}else{0}};
    let report=core::cmp::min(want.max(2),2+n.min(108));
    if report>0{if let Err(e)=crate::usercopy::copy_to_user(addr as usize,&sa[..report]){let _=crate::fd::close(newfd as usize);return errno::neg(e)}}
    if addrlen!=0{let len=report as u32;if let Err(e)=crate::usercopy::copy_to_user(addrlen as usize,&len.to_ne_bytes()){let _=crate::fd::close(newfd as usize);return errno::neg(e)}}
   }
   newfd as i64
  }
  Some(Err(e))=>{crate::fd::release(desc);errno::neg(e)}
  None=>{crate::fd::release(desc);errno::neg(errno::ESRCH)}
 }
}
fn sys_connect(fd:usize,addr:u64,len:u64)->i64{
 if netlink_fd(fd){let _=(addr,len);return 0;}
 let sock=match sock_fd(fd){Ok(s)=>s,Err(e)=>return errno::neg(e)};
 if let Some(tcp)=crate::socket::inet_kind(sock){
  let (ip,port)=match sockaddr_in_of(addr,len){Ok(v)=>v,Err(e)=>return errno::neg(e)};
  let r=if tcp{crate::net::tcp_connect(sock-crate::socket::INET_TCP_BASE,ip,port)}else{crate::net::udp_connect(sock-crate::socket::INET_UDP_BASE,ip,port)};
  return match r{Ok(())=>0,Err(e)=>errno::neg(e)};
 }
 let (pb,pl)=match sockaddr_path(addr,len){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let path=match core::str::from_utf8(&pb[..pl]){Ok(p)=>p,Err(_)=>return errno::neg(errno::EINVAL)};
 match crate::socket::connect(sock,path){Ok(())=>0,Err(e)=>errno::neg(e)}
}
fn sys_sendto(fd:usize,buf:u64,len:usize,_flags:u64,addr:u64,addrlen:u64)->i64{
 let nonblock=fd_id(fd as u64).map(crate::fd::is_nonblock).unwrap_or(false)||_flags&0x40!=0;
 if netlink_fd(fd){let _=(buf,addr,addrlen);return len as i64;}
 let sock=match sock_fd(fd){Ok(s)=>s,Err(e)=>return errno::neg(e)};
 if let Some(tcp)=crate::socket::inet_kind(sock){
  if tcp{
   let mut chunk=[0u8;512];let mut done=0usize;
   while done<len{
    let n=core::cmp::min(chunk.len(),len-done);
    if let Err(e)=unsafe{crate::usercopy::copy_from_user(&mut chunk[..n],buf as usize+done)}{return if done==0{errno::neg(e)}else{done as i64}}
    match crate::net::tcp_send(sock-crate::socket::INET_TCP_BASE,&chunk[..n]){Ok(g)=>{done+=g;if g<n{return done as i64}},Err(e)=>return if done==0{errno::neg(e)}else{done as i64}}
   }
   return done as i64;
  }
  let to=if addr!=0{Some(match sockaddr_in_of(addr,addrlen){Ok(v)=>v,Err(e)=>return errno::neg(e)})}else{None};
  if len>1472{return errno::neg(errno::EMSGSIZE)}
  let mut chunk=[0u8;1472];
  if let Err(e)=unsafe{crate::usercopy::copy_from_user(&mut chunk[..len],buf as usize)}{return errno::neg(e)}
  return match crate::net::udp_send(sock-crate::socket::INET_UDP_BASE,to,&chunk[..len]){Ok(g)=>g as i64,Err(e)=>errno::neg(e)};
 }
 let mut chunk=[0u8;512];let mut done=0usize;
 while done<len{
  let n=core::cmp::min(chunk.len(),len-done);
  if let Err(e)=unsafe{crate::usercopy::copy_from_user(&mut chunk[..n],buf as usize+done)}{return if done==0{errno::neg(e)}else{done as i64}}
  match crate::socket::send(sock,&chunk[..n],nonblock){Ok(g)=>{done+=g;if g<n{return done as i64}},Err(e)=>return if done==0{errno::neg(e)}else{done as i64}}
 }
 done as i64
}
fn sys_recvfrom(fd:usize,buf:u64,len:usize,_flags:u64,addr:u64,addrlen:u64)->i64{
 let nonblock=fd_id(fd as u64).map(crate::fd::is_nonblock).unwrap_or(false)||_flags&0x40!=0;
 if len==0{return 0}
 if netlink_fd(fd){return 0;}
 let sock=match sock_fd(fd){Ok(s)=>s,Err(e)=>return errno::neg(e)};
 if let Some(tcp)=crate::socket::inet_kind(sock){
  let n=core::cmp::min(len,1472);
  let mut chunk=[0u8;1472];
  if tcp{
   let got=match crate::net::tcp_recv(sock-crate::socket::INET_TCP_BASE,&mut chunk[..n]){Ok(g)=>g,Err(e)=>return errno::neg(e)};
   if got==0{return 0}
   if let Err(e)=unsafe{crate::usercopy::copy_to_user(buf as usize,&chunk[..got])}{return errno::neg(e)}
   if let Ok((ip,port))=crate::net::tcp_peername(sock-crate::socket::INET_TCP_BASE){
    if let Err(e)=write_sockaddr_in(addr,addrlen,ip,port){return errno::neg(e)}
   }
   return got as i64;
  }
  let(got,ip,port)=match crate::net::udp_recv(sock-crate::socket::INET_UDP_BASE,&mut chunk[..n]){Ok(v)=>v,Err(e)=>return errno::neg(e)};
  if let Err(e)=unsafe{crate::usercopy::copy_to_user(buf as usize,&chunk[..got])}{return errno::neg(e)}
  if let Err(e)=write_sockaddr_in(addr,addrlen,ip,port){return errno::neg(e)}
  return got as i64;
 }
 let mut chunk=[0u8;512];let mut done=0usize;
 while done<len{
  let n=core::cmp::min(chunk.len(),len-done);
  let got=match crate::socket::recv(sock,&mut chunk[..n],nonblock){Ok(g)=>g,Err(e)=>return if done==0{errno::neg(e)}else{done as i64}};
  if got==0{break}
  if let Err(e)=unsafe{crate::usercopy::copy_to_user(buf as usize+done,&chunk[..got])}{return if done==0{errno::neg(e)}else{done as i64}}
  done+=got;
 }
 done as i64
}
fn sys_shutdown(fd:usize,how:i32)->i64{
 if netlink_fd(fd){let _=how;return 0;}
 let sock=match sock_fd(fd){Ok(s)=>s,Err(e)=>return errno::neg(e)};
 if let Some(tcp)=crate::socket::inet_kind(sock){
  if tcp{return match crate::net::tcp_shutdown(sock-crate::socket::INET_TCP_BASE,how){Ok(())=>0,Err(e)=>errno::neg(e)}}
  return 0;
 }
 match crate::socket::shutdown(sock){Ok(())=>0,Err(e)=>errno::neg(e)}
}
fn sys_getpeername(fd:usize,addr:u64,addrlen:u64)->i64{
 if netlink_fd(fd){return errno::neg(errno::ENOTCONN);}
 let sock=match sock_fd(fd){Ok(s)=>s,Err(e)=>return errno::neg(e)};
 if let Some(tcp)=crate::socket::inet_kind(sock){
  let peer=if tcp{crate::net::tcp_peername(sock-crate::socket::INET_TCP_BASE)}else{crate::net::udp_peername(sock-crate::socket::INET_UDP_BASE)};
  return match peer{
   Ok((ip,port))=>match write_sockaddr_in(addr,addrlen,ip,port){Ok(())=>0,Err(e)=>errno::neg(e)},
   Err(e)=>errno::neg(e),
  };
 }
 errno::neg(errno::ENOTCONN)
}
fn sys_getsockname(fd:usize,addr:u64,addrlen:u64)->i64{
 if netlink_fd(fd){
  if addr!=0 && addrlen!=0{
   let mut sa=[0u8;12]; sa[0..2].copy_from_slice(&16u16.to_ne_bytes());
   let want={let mut b=[0u8;4];if crate::usercopy::copy_from_user(&mut b,addrlen as usize).is_ok(){u32::from_ne_bytes(b)as usize}else{0}};
   let report=core::cmp::min(want,12);
   if report>0{let _=crate::usercopy::copy_to_user(addr as usize,&sa[..report]);}
   let _=crate::usercopy::copy_to_user(addrlen as usize,&(12u32).to_ne_bytes());
  }
  return 0;
 }
 let sock=match sock_fd(fd){Ok(s)=>s,Err(e)=>return errno::neg(e)};
 if let Some(tcp)=crate::socket::inet_kind(sock){
  let local=if tcp{crate::net::tcp_local(sock-crate::socket::INET_TCP_BASE)}else{crate::net::udp_local(sock-crate::socket::INET_UDP_BASE)};
  return match local{
   Ok((ip,port))=>match write_sockaddr_in(addr,addrlen,ip,port){Ok(())=>0,Err(e)=>errno::neg(e)},
   Err(e)=>errno::neg(e),
  };
 }
 let p=crate::socket::path_of(sock).unwrap_or([0;108]);
 let n=crate::socket::path_len_of(sock);
 let mut sa=[0u8;110];
 sa[0..2].copy_from_slice(&1u16.to_ne_bytes());
 sa[2..2+n.min(108)].copy_from_slice(&p[..n.min(108)]);
 if addr!=0{
  let want={let mut b=[0u8;4];if crate::usercopy::copy_from_user(&mut b,addrlen as usize).is_ok(){u32::from_ne_bytes(b)as usize}else{0}};
  let report=core::cmp::min(want.max(2),2+n.min(108));
  if report>0{if let Err(e)=crate::usercopy::copy_to_user(addr as usize,&sa[..report]){return errno::neg(e)}}
  if addrlen!=0{let len=report as u32;if let Err(e)=crate::usercopy::copy_to_user(addrlen as usize,&len.to_ne_bytes()){return errno::neg(e)}}
 }
 0
}

fn resolve_path_ptr(path_ptr:u64)->Result<([u8;128],usize),i64>{
 let mut b=[0u8;128];
 let p=copy_path(path_ptr,&mut b)?;
 let (cwd,cwdlen,root,rootlen)=crate::task::with_current_process(|pr|(pr.cwd,pr.cwd_len,pr.root,pr.root_len)).unwrap_or(([0u8;128],1,[b'/' ;128],1));
 let mut input=[0u8;256];let mut ilen=0usize;
 if p.starts_with('/') && rootlen>1 {
  let rl=rootlen.min(120);
  input[..rl].copy_from_slice(&root[..rl]); ilen=rl;
  if ilen>0 && input[ilen-1]!=b'/'{input[ilen]=b'/';ilen+=1;}
  let pl=p.len().min(120-ilen);
  input[ilen..ilen+pl].copy_from_slice(&p.as_bytes()[..pl]); ilen+=pl;
  input[ilen]=0;
 }
 let input_str=if ilen>0 && input[0]!=0 {core::str::from_utf8(&input[..ilen]).unwrap_or(p)} else {p};
 let cwd_slice=if cwdlen==0{&[b'/'][..]}else{&cwd[..cwdlen]};
 let mut out=[0u8;128];
  let n=crate::vfs::resolve_follow(cwd_slice,input_str,true,&mut out).map_err(|e|errno::neg(e))?;
 Ok((out,n))
}
fn resolve_path_ptr_nofollow(path_ptr:u64)->Result<([u8;128],usize),i64>{
 let mut b=[0u8;128];
 let p=copy_path(path_ptr,&mut b)?;
 let (cwd,cwdlen,root,rootlen)=crate::task::with_current_process(|pr|(pr.cwd,pr.cwd_len,pr.root,pr.root_len)).unwrap_or(([0u8;128],1,[b'/' ;128],1));
 let mut input=[0u8;256];let mut ilen=0usize;
 if p.starts_with('/') && rootlen>1 {
  let rl=rootlen.min(120);
  input[..rl].copy_from_slice(&root[..rl]); ilen=rl;
  if ilen>0 && input[ilen-1]!=b'/'{input[ilen]=b'/';ilen+=1;}
  let pl=p.len().min(120-ilen);
  input[ilen..ilen+pl].copy_from_slice(&p.as_bytes()[..pl]); ilen+=pl;
 }
 let input_str=if ilen>0 {core::str::from_utf8(&input[..ilen]).unwrap_or(p)} else {p};
 let cwd_slice=if cwdlen==0{&[b'/'][..]}else{&cwd[..cwdlen]};
 let mut out=[0u8;128];
  let n=crate::vfs::resolve_follow(cwd_slice,input_str,false,&mut out).map_err(|e|errno::neg(e))?;
 Ok((out,n))
}
fn sys_chdir(path_ptr:u64)->i64{
 let (out,n)=match resolve_path_ptr(path_ptr){Ok(v)=>v,Err(e)=>return e};
 let path=match core::str::from_utf8(&out[..n]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
 if !crate::vfs::is_dir(path){return errno::neg(errno::ENOTDIR)}
 let _=crate::task::with_current_process(|pr|{pr.set_cwd(path);});
 0
}
fn sys_fchdir(fd:usize)->i64{
 let id=match fd_id(fd as u64){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 match crate::fd::kind(id){
  Some(crate::fd::OpenKind::Inode(n))=>{
   if n.mode & 0o170000 != crate::vfs::S_IFDIR as u32 {return errno::neg(errno::ENOTDIR)}
   let path=core::str::from_utf8(&n.name[..n.name_len]).unwrap_or("/");
   let _=crate::task::with_current_process(|pr|{pr.set_cwd(path);});
   0
  }
  _=>errno::neg(errno::ENOTDIR)
 }
}
fn sys_mkdir(path_ptr:u64,mode:u32)->i64{
 let (out,n)=match resolve_path_ptr(path_ptr){Ok(v)=>v,Err(e)=>return e};
 let path=match core::str::from_utf8(&out[..n]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
 match crate::vfs::mkdir(path,mode&0o777){Ok(_)=>0,Err(e)=>errno::neg(e)}
}
fn sys_mkdirat(dirfd:i64,path_ptr:u64,mode:u32)->i64{
 const AT_FDCWD:i64=-100;
 if dirfd!=AT_FDCWD{return errno::neg(errno::ENOSYS)}
 sys_mkdir(path_ptr,mode)
}
fn sys_rmdir(path_ptr:u64)->i64{
 let (out,n)=match resolve_path_ptr(path_ptr){Ok(v)=>v,Err(e)=>return e};
 let path=match core::str::from_utf8(&out[..n]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
 match crate::vfs::rmdir(path){Ok(())=>0,Err(e)=>errno::neg(e)}
}
fn sys_rename(old_ptr:u64,new_ptr:u64)->i64{
 let (o,on)=match resolve_path_ptr_nofollow(old_ptr){Ok(v)=>v,Err(e)=>return e};
 let (nw,nn)=match resolve_path_ptr_nofollow(new_ptr){Ok(v)=>v,Err(e)=>return e};
 let old=match core::str::from_utf8(&o[..on]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
 let newp=match core::str::from_utf8(&nw[..nn]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
 match crate::vfs::rename(old,newp){Ok(())=>0,Err(e)=>errno::neg(e)}
}
fn sys_renameat(olddir:i64,old_ptr:u64,newdir:i64,new_ptr:u64)->i64{
 const AT_FDCWD:i64=-100;
 if olddir!=AT_FDCWD||newdir!=AT_FDCWD{return errno::neg(errno::ENOSYS)}
 sys_rename(old_ptr,new_ptr)
}
fn sys_chmod(path_ptr:u64,mode:u32)->i64{
 let (out,n)=match resolve_path_ptr(path_ptr){Ok(v)=>v,Err(e)=>return e};
 let path=match core::str::from_utf8(&out[..n]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
 match crate::vfs::chmod_path(path,mode){Ok(())=>0,Err(e)=>errno::neg(e)}
}
fn sys_fchmod(fd:usize,mode:u32)->i64{
 let id=match fd_id(fd as u64){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 match crate::fd::kind(id){
  Some(crate::fd::OpenKind::Inode(n))=>match crate::vfs::chmod_ino(n.ino,mode){Ok(())=>0,Err(e)=>errno::neg(e)},
  _=>errno::neg(errno::EBADF)
 }
}
fn sys_fchmodat(dirfd:i64,path_ptr:u64,mode:u32,flags:u64)->i64{
 const AT_FDCWD:i64=-100;
 if dirfd!=AT_FDCWD||flags!=0{return errno::neg(errno::ENOSYS)}
 sys_chmod(path_ptr,mode)
}
fn sys_truncate(path_ptr:u64,len:u64)->i64{
 let (out,n)=match resolve_path_ptr(path_ptr){Ok(v)=>v,Err(e)=>return e};
 let path=match core::str::from_utf8(&out[..n]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
 match crate::vfs::truncate_path(path,len){Ok(())=>0,Err(e)=>errno::neg(e)}
}
fn sys_ftruncate(fd:usize,len:u64)->i64{
 let id=match fd_id(fd as u64){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 match crate::fd::kind(id){
  Some(crate::fd::OpenKind::Inode(n))=>match crate::vfs::truncate_ino(n.ino,len){Ok(())=>0,Err(e)=>errno::neg(e)},
  _=>errno::neg(errno::EBADF)
 }
}
fn sys_dup(fd:usize)->i64{fd_result(crate::fd::dup_from(fd,0))}
fn sys_gettimeofday(tv:u64,tz:u64)->i64{
 let ns=crate::time::monotonic_ns();
 #[repr(C)] struct Timeval{tv_sec:i64,tv_usec:i64}
 let t=Timeval{tv_sec:(ns/1_000_000_000)as i64,tv_usec:((ns%1_000_000_000)/1000)as i64};
 if tv!=0{let r=copy_obj(tv,&t);if r<0{return r;}}
 if tz!=0{let zeros=[0u8;8];let _=crate::usercopy::copy_to_user(tz as usize,&zeros);}
 0
}
fn sys_getrlimit(resource:u64,rlim:u64)->i64{
 if rlim==0{return errno::neg(errno::EFAULT)}
 let _=resource;
 copy_obj(rlim,&Rlimit{cur:u64::MAX/4,max:u64::MAX/4})
}
fn sys_setrlimit(resource:u64,rlim:u64)->i64{let _=(resource,rlim);0}
fn put_u64(b:&mut[u8;144],off:usize,v:u64){b[off..off+8].copy_from_slice(&v.to_le_bytes());}
fn rusage_buf(utime_ns:u64,stime_ns:u64,maxrss_kb:u64)->[u8;144]{
 let mut b=[0u8;144];
 /* The accounting counts the time the process spent on the processor, in
    nanoseconds, measured with the time stamp counter. */
 let ut=utime_ns;
 let st=stime_ns;
 put_u64(&mut b,0,ut/1_000_000_000);put_u64(&mut b,8,(ut%1_000_000_000)/1000);
 put_u64(&mut b,16,st/1_000_000_000);put_u64(&mut b,24,(st%1_000_000_000)/1000);
 put_u64(&mut b,32,maxrss_kb);
 b
}
fn sys_getrusage(who:u64,ru:u64)->i64{
 if ru==0{return errno::neg(errno::EFAULT)}
 let buf=if who==u64::MAX{
  let (ut,st)=crate::task::with_current_process(|p|(p.cutime_ns,p.cstime_ns)).unwrap_or((0,0));
  rusage_buf(ut,st,0)
 }else if who==0||who==1{
  match crate::task::with_current_process(|p|(p.utime_ns,p.stime_ns,p.resident_pages()*4)){
   Some((ut,st,rss))=>rusage_buf(ut,st,rss),
   None=>return errno::neg(errno::ESRCH),
  }
 }else{return errno::neg(errno::EINVAL)};
 if let Err(e)=crate::usercopy::copy_to_user(ru as usize,&buf){return errno::neg(e)}
 0
}
fn sys_times(ptr:u64)->i64{
 if ptr!=0{
  let (ut,st,cut,cst)=crate::task::with_current_process(|p|(p.utime_ns,p.stime_ns,p.cutime_ns,p.cstime_ns)).unwrap_or((0,0,0,0));
  let mut b=[0u8;32];
  for(i,v)in[ut,st,cut,cst].iter().enumerate(){b[i*8..i*8+8].copy_from_slice(&(v/10).to_le_bytes());}
  if let Err(e)=crate::usercopy::copy_to_user(ptr as usize,&b){return errno::neg(e)}
 }
 (crate::time::monotonic_ns()/10_000_000)as i64
}
fn sys_clock_getres(clock:u64,ptr:u64)->i64{
 let ns=if clock_ns(clock).is_some(){1_000_000}else{return errno::neg(errno::EINVAL)};
 if ptr!=0{copy_obj(ptr,&Timespec{tv_sec:0,tv_nsec:ns as i64})}else{0}
}
fn sys_clock_nanosleep(clock:u64,flags:u64,req:u64,rem:u64)->i64{
 if flags&!1!=0{return errno::neg(errno::EINVAL)}
 if matches!(clock,2|3){return errno::neg(errno::EINVAL)}
 if clock_ns(clock).is_none(){return errno::neg(errno::EINVAL)}
 let mut ts=Timespec::default();
 let b=unsafe{core::slice::from_raw_parts_mut((&mut ts as*mut Timespec).cast::<u8>(),core::mem::size_of::<Timespec>())};
 if let Err(e)=crate::usercopy::copy_from_user(b,req as usize){return errno::neg(e)}
 if ts.tv_sec<0||ts.tv_nsec<0||ts.tv_nsec>=1_000_000_000{return errno::neg(errno::EINVAL)}
 let ns=(ts.tv_sec as u64).saturating_mul(1_000_000_000).saturating_add(ts.tv_nsec as u64);
 if flags&1!=0{
  let now=clock_ns(clock).unwrap_or(0);
  if ns<=now{return 0}
  crate::task::sleep_until(crate::time::ticks_after_ns(ns-now));
 }else{
  crate::task::sleep_until(crate::time::ticks_after_ns(ns));
 }
 if rem!=0{let zero=Timespec::default();let _=copy_obj(rem,&zero);}
 0
}
fn bit_get(ptr:u64,fd:i32)->bool{
 let byte=(fd/8) as usize;let bit=fd%8;
 let mut b=[0u8;1];
 if crate::usercopy::copy_from_user(&mut b,ptr as usize+byte).is_ok(){b[0]&(1<<bit)!=0}else{false}
}
fn bit_set(out:&mut [u8],fd:i32){let byte=(fd/8)as usize;let bit=fd%8;if byte<out.len(){out[byte]|=1<<bit;}}
fn select_impl(nfds:i32,rfds:u64,wfds:u64,efds:u64,mut to_ns:u64,has_timeout:bool)->i64{
 if nfds<0||nfds>1024{return errno::neg(errno::EINVAL)}
 let nwords=(nfds as usize+7)/8;
 let mut retries=0u32;
 loop{
  let mut r_out=[0u8;128];let mut w_out=[0u8;128];let mut e_out=[0u8;128];
  let mut ready=0i64;
  for fd in 0..nfds{
   let mut want_r=false;let mut want_w=false;
   if rfds!=0{want_r=bit_get(rfds,fd);}
   if wfds!=0{want_w=bit_get(wfds,fd);}
   if !want_r&&!want_w{continue;}
   match fd_id(fd as u64){
    Some(id)=>{
     let readable=match crate::fd::kind(id){Some(crate::fd::OpenKind::Console)=>true,Some(crate::fd::OpenKind::PipeRead(_))=>true,Some(crate::fd::OpenKind::Inode(_))=>true,Some(crate::fd::OpenKind::Socket(_))=>true,_=>false};
     let writable=match crate::fd::kind(id){Some(crate::fd::OpenKind::Console)|Some(crate::fd::OpenKind::PipeWrite(_))|Some(crate::fd::OpenKind::Socket(_))=>true,_=>false};
     if want_r&&readable{bit_set(&mut r_out,fd);ready+=1;}
     if want_w&&writable{bit_set(&mut w_out,fd);ready+=1;}
    }
    None=>return errno::neg(errno::EBADF)
   }
  }
  if ready>0{
   if rfds!=0{if let Err(e)=crate::usercopy::copy_to_user(rfds as usize,&r_out[..nwords]){return errno::neg(e)}}
   if wfds!=0{if let Err(e)=crate::usercopy::copy_to_user(wfds as usize,&w_out[..nwords]){return errno::neg(e)}}
   if efds!=0{let z=[0u8;128];if let Err(e)=crate::usercopy::copy_to_user(efds as usize,&z[..nwords]){return errno::neg(e)}}
   return ready;
  }
  if has_timeout{
   if to_ns==0{return 0}
   let step=core::cmp::min(to_ns,1_000_000u64);
   crate::task::sleep_until(crate::time::ticks_after_ns(step));
   to_ns=to_ns.saturating_sub(step);
  } else {
   if retries>50{return 0}
   retries+=1;
   crate::task::sleep_until(crate::time::ticks_after_ns(1_000_000));
  }
 }
}
fn sys_select(nfds:i32,rfds:u64,wfds:u64,efds:u64,timeout:u64)->i64{
 let mut to_ns=0u64;let mut has_timeout=false;
 if timeout!=0{
  has_timeout=true;
  let mut b=[0u8;16];
  if let Err(e)=crate::usercopy::copy_from_user(&mut b,timeout as usize){return errno::neg(e)}
  let sec=i64::from_ne_bytes(b[0..8].try_into().unwrap());
  let usec=i64::from_ne_bytes(b[8..16].try_into().unwrap());
  if sec<0||usec<0||usec>=1_000_000{return errno::neg(errno::EINVAL)}
  to_ns=(sec as u64)*1_000_000_000+(usec as u64)*1000;
 }
 select_impl(nfds,rfds,wfds,efds,to_ns,has_timeout)
}
fn sys_pselect6(nfds:i32,rfds:u64,wfds:u64,efds:u64,sigmask_ptr:u64,timeout:u64)->i64{
 let _=sigmask_ptr;
 let mut to_ns=0u64;let mut has_timeout=false;
 if timeout!=0{
  has_timeout=true;
  let mut b=[0u8;16];
  if let Err(e)=crate::usercopy::copy_from_user(&mut b,timeout as usize){return errno::neg(e)}
  let sec=i64::from_ne_bytes(b[0..8].try_into().unwrap());
  let nsec=i64::from_ne_bytes(b[8..16].try_into().unwrap());
  if sec<0||nsec<0{return errno::neg(errno::EINVAL)}
  to_ns=(sec as u64)*1_000_000_000+(nsec as u64);
 }
 select_impl(nfds,rfds,wfds,efds,to_ns,has_timeout)
}
fn sys_utimensat(dirfd:i64,path_ptr:u64,times:u64,flags:u64)->i64{
 const AT_FDCWD:i64=-100;
 if dirfd!=AT_FDCWD||flags!=0{return errno::neg(errno::ENOSYS)}
 let (out,n)=match resolve_path_ptr(path_ptr){Ok(v)=>v,Err(e)=>return e};
 let _=times;
 let path=match core::str::from_utf8(&out[..n]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
 match crate::vfs::lookup(path){Ok(_)=>0,Err(e)=>errno::neg(e)}
}

fn sys_stat(path_ptr:u64,ptr:u64)->i64{
 let (out,n)=match resolve_path_ptr(path_ptr){Ok(v)=>v,Err(e)=>return e};
 let path=match core::str::from_utf8(&out[..n]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
 let node=match crate::vfs::lookup(path){Ok(n)=>n,Err(e)=>return errno::neg(e)};
 let mut st=Stat::default();
 st.st_ino=node.ino;st.st_mode=node.mode;st.st_nlink=1;st.st_size=node.size as i64;st.st_rdev=node.rdev;st.st_blksize=4096;st.st_blocks=((node.size+511)/512)as i64;
 copy_obj(ptr,&st)
}
fn sys_lstat(path_ptr:u64,ptr:u64)->i64{
 let (out,n)=match resolve_path_ptr_nofollow(path_ptr){Ok(v)=>v,Err(e)=>return e};
 let path=match core::str::from_utf8(&out[..n]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
 let node=match crate::vfs::lookup(path){Ok(n)=>n,Err(e)=>return errno::neg(e)};
 let mut st=Stat::default();
 st.st_ino=node.ino;st.st_mode=node.mode;st.st_nlink=1;st.st_size=node.size as i64;st.st_rdev=node.rdev;st.st_blksize=4096;st.st_blocks=((node.size+511)/512)as i64;
 copy_obj(ptr,&st)
}
fn sys_unlink(path_ptr:u64)->i64{
 let (out,n)=match resolve_path_ptr_nofollow(path_ptr){Ok(v)=>v,Err(e)=>return e};
 let path=match core::str::from_utf8(&out[..n]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
 match crate::vfs::unlink(path){Ok(())=>0,Err(e)=>errno::neg(e)}
}

fn sys_setpgid(pid:u32,pgid:u32)->i64{match crate::task::setpgid(pid,pgid){Ok(())=>0,Err(e)=>errno::neg(e)}}
fn sys_getpgid(pid:u32)->i64{
 let p=if pid==0{crate::task::current_pid()}else{pid};
 if p!=crate::task::current_pid(){return errno::neg(errno::ESRCH)}
 crate::task::current_pgid() as i64
}
fn sys_setsid()->i64{match crate::task::setsid(){Ok(v)=>v as i64,Err(e)=>errno::neg(e)}}
fn sys_getsid(pid:u32)->i64{
 let p=if pid==0{crate::task::current_pid()}else{pid};
 if p!=crate::task::current_pid(){return errno::neg(errno::ESRCH)}
 crate::task::current_sid() as i64
}
fn sys_prctl(option:i32,arg2:u64,arg3:u64,arg4:u64,arg5:u64)->i64{
  match option{
  15=>{ // PR_SET_NAME
   let mut b=[0u8;16];
   if let Err(e)=crate::usercopy::copy_from_user(&mut b,arg2 as usize){return errno::neg(e)}
   let _=crate::task::with_current_process(|p|{p.name=b;});
   0
  }
  16=>{ // PR_GET_NAME
   let name=crate::task::with_current_process(|p|p.name).unwrap_or([0;16]);
   if let Err(e)=crate::usercopy::copy_to_user(arg2 as usize,&name){return errno::neg(e)}
   0
   }
   21=>crate::task::with_current_process(|p|p.seccomp_mode).unwrap_or(0) as i64, // PR_GET_SECCOMP
   22=>{ // PR_SET_SECCOMP
    if arg4!=0||arg5!=0{return errno::neg(errno::EINVAL)}
    if arg2==1{if arg3!=0{return errno::neg(errno::EINVAL)}sys_seccomp(0,0,0)}
    else if arg2==2{sys_seccomp(1,0,arg3)}
    else{errno::neg(errno::EINVAL)}
   }
   38=>{if arg2!=1||arg3!=0||arg4!=0||arg5!=0{return errno::neg(errno::EINVAL)}if crate::task::with_current_process(|p|{p.no_new_privs=true;}).is_some(){0}else{errno::neg(errno::ESRCH)}}
   39=>crate::task::with_current_process(|p|p.no_new_privs as i64).unwrap_or(errno::neg(errno::ESRCH)),
  8=>0,  // PR_SET_KEEPCAPS
  9=>0,  // PR_GET_KEEPCAPS
  _=>0
 }
}
fn sys_sysinfo(ptr:u64)->i64{
 if ptr==0{return errno::neg(errno::EFAULT)}
 // struct sysinfo is 112 bytes; fill zeros with uptime 1
 let mut b=[0u8;112];
 // uptime at offset 0 (long)
 b[0..8].copy_from_slice(&1i64.to_ne_bytes());
 if let Err(e)=crate::usercopy::copy_to_user(ptr as usize,&b){return errno::neg(e)}
 0
}

fn open_common(p:&str,flags:u64,mode:u64)->i64{
 const O_CREAT:u64=0o100;const O_CLOEXEC:u64=0o2000000;const O_APPEND:u64=0o2000;const O_NONBLOCK:u64=0o4000;
 let mut status=0u32;
 if flags&O_APPEND!=0{status|=crate::fd::O_APPEND}
 if flags&O_NONBLOCK!=0{status|=crate::fd::O_NONBLOCK}
 let opened=if flags&O_CREAT!=0{crate::fd::create_open_flags(p,if mode==0{0o644}else{mode as u32},status)}else{crate::fd::open_flags(p,status)};
 match opened{
  Ok(fd)=>{
   if flags&0o1000!=0{let _=crate::vfs::truncate_path(p,0);}
   if flags&O_CLOEXEC!=0{let _=crate::task::with_current_process(|proc|proc.fds.set_close_exec(fd as usize,true));}
   fd as i64
  }
  Err(e)=>errno::neg(e),
 }
}
fn sys_open(path_ptr:u64,flags:u64,mode:u64)->i64{
 let (out,n)=match resolve_path_ptr(path_ptr){Ok(v)=>v,Err(e)=>return e};
 let p=match core::str::from_utf8(&out[..n]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
 open_common(p,flags,mode)
}
fn sys_creat(path_ptr:u64,mode:u64)->i64{sys_open(path_ptr,0o100|0o1,if mode==0{0o644}else{mode})}
fn sys_fsync(fd:usize)->i64{let _=fd;0}
fn sys_epoll_create1(flags:u64)->i64{
 if flags&!0o2000000!=0{return errno::neg(errno::EINVAL)}
 let id=match crate::fd::alloc_file(crate::fd::OpenKind::Epoll){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 match crate::task::with_current_process(|p|p.fds.install(id)){Some(Ok(fd))=>fd as i64,Some(Err(e))=>{crate::fd::release(id);errno::neg(e)},None=>{crate::fd::release(id);errno::neg(errno::ESRCH)}}
}
fn sys_timerfd_settime(fd:usize,flags:u64,newptr:u64,oldptr:u64)->i64{
 let id=match fd_id(fd as u64){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 let mut buf=[0u8;32];
 if let Err(e)=crate::usercopy::copy_from_user(&mut buf,newptr as usize){return errno::neg(e)}
 let ival_sec=i64::from_ne_bytes(buf[0..8].try_into().unwrap());
 let ival_nsec=i64::from_ne_bytes(buf[8..16].try_into().unwrap());
 let val_sec=i64::from_ne_bytes(buf[16..24].try_into().unwrap());
 let val_nsec=i64::from_ne_bytes(buf[24..32].try_into().unwrap());
 if ival_sec<0||ival_nsec<0||val_sec<0||val_nsec<0{return errno::neg(errno::EINVAL)}
 let now=crate::arch::x86_64::apic::ticks();
 /* The length of a tick is measured at boot, so the delay is turned into
    ticks with the measured rate and not with a fixed unit. */
 let delay_ns=(val_sec as u64).saturating_mul(1_000_000_000).saturating_add(val_nsec as u64);
 let delay_ticks=crate::time::ns_to_ticks_ceil(delay_ns).max(1);
 let exp=now.saturating_add(delay_ticks);
 let ivl_ns=(ival_sec as u64).saturating_mul(1_000_000_000).saturating_add(ival_nsec as u64);
 let ivl_ticks=crate::time::ns_to_ticks_ceil(ivl_ns);
 // update kind
 crate::fd::timerfd_set(id,exp,ivl_ticks);
 if oldptr!=0{
  let zeros=[0u8;32]; let _=crate::usercopy::copy_to_user(oldptr as usize,&zeros);
 }
 let _=flags;
 0
}
fn sys_timerfd_gettime(fd:usize,ptr:u64)->i64{
 let id=match fd_id(fd as u64){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 let (exp,ivl)=match crate::fd::timerfd_get(id){Some(v)=>v,_=>return errno::neg(errno::EBADF)};
 let now=crate::arch::x86_64::apic::ticks();
 let rem=exp.saturating_sub(now);
 let ivl_ns=crate::time::ticks_to_ns(ivl);
 let rem_ns=crate::time::ticks_to_ns(rem);
 let mut buf=[0u8;32];
 buf[0..8].copy_from_slice(&(ivl_ns/1_000_000_000).to_ne_bytes());
 buf[8..16].copy_from_slice(&((ivl_ns%1_000_000_000)/1000).to_ne_bytes());
 buf[16..24].copy_from_slice(&(rem_ns/1_000_000_000).to_ne_bytes());
 buf[24..32].copy_from_slice(&((rem_ns%1_000_000_000)/1000).to_ne_bytes());
 if let Err(e)=crate::usercopy::copy_to_user(ptr as usize,&buf){return errno::neg(e)}
 0
}

#[repr(C)]#[derive(Clone,Copy,Default)]struct ItimerSpec{iv_sec:i64,iv_nsec:i64,v_sec:i64,v_nsec:i64}
#[repr(C)]#[derive(Clone,Copy,Default)]struct ItimerVal{iv_sec:i64,iv_sec2:i64,v_sec:i64,v_sec2:i64}
fn read_itimer<T:Default+Copy>(ptr:u64)->Result<T,i64>{
 let mut v=T::default();
 let b=unsafe{core::slice::from_raw_parts_mut((&mut v as*mut T).cast::<u8>(),size_of::<T>())};
 crate::usercopy::copy_from_user(b,ptr as usize)?;
 Ok(v)
}
fn timer_ns(sec:i64,nsec:i64)->Result<u64,i64>{if sec<0||nsec<0||nsec>=1_000_000_000{return Err(errno::EINVAL)}Ok((sec as u64).saturating_mul(1_000_000_000).saturating_add(nsec as u64))}
fn sys_timer_create(clock:i32,sevptr:u64,out:u64)->i64{
 let (notify,sig,value)=if sevptr==0{(0i32,crate::signal::SIGALRM,0u64)}else{
  let mut b=[0u8;24];
  if let Err(e)=crate::usercopy::copy_from_user(&mut b,sevptr as usize){return errno::neg(e)}
  let value=u64::from_ne_bytes(b[0..8].try_into().unwrap());
  let signo=i32::from_ne_bytes(b[8..12].try_into().unwrap());
  let n=i32::from_ne_bytes(b[12..16].try_into().unwrap());
  (n,signo,value)
 };
 let notify=match notify{
  0|4=>{if !crate::signal::valid(sig)||crate::signal::unblockable(sig){return errno::neg(errno::EINVAL)}crate::posix_timer::Notify::Signal{sig,value}},
  1=>crate::posix_timer::Notify::None,
  _=>return errno::neg(errno::EINVAL)
 };
 let pid=crate::task::current_pid();
 let id=match crate::posix_timer::create(pid,clock,notify){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let r=copy_obj(out,&id);
 if r!=0{crate::posix_timer::delete(id,pid).ok();}
 r
}
fn sys_timer_settime(id:u32,flags:u64,newptr:u64,oldptr:u64)->i64{
 if flags&!1!=0{return errno::neg(errno::EINVAL)}
 let pid=crate::task::current_pid();
 let now=crate::arch::x86_64::apic::ticks();
 let (old_rem,old_iv)=match crate::posix_timer::gettime(id,pid,now){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let spec=match read_itimer::<ItimerSpec>(newptr){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let iv_ns=match timer_ns(spec.iv_sec,spec.iv_nsec){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let v_ns=match timer_ns(spec.v_sec,spec.v_nsec){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let active=v_ns!=0;
 let interval=crate::time::ns_to_ticks_ceil(iv_ns);
 let next=if !active{0}else if flags&1!=0{let t=v_ns/1_000_000;if t<=now{now}else{t}}else{crate::time::ticks_after_ns(v_ns)};
 if let Err(e)=crate::posix_timer::settime(id,pid,active,next,interval){return errno::neg(e)}
 if oldptr!=0{
  let rem_ns=crate::time::ticks_to_ns(old_rem);let iv_ticks_ns=crate::time::ticks_to_ns(old_iv);
  let old=ItimerSpec{iv_sec:(iv_ticks_ns/1_000_000_000)as i64,iv_nsec:(iv_ticks_ns%1_000_000_000)as i64,v_sec:(rem_ns/1_000_000_000)as i64,v_nsec:(rem_ns%1_000_000_000)as i64};
  return copy_obj(oldptr,&old)
 }
 0
}
fn sys_timer_gettime(id:u32,ptr:u64)->i64{
 let now=crate::arch::x86_64::apic::ticks();
 let (rem,iv)=match crate::posix_timer::gettime(id,crate::task::current_pid(),now){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let rem_ns=crate::time::ticks_to_ns(rem);let iv_ns=crate::time::ticks_to_ns(iv);
 let spec=ItimerSpec{iv_sec:(iv_ns/1_000_000_000)as i64,iv_nsec:(iv_ns%1_000_000_000)as i64,v_sec:(rem_ns/1_000_000_000)as i64,v_nsec:(rem_ns%1_000_000_000)as i64};
 copy_obj(ptr,&spec)
}
fn sys_timer_getoverrun(id:u32)->i64{
 match crate::posix_timer::getoverrun(id,crate::task::current_pid()){Ok(v)=>v as i64,Err(e)=>errno::neg(e)}
}
fn sys_timer_delete(id:u32)->i64{match crate::posix_timer::delete(id,crate::task::current_pid()){Ok(())=>0,Err(e)=>errno::neg(e)}}
/* User-visible siginfo_t layout (matches the x86-64 kernel): for SI_TIMER
   si_timerid@16, si_overrun@20 and si_value@24. */
fn build_siginfo(sig:i32,code:i32,value:u64,overrun:u32)->[u8;128]{
 let mut b=[0u8;128];
 /* The published siginfo layout: signo, errno, code, then the union.
    si_overrun is at offset 20, si_value at offset 24. */
 b[0..4].copy_from_slice(&sig.to_ne_bytes());
 b[8..12].copy_from_slice(&code.to_ne_bytes());
 b[20..24].copy_from_slice(&(overrun as i32).to_ne_bytes());
 b[24..32].copy_from_slice(&value.to_ne_bytes());
 b
}
fn sys_rt_sigtimedwait(setptr:u64,infoptr:u64,timeoutptr:u64,setsize:u64)->i64{
 if setsize!=8{return errno::neg(errno::EINVAL)}
 let mut mb=[0u8;8];
 if let Err(e)=crate::usercopy::copy_from_user(&mut mb,setptr as usize){return errno::neg(e)}
 let mask=u64::from_ne_bytes(mb);
 let deadline=if timeoutptr!=0{
  let ts=match read_itimer::<Timespec>(timeoutptr){Ok(v)=>v,Err(e)=>return errno::neg(e)};
  if ts.tv_sec<0||ts.tv_nsec<0||ts.tv_nsec>=1_000_000_000{return errno::neg(errno::EINVAL)}
  let ns=(ts.tv_sec as u64).saturating_mul(1_000_000_000).saturating_add(ts.tv_nsec as u64);
  Some(crate::time::ticks_after_ns(ns))
 }else{None};
 loop{
  let found=crate::task::with_current_process(|p|{
   let m=p.pending&mask&!(crate::signal::unblockable_mask());
   if m==0{return None}
   let sig=(m.trailing_zeros()as i32)+1;
   /* The value comes from the process's stored value for this signal.
      Real-time signals keep a queue; the standard signals keep one. */
   let value=if crate::signal::is_rt(sig){
    let i=crate::signal::rt_index(sig);
    if p.rt_len[i]>0{p.rt_value[i][0]}else{0}
   }else{p.si_value[sig as usize]};
   if crate::signal::is_rt(sig){
    let i=crate::signal::rt_index(sig);
    if p.rt_len[i]>0{
     p.rt_len[i]-=1;
     let n=p.rt_len[i] as usize;
     let mut k=0;while k<n{p.rt_value[i][k]=p.rt_value[i][k+1];k+=1;}
     if p.rt_len[i]>0{p.pending|=crate::signal::bit(sig);}
    }
   }else{
    p.pending&=!crate::signal::bit(sig);
   }
   Some((sig,p.si_code[sig as usize],value,p.si_overrun[sig as usize]))
  }).flatten();
  if let Some((sig,code,value,overrun))=found{
   if infoptr!=0{
    let b=build_siginfo(sig,code,value,overrun);
    if let Err(e)=crate::usercopy::copy_to_user(infoptr as usize,&b){return errno::neg(e)}
    /* Read back to verify. */
    let mut rb=[0u8;4];
    let _=crate::usercopy::copy_from_user(&mut rb,(infoptr+20) as usize);
   }
   return sig as i64
  }
  if let Some(d)=deadline{
   /* The deadline is an absolute time, so the wait uses the clock. */
   if crate::time::monotonic_ns()>=crate::time::ticks_to_ns(d){return errno::neg(errno::EAGAIN)}
   crate::task::sleep_until(d);
  }else{
   crate::task::block_current();
  }
 }
}
fn sys_rt_sigpending(ptr:u64,setsize:u64)->i64{
 if setsize!=8{return errno::neg(errno::EINVAL)}
 let m=crate::task::with_current_process(|p|p.pending).unwrap_or(0);
 copy_obj(ptr,&m)
}
fn sys_pause()->i64{
 loop{
  if crate::task::has_deliverable_signal(){return errno::neg(errno::EINTR)}
  crate::task::block_current();
 }
}
fn write_itimer_val(ptr:u64,rem_ticks:u64,iv_ticks:u64)->i64{
 let rem_us=crate::time::ticks_to_ns(rem_ticks)/1000;let iv_us=crate::time::ticks_to_ns(iv_ticks)/1000;
 let val=ItimerVal{iv_sec:(iv_us/1_000_000)as i64,iv_sec2:(iv_us%1_000_000)as i64,v_sec:(rem_us/1_000_000)as i64,v_sec2:(rem_us%1_000_000)as i64};
 copy_obj(ptr,&val)
}
fn sys_alarm(sec:u64)->i64{
 let pid=crate::task::current_pid();let now=crate::arch::x86_64::apic::ticks();
 let active=sec!=0;
 /* The deadline is a number of ticks, and the length of a tick is measured
    at boot, so the seconds are turned into ticks with the measured rate. */
 let next=if active{crate::time::ticks_after_ns_floor(sec.saturating_mul(1_000_000_000))}else{0};
 match crate::posix_timer::alarm_set(pid,0,active,next,0,now){Ok((old_rem,_))=>{
  /* The remaining time is reported in whole seconds, rounded up. */
  ((crate::time::ticks_to_ns(old_rem)+999_999_999)/1_000_000_000)as i64},Err(e)=>errno::neg(e)}
}
fn sys_setitimer(which:u64,newptr:u64,oldptr:u64)->i64{
 if which>2{return errno::neg(errno::EINVAL)}
 let val=match read_itimer::<ItimerVal>(newptr){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let iv_ns=match timer_ns(val.iv_sec,val.iv_sec2){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let v_ns=match timer_ns(val.v_sec,val.v_sec2){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let pid=crate::task::current_pid();let now=crate::arch::x86_64::apic::ticks();
 let active=v_ns!=0;
 /* REAL counts wall ticks (absolute deadline); VIRTUAL/PROF count down the
    process's own CPU ticks. */
 /* The wall clock timer rounds the deadline down, so the time that is left
    never reaches the time that was asked for, which is what Linux reports
    when the timer is cancelled. */
 let next=if active{if which==0{crate::time::ticks_after_ns_floor(v_ns)}else{crate::time::ns_to_ticks_ceil(v_ns)}}else{0};
 let interval=crate::time::ns_to_ticks_ceil(iv_ns);
 let (old_rem,old_iv)=match crate::posix_timer::alarm_set(pid,which as u8,active,next,interval,now){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 if oldptr!=0{return write_itimer_val(oldptr,old_rem,old_iv)}
 0
}
fn sys_getitimer(which:u64,ptr:u64)->i64{
 if which>2{return errno::neg(errno::EINVAL)}
 let (rem,iv)=crate::posix_timer::alarm_get(crate::task::current_pid(),which as u8,crate::arch::x86_64::apic::ticks());
 write_itimer_val(ptr,rem,iv)
}
fn sys_set_robust_list(head:u64,len:u64)->i64{
 if len!=24{return errno::neg(errno::EINVAL)}
 if head!=0{
  let end=match head.checked_add(24){Some(v)=>v,None=>return errno::neg(errno::EFAULT)};
  if end>crate::usercopy::USER_MAX as u64{return errno::neg(errno::EFAULT)}
 }
 match crate::task::with_current_process(|p|{p.robust_list_head=head;p.robust_list_len=len;}){Some(())=>0,None=>errno::neg(errno::ESRCH)}
}
fn sys_get_robust_list(pid:u32,headptr:u64,lenptr:u64)->i64{
 let target=if pid==0{crate::task::current_pid()}else{pid};
 let head=match crate::task::with_process_pid(target,|p|p.robust_list_head){Some(v)=>v,None=>return errno::neg(errno::ESRCH)};
 if headptr!=0{let r=copy_obj(headptr,&head);if r!=0{return r}}
 if lenptr!=0{let r=copy_obj(lenptr,&24u64);if r!=0{return r}}
 0
}

fn sys_timerfd_create(clock:i32,flags:u64)->i64{
 if clock!=0 && clock!=1{return errno::neg(errno::EINVAL)}
 if flags&!3!=0{return errno::neg(errno::EINVAL)}
 let first=crate::arch::x86_64::apic::ticks().saturating_add(crate::time::ns_to_ticks_ceil(1_000_000_000));
 let id=match crate::fd::alloc_file(crate::fd::OpenKind::TimerFd(first,0)){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 match crate::task::with_current_process(|p|p.fds.install(id)){Some(Ok(fd))=>fd as i64,Some(Err(e))=>{crate::fd::release(id);errno::neg(e)},None=>{crate::fd::release(id);errno::neg(errno::ESRCH)}}
}
fn sys_eventfd2(init:u64,flags:u64)->i64{
 if flags&!0x80003!=0{return errno::neg(errno::EINVAL)}
 let id=match crate::fd::alloc_file(crate::fd::OpenKind::EventFd(init, (flags&1) as u8)){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 // install
 match crate::task::with_current_process(|p|p.fds.install(id)){Some(Ok(fd))=>fd as i64,Some(Err(e))=>{crate::fd::release(id);errno::neg(e)},None=>{crate::fd::release(id);errno::neg(errno::ESRCH)}}
}
fn sys_time(ptr:u64)->i64{
 let sec=(crate::time::monotonic_ns()/1_000_000_000) as i64;
 if ptr!=0{let _=copy_obj(ptr,&sec);}
 sec
}
fn sys_tgkill(tgid:u32,pid:u32,sig:i32)->i64{
 if sig==0{return 0}
 match crate::task::signal_task(tgid,pid,sig){Ok(())=>{
  /* A fatal signal aimed at the caller ends the caller before the syscall
     returns, exactly like Linux. A signal with a handler is delivered
     normally, so the check only applies to the default disposition. */
  let blocked=crate::task::with_current_process(|p|p.sigmask&crate::signal::bit(sig)!=0).unwrap_or(false);
  if pid==crate::task::current_pid()&&!blocked&&crate::signal::default_terminates(sig)&&crate::signal::action_get(sig).map(|a|a.handler==crate::signal::SIG_DFL).unwrap_or(true){crate::task::exit_signal(sig);}
  0
 } Err(e)=>errno::neg(e)}
}

#[repr(C)] struct StatFs{f_type:i64,f_bsize:i64,f_blocks:u64,f_bfree:u64,f_bavail:u64,f_files:u64,f_ffree:u64,f_fsid:i64,f_namelen:i64,f_frsize:i64,f_flags:i64,f_spare:[i64;4]}
fn sys_statfs(path_ptr:u64,buf:u64)->i64{
 let (out,n)=match resolve_path_ptr(path_ptr){Ok(v)=>v,Err(e)=>return e};
 let _=core::str::from_utf8(&out[..n]).ok();
 let st=StatFs{f_type:0x01021997,f_bsize:4096,f_blocks:4096,f_bfree:4096,f_bavail:4096,f_files:32 as u64,f_ffree:(32 as u64).saturating_sub(8),f_fsid:0,f_namelen:255,f_frsize:4096,f_flags:0,f_spare:[0;4]};
 copy_obj(buf,&st)
}
fn sys_fstatfs(fd:usize,buf:u64)->i64{
 let _=fd;
 let st=StatFs{f_type:0x01021997,f_bsize:4096,f_blocks:4096,f_bfree:4096,f_bavail:4096,f_files:32 as u64,f_ffree:(32 as u64).saturating_sub(8),f_fsid:0,f_namelen:255,f_frsize:4096,f_flags:0,f_spare:[0;4]};
 copy_obj(buf,&st)
}
fn sys_symlink(target_ptr:u64,path_ptr:u64)->i64{
 let mut tb=[0u8;128];let mut pb=[0u8;128];
 let t=match copy_path(target_ptr,&mut tb){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let (out,n)=match resolve_path_ptr_nofollow(path_ptr){Ok(v)=>v,Err(e)=>return e};
 let p=match core::str::from_utf8(&out[..n]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
 match crate::vfs::symlink(t,p){Ok(_)=>0,Err(e)=>errno::neg(e)}
}
fn sys_link(old_ptr:u64,new_ptr:u64)->i64{
 let (o,on)=match resolve_path_ptr_nofollow(old_ptr){Ok(v)=>v,Err(e)=>return e};
 let (nw,nn)=match resolve_path_ptr_nofollow(new_ptr){Ok(v)=>v,Err(e)=>return e};
 let old=match core::str::from_utf8(&o[..on]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
 let newp=match core::str::from_utf8(&nw[..nn]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
 match crate::vfs::link(old,newp){Ok(_)=>0,Err(e)=>errno::neg(e)}
}
fn sys_readlink(path_ptr:u64,buf:u64,bufsiz:u64)->i64{
 let (out,n)=match resolve_path_ptr_nofollow(path_ptr){Ok(v)=>v,Err(e)=>return e};
 let p=match core::str::from_utf8(&out[..n]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
 let node=match crate::vfs::lookup(p){Ok(n)=>n,Err(e)=>return errno::neg(e)};
 if node.mode & 0o170000 != 0o120000 {return errno::neg(errno::EINVAL)}
 let m=core::cmp::min(node.size as usize,bufsiz as usize);
 if let Err(e)=crate::usercopy::copy_to_user(buf as usize,unsafe{core::slice::from_raw_parts(node.data,m)}){return errno::neg(e)}
 return m as i64;
}
fn sys_close_range(first:usize,last:usize,flags:u64)->i64{
 if flags!=0{return errno::neg(errno::EINVAL)}
 let mut fd=first;
 while fd<=last && fd<32{
  let _=crate::task::with_current_process(|p|p.fds.close(fd));
  fd+=1;
 }
 0
}
fn sys_mincore(addr:u64,len:u64,vec:u64)->i64{
 if vec==0{return errno::neg(errno::EFAULT)}
 if len==0{return 0}
 if addr&4095!=0{return errno::neg(errno::EINVAL)}
 let end=match addr.checked_add(len){Some(v)=>v,None=>return errno::neg(errno::ENOMEM)};
 let mapped=crate::task::with_current_process(|p|{
  let mut va=addr;
  while va<end{
   let in_stack=va>=p.stack_base&&va<p.stack_top;
   let in_seg=p.segs[..p.nsegs].iter().any(|s|va>=s.start&&va<s.end);
   if !in_stack&&!in_seg&&p.vm.find_vma(va as usize).is_none(){return false}
   va+=4096;
  }
  true
 }).unwrap_or(false);
 if !mapped{return errno::neg(errno::ENOMEM)}
 let pages=((len as usize)+4095)/4096;
 let mut done=0usize;let mut buf=[0u8;64];
 while done<pages{
  let n=core::cmp::min(buf.len(),pages-done);
  let present=crate::task::with_current_process(|p|{
   let root=p.root();let phys=crate::arch::x86_64::phys::Identity;let mut v=[0u8;64];
   for i in 0..n{
    let resident=root.query(&phys,addr+((done+i)*4096)as u64).map(|(_,f)|f&crate::arch::x86_64::paging::PRESENT!=0).unwrap_or(false);
    v[i]=if resident{1}else{0};
   }
   v
  }).unwrap_or([0u8;64]);
  if let Err(e)=crate::usercopy::copy_to_user(vec as usize+done,&present[..n]){return errno::neg(e)}
  done+=n;
 }
 0
}

fn sys_fallocate(fd:usize,mode:u64,offset:u64,len:u64)->i64{
 if mode!=0{return errno::neg(errno::EOPNOTSUPP)}
 let id=match fd_id(fd as u64){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 match crate::fd::kind(id){
  Some(crate::fd::OpenKind::Inode(n))=>{
   let end=match offset.checked_add(len){Some(v)=>v,None=>return errno::neg(errno::EINVAL)};
   if end>n.capacity{return errno::neg(errno::ENOSPC)}
   if end>n.size{
    let _=crate::vfs::truncate_ino(n.ino,end);
   }
   0
  }
  _=>errno::neg(errno::ENODEV)
 }
}
fn sys_sendfile(out_fd:usize,in_fd:usize,off:u64,count:usize)->i64{
 if count==0{return 0}
 let in_id=match fd_id(in_fd as u64){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 let out_id=match fd_id(out_fd as u64){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 let mut buf=[0u8;512];
 let mut done=0usize;
 while done<count{
  let n=core::cmp::min(buf.len(),count-done);
  let got=match crate::fd::read(in_id,&mut buf[..n]){Ok(g)=>g,Err(e)=>return if done==0{errno::neg(e)}else{done as i64}};
  if got==0{break}
  match crate::fd::write(out_id,&buf[..got]){Ok(m)=>done+=m,Err(e)=>return if done==0{errno::neg(e)}else{done as i64}}
 }
 done as i64
}

static mut EPOLL_COUNT:[u8;64]=[0;64];
static mut EPOLL_FDS:[[u32;16];64]=[[0;16];64];
static mut EPOLL_EVENTS:[[u32;16];64]=[[0;16];64];
static mut EPOLL_DATA:[[u64;16];64]=[[0;16];64];

fn epoll_find(epfd:usize,fd:usize)->Option<usize>{
 if epfd>=64{return None}
 let c=unsafe{EPOLL_COUNT[epfd]} as usize;
 for i in 0..c{ if unsafe{EPOLL_FDS[epfd][i]} as usize==fd {return Some(i)} }
 None
}
fn sys_epoll_ctl(epfd:usize,op:i32,fd:usize,event_ptr:u64)->i64{
 let id=match fd_id(epfd as u64){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 if !matches!(crate::fd::kind(id),Some(crate::fd::OpenKind::Epoll)){return errno::neg(errno::EINVAL)}
 if fd>=32{return errno::neg(errno::EBADF)}
 match op{
  1=>{ // EPOLL_CTL_ADD
   if epoll_find(epfd,fd).is_some(){return errno::neg(errno::EEXIST)}
   let mut ev=[0u8;12];
   if event_ptr==0{return errno::neg(errno::EFAULT)}
   if let Err(e)=crate::usercopy::copy_from_user(&mut ev,event_ptr as usize){return errno::neg(e)}
   let events=u32::from_ne_bytes(ev[0..4].try_into().unwrap());
   let data=u64::from_ne_bytes(ev[4..12].try_into().unwrap());
   let c=unsafe{EPOLL_COUNT[epfd]} as usize;
   if c>=16{return errno::neg(errno::ENOMEM)}
   unsafe{EPOLL_FDS[epfd][c]=fd as u32;EPOLL_EVENTS[epfd][c]=events;EPOLL_DATA[epfd][c]=data;EPOLL_COUNT[epfd]+=1;}
   0
  }
  2=>{ // EPOLL_CTL_DEL
   if let Some(i)=epoll_find(epfd,fd){
    let c=unsafe{EPOLL_COUNT[epfd]} as usize;
    for j in i..c.saturating_sub(1){
     unsafe{EPOLL_FDS[epfd][j]=EPOLL_FDS[epfd][j+1];EPOLL_EVENTS[epfd][j]=EPOLL_EVENTS[epfd][j+1];EPOLL_DATA[epfd][j]=EPOLL_DATA[epfd][j+1];}
    }
    unsafe{EPOLL_COUNT[epfd]-=1;}
    0
   }else{errno::neg(errno::ENOENT)}
  }
  3=>{ // EPOLL_CTL_MOD
   if let Some(i)=epoll_find(epfd,fd){
    let mut ev=[0u8;12];
    if event_ptr==0{return errno::neg(errno::EFAULT)}
    if let Err(e)=crate::usercopy::copy_from_user(&mut ev,event_ptr as usize){return errno::neg(e)}
    let events=u32::from_ne_bytes(ev[0..4].try_into().unwrap());
    let data=u64::from_ne_bytes(ev[4..12].try_into().unwrap());
    unsafe{EPOLL_EVENTS[epfd][i]=events;EPOLL_DATA[epfd][i]=data;}
    0
   }else{errno::neg(errno::ENOENT)}
  }
  _=>errno::neg(errno::EINVAL)
 }
}
fn epoll_ready(epfd:usize,events:u32,fd:usize)->bool{
 let id=match crate::fd::kind(fd as u32){Some(x)=>x,_=>return false};
 let readable=match id{
  crate::fd::OpenKind::Console=>true,
  crate::fd::OpenKind::PipeRead(p)=>crate::pipe::has_data(p),
  crate::fd::OpenKind::Inode(_)=>true,
  crate::fd::OpenKind::Socket(_)=>true,
  crate::fd::OpenKind::CharDevice(_)=>true,
  crate::fd::OpenKind::EventFd(c,_)=>c!=0,
  crate::fd::OpenKind::TimerFd(_,_)=>true,
  _=>false,
 };
 let writable=match id{
  crate::fd::OpenKind::Console=>true,
  crate::fd::OpenKind::PipeWrite(p)=>crate::pipe::has_space(p),
  crate::fd::OpenKind::Socket(_)=>true,
  crate::fd::OpenKind::CharDevice(_)=>true,
  crate::fd::OpenKind::EventFd(_,_)=>true,
  _=>false,
 };
 let mut ready=false;
 if events&0x1!=0 && readable{ready=true;}
 if events&0x4!=0 && writable{ready=true;}
 ready
}
fn sys_epoll_wait(epfd:usize,events_ptr:u64,maxevents:i32,timeout:i32)->i64{
 if maxevents<=0{return errno::neg(errno::EINVAL)}
 let id=match fd_id(epfd as u64){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 if !matches!(crate::fd::kind(id),Some(crate::fd::OpenKind::Epoll)){return errno::neg(errno::EINVAL)}
 let c=unsafe{EPOLL_COUNT[epfd]} as usize;
 let mut ready_count=0i64;
 let mut retries=0u32;
 loop{
  for i in 0..c{
   let fd=unsafe{EPOLL_FDS[epfd][i]} as usize;
   let evmask=unsafe{EPOLL_EVENTS[epfd][i]};
   if !epoll_ready(epfd,evmask,fd){continue}
   let mut rev=0u32;
   if evmask&0x1!=0 && epoll_ready(epfd,0x1,fd){rev|=0x1}
   if evmask&0x4!=0 && epoll_ready(epfd,0x4,fd){rev|=0x4}
   if rev==0{rev=evmask&0x3;}
   let mut buf=[0u8;12];
   buf[0..4].copy_from_slice(&rev.to_ne_bytes());
   buf[4..12].copy_from_slice(&unsafe{EPOLL_DATA[epfd][i]}.to_ne_bytes());
   let off=(ready_count as usize)*12;
   if let Err(e)=crate::usercopy::copy_to_user(events_ptr as usize+off,&buf){return errno::neg(e)}
   ready_count+=1;
   if ready_count>=maxevents as i64{break}
  }
  if ready_count>0{return ready_count}
  if timeout==0{return 0}
  if timeout>0{
   if retries as i32 >= timeout {return 0} // coarse: 1ms per retry
   crate::task::sleep_until(crate::time::ticks_after_ns(1_000_000));
   retries+=1;
  } else {
   crate::task::sleep_until(crate::time::ticks_after_ns(1_000_000));
  }
 }
}

const CAP_SETGID:u32=6;const CAP_SETUID:u32=7;const CAP_SYS_CHROOT:u32=18;const CAP_SYS_ADMIN:u32=21;
fn current_has_cap(cap:u32)->bool{crate::task::with_current_process(|p|p.cap_eff&(1u64<<cap)!=0).unwrap_or(false)}
fn current_has_initial_cap(cap:u32)->bool{crate::task::with_current_process(|p|p.ns_user==1&&p.cap_eff&(1u64<<cap)!=0).unwrap_or(false)}
fn current_has_mount_cap()->bool{crate::task::with_current_process(|p|p.cap_eff&(1u64<<CAP_SYS_ADMIN)!=0&&(p.ns_user==1||p.ns_mnt!=1)).unwrap_or(false)}
fn current_has_uts_cap()->bool{crate::task::with_current_process(|p|p.cap_eff&(1u64<<CAP_SYS_ADMIN)!=0&&(p.ns_user==1||p.ns_uts!=1)).unwrap_or(false)}
fn sys_getuid()->i64{crate::task::with_current_process(|p|p.uid).unwrap_or(0) as i64}
fn sys_getgid()->i64{crate::task::with_current_process(|p|p.gid).unwrap_or(0) as i64}
fn sys_geteuid()->i64{crate::task::with_current_process(|p|p.euid).unwrap_or(0) as i64}
fn sys_getegid()->i64{crate::task::with_current_process(|p|p.egid).unwrap_or(0) as i64}
fn sys_setuid(uid:u32)->i64{
  let r=crate::task::with_current_process(|p|{
   if p.cap_eff&(1u64<<CAP_SETUID)!=0{p.uid=uid;p.suid=uid;p.euid=uid;p.fsuid=uid;if uid!=0{p.cap_eff=0;p.cap_prm=0;}}
   else if uid==p.uid||uid==p.suid{let old_euid=p.euid;p.euid=uid;p.fsuid=uid;if old_euid==0&&uid!=0{p.cap_eff=0;p.cap_prm=0;}}
   else{return Err(crate::errno::EPERM)}
  Ok(())
 });
 match r{Some(Ok(()))=>0,Some(Err(e))=>errno::neg(e),None=>errno::neg(errno::ESRCH)}
}
fn sys_setgid(gid:u32)->i64{
  let r=crate::task::with_current_process(|p|{
   if p.cap_eff&(1u64<<CAP_SETGID)!=0{p.gid=gid;p.sgid=gid;p.egid=gid;p.fsgid=gid;}
  else if gid==p.gid||gid==p.sgid{p.egid=gid;p.fsgid=gid;}
  else{return Err(crate::errno::EPERM)}
  Ok(())
 });
 match r{Some(Ok(()))=>0,Some(Err(e))=>errno::neg(e),None=>errno::neg(errno::ESRCH)}
}
fn sys_setreuid(ruid:u32,euid:u32)->i64{
  let r=crate::task::with_current_process(|p|{
   let privileged=p.cap_eff&(1u64<<CAP_SETUID)!=0;let old_euid=p.euid;
   if ruid!=u32::MAX{
    if !privileged&&ruid!=p.uid&&ruid!=p.suid{return Err(crate::errno::EPERM)}
   }
   if euid!=u32::MAX{
    if !privileged&&euid!=p.uid&&euid!=p.suid&&euid!=p.euid{return Err(crate::errno::EPERM)}
   }
   if ruid!=u32::MAX{p.uid=ruid;}
   if euid!=u32::MAX{p.euid=euid;p.fsuid=euid;}
   if ruid!=u32::MAX||euid!=u32::MAX{p.suid=p.euid;}
   if old_euid==0&&p.euid!=0{p.cap_eff=0;p.cap_prm=0;}
   Ok(())
 });
 match r{Some(Ok(()))=>0,Some(Err(e))=>errno::neg(e),None=>errno::neg(errno::ESRCH)}
}
fn sys_setregid(rgid:u32, egid:u32)->i64{
  let r=crate::task::with_current_process(|p|{
   let privileged=p.cap_eff&(1u64<<CAP_SETGID)!=0;
   if rgid!=u32::MAX{
    if !privileged&&rgid!=p.gid&&rgid!=p.sgid{return Err(crate::errno::EPERM)}
   }
   if egid!=u32::MAX{
    if !privileged&&egid!=p.gid&&egid!=p.sgid&&egid!=p.egid{return Err(crate::errno::EPERM)}
   }
   if rgid!=u32::MAX{p.gid=rgid;}
   if egid!=u32::MAX{p.egid=egid;p.fsgid=egid;}
   if rgid!=u32::MAX||egid!=u32::MAX{p.sgid=p.egid;}
  Ok(())
 });
 match r{Some(Ok(()))=>0,Some(Err(e))=>errno::neg(e),None=>errno::neg(errno::ESRCH)}
}
fn sys_getgroups(size:i32,ptr:u64)->i64{
 let ng=crate::task::with_current_process(|p|p.ngroups).unwrap_or(0);
 if size==0{return ng as i64}
 if size<ng as i32{return errno::neg(errno::EINVAL)}
 let groups=crate::task::with_current_process(|p|p.groups).unwrap_or([0;32]);
 if ptr!=0{
  let mut buf=[0u8;32*4];
  for i in 0..ng{buf[i*4..i*4+4].copy_from_slice(&groups[i].to_ne_bytes());}
  if let Err(e)=crate::usercopy::copy_to_user(ptr as usize,&buf[..ng*4]){return errno::neg(e)}
 }
 ng as i64
}
fn sys_setgroups(size:usize,ptr:u64)->i64{
  let r=crate::task::with_current_process(|p|{
   if p.cap_eff&(1u64<<CAP_SETGID)==0{return Err(crate::errno::EPERM)}
  if size>32{return Err(crate::errno::EINVAL)}
  let mut groups=[0u32;32];
  if size>0{
   let mut buf=[0u8;32*4];
   if let Err(e)=crate::usercopy::copy_from_user(&mut buf,ptr as usize){return Err(e)}
   for i in 0..size{groups[i]=u32::from_ne_bytes(buf[i*4..i*4+4].try_into().unwrap());}
  }
  p.groups=groups;p.ngroups=size;
  Ok(())
 });
 match r{Some(Ok(()))=>0,Some(Err(e))=>errno::neg(e),None=>errno::neg(errno::ESRCH)}
}
fn sys_setresuid(ruid:u32,euid:u32,suid:u32)->i64{
  let r=crate::task::with_current_process(|p|{
   let privileged=p.cap_eff&(1u64<<CAP_SETUID)!=0;let old_euid=p.euid;
   if ruid!=u32::MAX && !(privileged||ruid==p.uid||ruid==p.euid||ruid==p.suid){return Err(crate::errno::EPERM)}
   if euid!=u32::MAX && !(privileged||euid==p.uid||euid==p.euid||euid==p.suid){return Err(crate::errno::EPERM)}
   if suid!=u32::MAX && !(privileged||suid==p.uid||suid==p.euid||suid==p.suid){return Err(crate::errno::EPERM)}
   if ruid!=u32::MAX{p.uid=ruid;}
   if euid!=u32::MAX{p.euid=euid;p.fsuid=euid;}
   if suid!=u32::MAX{p.suid=suid;}
   if old_euid==0&&p.euid!=0{p.cap_eff=0;p.cap_prm=0;}
  Ok(())
 });
 match r{Some(Ok(()))=>0,Some(Err(e))=>errno::neg(e),None=>errno::neg(errno::ESRCH)}
}
fn sys_getresuid(ruid:u64,euid:u64,suid:u64)->i64{
 if let Some(p)=crate::task::with_current_process(|p|(p.uid,p.euid,p.suid)){
  if ruid!=0{let _=crate::usercopy::copy_to_user(ruid as usize,&p.0.to_ne_bytes());}
  if euid!=0{let _=crate::usercopy::copy_to_user(euid as usize,&p.1.to_ne_bytes());}
  if suid!=0{let _=crate::usercopy::copy_to_user(suid as usize,&p.2.to_ne_bytes());}
  0
 }else{errno::neg(errno::ESRCH)}
}
fn sys_setresgid(rgid:u32,egid:u32,sgid:u32)->i64{
  let r=crate::task::with_current_process(|p|{
   let privileged=p.cap_eff&(1u64<<CAP_SETGID)!=0;
   if rgid!=u32::MAX && !(privileged||rgid==p.gid||rgid==p.egid||rgid==p.sgid){return Err(crate::errno::EPERM)}
   if egid!=u32::MAX && !(privileged||egid==p.gid||egid==p.egid||egid==p.sgid){return Err(crate::errno::EPERM)}
   if sgid!=u32::MAX && !(privileged||sgid==p.gid||sgid==p.egid||sgid==p.sgid){return Err(crate::errno::EPERM)}
  if rgid!=u32::MAX{p.gid=rgid;}
  if egid!=u32::MAX{p.egid=egid;p.fsgid=egid;}
  if sgid!=u32::MAX{p.sgid=sgid;}
  Ok(())
 });
 match r{Some(Ok(()))=>0,Some(Err(e))=>errno::neg(e),None=>errno::neg(errno::ESRCH)}
}
fn sys_getresgid(rgid:u64,egid:u64,sgid:u64)->i64{
 if let Some(p)=crate::task::with_current_process(|p|(p.gid,p.egid,p.sgid)){
  if rgid!=0{let _=crate::usercopy::copy_to_user(rgid as usize,&p.0.to_ne_bytes());}
  if egid!=0{let _=crate::usercopy::copy_to_user(egid as usize,&p.1.to_ne_bytes());}
  if sgid!=0{let _=crate::usercopy::copy_to_user(sgid as usize,&p.2.to_ne_bytes());}
  0
 }else{errno::neg(errno::ESRCH)}
}
fn sys_umask(mask:u32)->i64{
 let old=crate::task::with_current_process(|p|{let o=p.umask;p.umask=mask&0o777;o}).unwrap_or(0);
 old as i64
}
fn sys_unshare(flags:u64)->i64{
 const CLONE_NEWUTS:u64=0x04000000; const CLONE_NEWIPC:u64=0x08000000; const CLONE_NEWUSER:u64=0x10000000; const CLONE_NEWPID:u64=0x20000000; const CLONE_NEWNET:u64=0x40000000; const CLONE_NEWCGROUP:u64=0x02000000; const CLONE_NEWTIME:u64=0x00000080; const CLONE_NEWNS:u64=0x00020000;
 if flags & !(CLONE_NEWUTS|CLONE_NEWIPC|CLONE_NEWUSER|CLONE_NEWPID|CLONE_NEWNET|CLONE_NEWCGROUP|CLONE_NEWTIME|CLONE_NEWNS)!=0{return errno::neg(errno::EINVAL)}
 let privileged_flags=CLONE_NEWUTS|CLONE_NEWIPC|CLONE_NEWPID|CLONE_NEWNET|CLONE_NEWCGROUP|CLONE_NEWTIME|CLONE_NEWNS;
 if flags&privileged_flags!=0&&flags&CLONE_NEWUSER==0&&!current_has_cap(CAP_SYS_ADMIN){return errno::neg(errno::EPERM)}
 let r=crate::task::with_current_process(|p|{
  if flags&CLONE_NEWUTS!=0{p.ns_uts=crate::task::alloc_ns();}
  if flags&CLONE_NEWIPC!=0{p.ns_ipc=crate::task::alloc_ns();}
   if flags&CLONE_NEWNS!=0{let ns=crate::task::alloc_ns();crate::vfs::clone_mount_namespace(p.ns_mnt,ns)?;p.ns_mnt=ns;}
  if flags&CLONE_NEWPID!=0{p.ns_pid=crate::task::alloc_ns();}
  if flags&CLONE_NEWNET!=0{p.ns_net=crate::task::alloc_ns();}
  if flags&CLONE_NEWUSER!=0{p.ns_user=crate::task::alloc_ns();p.uid=0;p.gid=0;p.euid=0;p.egid=0;p.suid=0;p.sgid=0;p.cap_eff=u64::MAX;p.cap_prm=u64::MAX;}
  if flags&CLONE_NEWCGROUP!=0{p.ns_cgroup=crate::task::alloc_ns();}
  if flags&CLONE_NEWTIME!=0{p.ns_time=crate::task::alloc_ns();}
  Ok(())
 });
 match r{Some(Ok(()))=>0,Some(Err(e))=>errno::neg(e),None=>errno::neg(errno::ESRCH)}
}
fn sys_setns(fd:i32,nstype:i32)->i64{
 let _=(fd,nstype);
 0
}
fn sys_seccomp(op:u32,flags:u32,uargs:u64)->i64{
  match op{
   0=>{ // SECCOMP_SET_MODE_STRICT
    if flags!=0{return errno::neg(errno::EINVAL)}
    match crate::task::with_current_process(|p|{if p.seccomp_mode!=0{return Err(errno::EINVAL)}p.seccomp_mode=1;Ok(())}){Some(Ok(()))=>0,Some(Err(e))=>errno::neg(e),None=>errno::neg(errno::ESRCH)}
   }
   1=>{ // SECCOMP_SET_MODE_FILTER
    if flags!=0{return errno::neg(errno::EOPNOTSUPP)}
    let permitted=crate::task::with_current_process(|p|p.no_new_privs||p.cap_eff&(1<<21)!=0).unwrap_or(false);
    if !permitted{return errno::neg(errno::EPERM)}
    if crate::task::with_current_process(|p|p.seccomp_mode==1||p.seccomp_filter!=0).unwrap_or(true){return errno::neg(errno::EOPNOTSUPP)}
    if uargs==0{return errno::neg(errno::EFAULT)}
   let mut hdr=[0u8;16];
   if let Err(e)=crate::usercopy::copy_from_user(&mut hdr,uargs as usize){return errno::neg(e)}
   let len=u16::from_ne_bytes(hdr[0..2].try_into().unwrap()) as usize;
   let fptr=u64::from_ne_bytes(hdr[8..16].try_into().unwrap());
   if len==0 || len>512 || fptr==0{return errno::neg(errno::EINVAL)}
   let frame=match crate::memory::alloc_frame(){Some(f)=>f,None=>return errno::neg(errno::ENOMEM)};
   let size=len*8;
   let mut buf=[0u8;4096];
   if let Err(e)=crate::usercopy::copy_from_user(&mut buf[..size],fptr as usize){let _=crate::memory::free_frame(frame);return errno::neg(e)}
   unsafe{core::slice::from_raw_parts_mut(crate::arch::x86_64::phys::phys_to_virt(frame)as *mut u8,size).copy_from_slice(&buf[..size]);}
     let filter=crate::arch::x86_64::phys::phys_to_virt(frame)as *const SockFilter;
     if !seccomp_validate(filter,len){let _=crate::memory::free_frame(frame);return errno::neg(errno::EINVAL)}
     if let Err(e)=crate::process::seccomp_filter_register(frame){let _=crate::memory::free_frame(frame);return errno::neg(e)}
    let r=crate::task::with_current_process(|p|{
     p.seccomp_filter=frame as usize;
     p.seccomp_filter_len=len as u16;
     p.seccomp_mode=2;
   });
     if r.is_none(){let _=crate::process::seccomp_filter_release(frame);return errno::neg(errno::ESRCH)}
    0
   }
   2=>{
    if flags!=0{return errno::neg(errno::EINVAL)}if uargs==0{return errno::neg(errno::EFAULT)}
    let mut action=[0u8;4];if let Err(e)=crate::usercopy::copy_from_user(&mut action,uargs as usize){return errno::neg(e)}
    let available=matches!(u32::from_ne_bytes(action)&0xffff0000,0x80000000|0x00000000|0x00030000|0x00050000|0x7ffc0000|0x7fff0000);
    if available{0}else{errno::neg(errno::EOPNOTSUPP)}
   }
   _=>errno::neg(errno::EINVAL),
  }
}
fn sys_inotify_init()->i64{sys_inotify_init1(0)}
fn sys_inotify_init1(flags:u64)->i64{
  if flags&!(crate::inotify::IN_NONBLOCK|crate::inotify::IN_CLOEXEC)!=0{return errno::neg(errno::EINVAL)}
  let _=flags;
 let id=match crate::fd::alloc_file(crate::fd::OpenKind::Inotify){Ok(d)=>d,Err(e)=>return errno::neg(e)};
 match crate::task::with_current_process(|p|p.fds.install(id)){Some(Ok(fd))=>fd as i64,Some(Err(e))=>{crate::fd::release(id);errno::neg(e)},None=>{crate::fd::release(id);errno::neg(errno::ESRCH)}}
}
fn sys_inotify_add_watch(fd:usize,path:u64,mask:u64)->i64{
  let mask=mask as u32;
  let (out,n)=match if mask&crate::inotify::IN_DONT_FOLLOW!=0{resolve_path_ptr_nofollow(path)}else{resolve_path_ptr(path)}{Ok(v)=>v,Err(e)=>return e};
  let p=match core::str::from_utf8(&out[..n]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
  let fdid=match crate::task::with_current_process(|pr|pr.fds.get(fd)).flatten(){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
  if !matches!(crate::fd::kind(fdid),Some(crate::fd::OpenKind::Inotify)){return errno::neg(errno::EINVAL)}
  let inode=match crate::vfs::lookup(p){Ok(n)=>n,Err(e)=>return errno::neg(e)};
  if mask&crate::inotify::IN_ONLYDIR!=0&&inode.mode&0o170000!=crate::vfs::S_IFDIR{return errno::neg(errno::ENOTDIR)}
  match crate::inotify::add_watch(fdid,p,mask){Ok(wd)=>wd as i64,Err(e)=>errno::neg(e)}
}
fn sys_inotify_rm_watch(fd:usize,wd:i32)->i64{
  let fdid=match crate::task::with_current_process(|pr|pr.fds.get(fd)).flatten(){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
  if !matches!(crate::fd::kind(fdid),Some(crate::fd::OpenKind::Inotify)){return errno::neg(errno::EINVAL)}
  match crate::inotify::rm_watch(fdid,wd){Ok(())=>0,Err(e)=>errno::neg(e)}
}
fn sys_signalfd4(fd:i32,mask:u64,flags:u64,sizemask:u64)->i64{
 let _=(fd,mask,flags,sizemask);
 if fd!=-1{return fd as i64}
 let id=match crate::fd::alloc_file(crate::fd::OpenKind::SignalFd(mask)){Ok(d)=>d,Err(e)=>return errno::neg(e)};
 match crate::task::with_current_process(|p|p.fds.install(id)){Some(Ok(newfd))=>newfd as i64,Some(Err(e))=>{crate::fd::release(id);errno::neg(e)},None=>{crate::fd::release(id);errno::neg(errno::ESRCH)}}
}

fn sys_mount(source:u64,target:u64,fstype:u64,flags:u64,data:u64)->i64{
  let _=data;
   if !current_has_mount_cap(){return errno::neg(errno::EPERM)}
  let mut sb=[0u8;128];let mut fb=[0u8;32];
  let sp=if source==0{""}else{match copy_path(source,&mut sb){Ok(v)=>v,Err(e)=>return errno::neg(e)}};
  if fstype==0{return errno::neg(errno::EINVAL)}
  let fs=match copy_path(fstype,&mut fb){Ok(v)=>v,Err(e)=>return errno::neg(e)};
  let(out,n)=match resolve_path_ptr(target){Ok(v)=>v,Err(e)=>return e};
  let tp=match core::str::from_utf8(&out[..n]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
   let ns=crate::task::with_current_process(|p|p.ns_mnt).unwrap_or(1);
   if fs=="ext2"&&!current_has_initial_cap(CAP_SYS_ADMIN){return errno::neg(errno::EPERM)}
  match crate::vfs::mount_filesystem(sp,tp,fs,flags,ns){Ok(())=>0,Err(e)=>errno::neg(e)}
}
fn sys_umount2(target:u64,flags:u64)->i64{
   if !current_has_mount_cap(){return errno::neg(errno::EPERM)}
  let(out,n)=match resolve_path_ptr(target){Ok(v)=>v,Err(e)=>return e};
  let tp=match core::str::from_utf8(&out[..n]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
  let ns=crate::task::with_current_process(|p|p.ns_mnt).unwrap_or(1);
  match crate::vfs::unmount_filesystem(tp,flags,ns){Ok(())=>0,Err(e)=>errno::neg(e)}
}
fn sys_pivot_root(new_root:u64,put_old:u64)->i64{
  if !current_has_cap(CAP_SYS_ADMIN){return errno::neg(errno::EPERM)}
 let (out,n)=match resolve_path_ptr(new_root){Ok(v)=>v,Err(e)=>return e};
 let p=match core::str::from_utf8(&out[..n]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
 if !crate::vfs::is_dir(p){return errno::neg(errno::ENOTDIR)}
 let _=crate::task::with_current_process(|pr|{pr.set_root(p);});
 let _=put_old;
 0
}
fn sys_chroot(path:u64)->i64{
  if !current_has_cap(CAP_SYS_CHROOT){return errno::neg(errno::EPERM)}
  let (out,n)=match resolve_path_ptr(path){Ok(v)=>v,Err(e)=>return e};
 let p=match core::str::from_utf8(&out[..n]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
 if !crate::vfs::is_dir(p){return errno::neg(errno::ENOTDIR)}
 let _=crate::task::with_current_process(|pr|{pr.set_root(p);});
 0
}
fn sys_sethostname(name:u64,len:u64)->i64{
  if !current_has_uts_cap(){return errno::neg(errno::EPERM)}
 if len>64{return errno::neg(errno::EINVAL)}
 let mut buf=[0u8;64];
 if let Err(e)=crate::usercopy::copy_from_user(&mut buf,name as usize){return errno::neg(e)}
 let ns=crate::task::with_current_process(|p|p.ns_uts).unwrap_or(1);
 crate::task::set_uts_hostname(ns,&buf[..len as usize]);
 0
}
fn sys_setdomainname(name:u64,len:u64)->i64{
  if !current_has_uts_cap(){return errno::neg(errno::EPERM)}
 let _=(name,len);
 0
}

fn sys_capget(header:u64,data:u64)->i64{
  if header==0{return errno::neg(errno::EFAULT)}
  const V1:u32=0x19980330;const V2:u32=0x20071026;const V3:u32=0x20080522;
  let mut hdr=[0u8;8];if let Err(e)=crate::usercopy::copy_from_user(&mut hdr,header as usize){return errno::neg(e)}
  let version=u32::from_ne_bytes(hdr[..4].try_into().unwrap());let pid=i32::from_ne_bytes(hdr[4..8].try_into().unwrap());
  if !matches!(version,V1|V2|V3){let _=crate::usercopy::copy_to_user(header as usize,&V3.to_ne_bytes());return errno::neg(errno::EINVAL)}
  if pid!=0&&pid as u32!=crate::task::current_pid(){return errno::neg(errno::ESRCH)}
  if data!=0{
   let(effective,permitted,inheritable)=crate::task::with_current_process(|p|(p.cap_eff,p.cap_prm,p.cap_inh)).unwrap_or((0,0,0));let words=if version==V1{1}else{2};
   let mut buf=[0u8;24];
   for i in 0..words{let shift=i*32;let off=i*12;buf[off..off+4].copy_from_slice(&((effective>>shift)as u32).to_ne_bytes());buf[off+4..off+8].copy_from_slice(&((permitted>>shift)as u32).to_ne_bytes());buf[off+8..off+12].copy_from_slice(&((inheritable>>shift)as u32).to_ne_bytes());}
   if let Err(e)=crate::usercopy::copy_to_user(data as usize,&buf[..words*12]){return errno::neg(e)}
  }
  0
}
fn sys_capset(header:u64,data:u64)->i64{
  const V1:u32=0x19980330;const V2:u32=0x20071026;const V3:u32=0x20080522;
  if header==0||data==0{return errno::neg(errno::EFAULT)}
  let mut hdr=[0u8;8];if let Err(e)=crate::usercopy::copy_from_user(&mut hdr,header as usize){return errno::neg(e)}
  let version=u32::from_ne_bytes(hdr[..4].try_into().unwrap());let pid=i32::from_ne_bytes(hdr[4..8].try_into().unwrap());
  if !matches!(version,V1|V2|V3){let _=crate::usercopy::copy_to_user(header as usize,&V3.to_ne_bytes());return errno::neg(errno::EINVAL)}
  if pid!=0&&pid as u32!=crate::task::current_pid(){return errno::neg(errno::EPERM)}
  let words=if version==V1{1}else{2};let mut raw=[0u8;24];if let Err(e)=crate::usercopy::copy_from_user(&mut raw[..words*12],data as usize){return errno::neg(e)}
  let mut effective=0u64;let mut permitted=0u64;let mut inheritable=0u64;
  for i in 0..words{let shift=i*32;let off=i*12;effective|=(u32::from_ne_bytes(raw[off..off+4].try_into().unwrap())as u64)<<shift;permitted|=(u32::from_ne_bytes(raw[off+4..off+8].try_into().unwrap())as u64)<<shift;inheritable|=(u32::from_ne_bytes(raw[off+8..off+12].try_into().unwrap())as u64)<<shift;}
  let r=crate::task::with_current_process(|p|{
   if effective&!permitted!=0||permitted&!p.cap_prm!=0||inheritable&!(p.cap_inh|p.cap_prm)!=0{return Err(errno::EPERM)}
   p.cap_eff=effective;p.cap_prm=permitted;p.cap_inh=inheritable;Ok(())
  });
  match r{Some(Ok(()))=>0,Some(Err(e))=>errno::neg(e),None=>errno::neg(errno::ESRCH)}
}

fn sys_io_uring_setup(entries:u32,params:u64)->i64{
  const MAX_ENTRIES:u32=64;const PARAMS_SIZE:usize=120;
  if params==0{return errno::neg(errno::EFAULT)}
  if entries==0||entries>MAX_ENTRIES{return errno::neg(errno::EINVAL)}
  let count=match entries.checked_next_power_of_two(){Some(v)if v<=MAX_ENTRIES=>v,_=>return errno::neg(errno::EINVAL)};
  let mut input=[0u8;PARAMS_SIZE];if let Err(e)=crate::usercopy::copy_from_user(&mut input,params as usize){return errno::neg(e)}
  let setup_flags=u32::from_ne_bytes(input[8..12].try_into().unwrap());
  let thread_cpu=u32::from_ne_bytes(input[12..16].try_into().unwrap());let thread_idle=u32::from_ne_bytes(input[16..20].try_into().unwrap());let wq_fd=u32::from_ne_bytes(input[24..28].try_into().unwrap());
  if setup_flags!=0||thread_cpu!=0||thread_idle!=0||wq_fd!=0||input[28..40].iter().any(|b|*b!=0){return errno::neg(errno::EOPNOTSUPP)}
  let id=match crate::fd::alloc_file(crate::fd::OpenKind::IoUring){Ok(d)=>d,Err(e)=>return errno::neg(e)};
  let mut frames=[0u64;3];
  for i in 0..frames.len(){match crate::memory::alloc_frame(){Some(frame)=>frames[i]=frame,None=>{for frame in frames.iter().copied().filter(|p|*p!=0){let _=crate::memory::free_frame(frame);}crate::fd::release(id);return errno::neg(errno::ENOMEM)}}}
  let sq=unsafe{crate::arch::x86_64::phys::Identity.bytes(frames[0],4096)};
  let cq=unsafe{crate::arch::x86_64::phys::Identity.bytes(frames[1],4096)};
  let sqes=unsafe{crate::arch::x86_64::phys::Identity.bytes(frames[2],4096)};
  sq.fill(0);cq.fill(0);sqes.fill(0);
  sq[8..12].copy_from_slice(&(count-1).to_ne_bytes());sq[12..16].copy_from_slice(&count.to_ne_bytes());
  for i in 0..count as usize{let off=24+i*4;sq[off..off+4].copy_from_slice(&(i as u32).to_ne_bytes());}
  cq[8..12].copy_from_slice(&(count-1).to_ne_bytes());cq[12..16].copy_from_slice(&count.to_ne_bytes());
  crate::fd::iouring_set(id,frames[0],frames[1],frames[2],count);
  let mut output=[0u8;PARAMS_SIZE];
  output[0..4].copy_from_slice(&count.to_ne_bytes());output[4..8].copy_from_slice(&count.to_ne_bytes());
  // SQ offsets: head, tail, mask, entries, flags, dropped, array.
  output[40..44].copy_from_slice(&0u32.to_ne_bytes());output[44..48].copy_from_slice(&4u32.to_ne_bytes());output[48..52].copy_from_slice(&8u32.to_ne_bytes());output[52..56].copy_from_slice(&12u32.to_ne_bytes());output[56..60].copy_from_slice(&16u32.to_ne_bytes());output[60..64].copy_from_slice(&20u32.to_ne_bytes());output[64..68].copy_from_slice(&24u32.to_ne_bytes());
  // CQ offsets: head, tail, mask, entries, overflow, CQEs.
  output[80..84].copy_from_slice(&0u32.to_ne_bytes());output[84..88].copy_from_slice(&4u32.to_ne_bytes());output[88..92].copy_from_slice(&8u32.to_ne_bytes());output[92..96].copy_from_slice(&12u32.to_ne_bytes());output[96..100].copy_from_slice(&16u32.to_ne_bytes());output[100..104].copy_from_slice(&20u32.to_ne_bytes());
  if let Err(e)=crate::usercopy::copy_to_user(params as usize,&output){crate::fd::iouring_clear(id);for frame in frames{let _=crate::memory::free_frame(frame);}crate::fd::release(id);return errno::neg(e)}
  match crate::task::with_current_process(|p|p.fds.install(id)){Some(Ok(fd))=>fd as i64,Some(Err(e))=>{crate::fd::iouring_clear(id);for frame in frames{let _=crate::memory::free_frame(frame);}crate::fd::release(id);errno::neg(e)},None=>{crate::fd::iouring_clear(id);for frame in frames{let _=crate::memory::free_frame(frame);}crate::fd::release(id);errno::neg(errno::ESRCH)}}
}
fn io_uring_rw(fd:i32,offset:u64,address:u64,len:usize,read:bool)->i32{
 if fd<0{return -(errno::EBADF as i32)}if len>65_536{return -(errno::EINVAL as i32)}if len>0&&address==0{return -(errno::EFAULT as i32)}
 let id=match crate::task::with_current_process(|p|p.fds.get(fd as usize)).flatten(){Some(id)=>id,None=>return -(errno::EBADF as i32)};
 let mut done=0usize;let mut buffer=[0u8;512];
 while done<len{
  let count=core::cmp::min(buffer.len(),len-done);let off=match offset.checked_add(done as u64){Some(v)=>v,None=>return if done==0{-(errno::EINVAL as i32)}else{done as i32}};
  let result=if read{crate::fd::read_at(id,off,&mut buffer[..count])}else{
   let pointer=match (address as usize).checked_add(done){Some(v)=>v,None=>return if done==0{-(errno::EFAULT as i32)}else{done as i32}};
   if let Err(e)=crate::usercopy::copy_from_user(&mut buffer[..count],pointer){return if done==0{-(e as i32)}else{done as i32}}
   crate::fd::write_at(id,off,&buffer[..count])
  };
  let n=match result{Ok(n)=>n,Err(e)=>return if done==0{-(e as i32)}else{done as i32}};if n>count{return if done==0{-(errno::EIO as i32)}else{done as i32}}
  if n==0{break}
  if read{let pointer=match (address as usize).checked_add(done){Some(v)=>v,None=>return if done==0{-(errno::EFAULT as i32)}else{done as i32}};if let Err(e)=crate::usercopy::copy_to_user(pointer,&buffer[..n]){return if done==0{-(e as i32)}else{done as i32}}}
  done+=n;
 }
 done as i32
}
fn sys_io_uring_enter(fd:usize,to_submit:u32,min_complete:u32,flags:u32,sig:u64,sigsetsize:u64)->i64{
  if flags!=0{return errno::neg(errno::EOPNOTSUPP)}if sig!=0||sigsetsize!=0{return errno::neg(errno::EINVAL)}if min_complete!=0{return errno::neg(errno::EOPNOTSUPP)}
  let id=match crate::task::with_current_process(|p|p.fds.get(fd)).flatten(){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
  if !matches!(crate::fd::kind(id),Some(crate::fd::OpenKind::IoUring)){return errno::neg(errno::EINVAL)}
  let entries=crate::fd::iouring_entries(id);if entries==0||entries>64||!entries.is_power_of_two(){return errno::neg(errno::EINVAL)}
  let sq_frame=crate::fd::iouring_sq_ring(id);let cq_frame=crate::fd::iouring_cq_ring(id);let sqes_frame=crate::fd::iouring_sqes(id);
  if sq_frame==0||cq_frame==0||sqes_frame==0{return errno::neg(errno::EINVAL)}
  let sq=unsafe{crate::arch::x86_64::phys::Identity.bytes(sq_frame,4096)};let cq=unsafe{crate::arch::x86_64::phys::Identity.bytes(cq_frame,4096)};let sqes=unsafe{crate::arch::x86_64::phys::Identity.bytes(sqes_frame,4096)};
  let mut sq_head=crate::fd::iouring_sq_head(id);let sq_tail=u32::from_ne_bytes(sq[4..8].try_into().unwrap());let pending=sq_tail.wrapping_sub(sq_head);
  if pending>entries{return errno::neg(errno::EINVAL)}
  let mut cq_tail=crate::fd::iouring_cq_tail(id);let cq_head=u32::from_ne_bytes(cq[..4].try_into().unwrap());let occupied=cq_tail.wrapping_sub(cq_head);
  if occupied>entries{return errno::neg(errno::EINVAL)}
  let space=entries-occupied;if space==0&&to_submit!=0{return errno::neg(errno::EAGAIN)}
  let count=to_submit.min(pending).min(space).min(16);let mask=entries-1;let mut processed=0u32;
  while processed<count{
   let array_off=24+((sq_head&mask)as usize)*4;let sqe_index=u32::from_ne_bytes(sq[array_off..array_off+4].try_into().unwrap());
   let(index_valid,sqe_index)=(sqe_index<entries,sqe_index as usize);
   let(mut user_data,mut result)=(0u64,0i32);
   if index_valid{
    let off=sqe_index*64;let sqe=&sqes[off..off+64];let opcode=sqe[0];let file_fd=i32::from_ne_bytes(sqe[4..8].try_into().unwrap());let file_off=u64::from_ne_bytes(sqe[8..16].try_into().unwrap());let address=u64::from_ne_bytes(sqe[16..24].try_into().unwrap());let len=u32::from_ne_bytes(sqe[24..28].try_into().unwrap())as usize;user_data=u64::from_ne_bytes(sqe[32..40].try_into().unwrap());
    result=match opcode{0=>0,22=>io_uring_rw(file_fd,file_off,address,len,true),23=>io_uring_rw(file_fd,file_off,address,len,false),_=>-(errno::ENOSYS as i32)};
   }else{result=-(errno::EINVAL as i32)}
   let cqe_off=20+((cq_tail&mask)as usize)*16;cq[cqe_off..cqe_off+8].copy_from_slice(&user_data.to_ne_bytes());cq[cqe_off+8..cqe_off+12].copy_from_slice(&result.to_ne_bytes());cq[cqe_off+12..cqe_off+16].copy_from_slice(&0u32.to_ne_bytes());
   sq_head=sq_head.wrapping_add(1);cq_tail=cq_tail.wrapping_add(1);processed+=1;
  }
  sq[..4].copy_from_slice(&sq_head.to_ne_bytes());cq[4..8].copy_from_slice(&cq_tail.to_ne_bytes());crate::fd::iouring_set_sq_head(id,sq_head);crate::fd::iouring_set_cq_tail(id,cq_tail);processed as i64
}
fn sys_io_uring_register(fd:usize,opcode:u32,arg:u64,nr_args:u32)->i64{
  let _=(opcode,arg,nr_args);
  let id=match crate::task::with_current_process(|p|p.fds.get(fd)).flatten(){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
  if !matches!(crate::fd::kind(id),Some(crate::fd::OpenKind::IoUring)){return errno::neg(errno::EINVAL)}
  errno::neg(errno::EOPNOTSUPP)
}

fn sys_setsockopt(fd:usize,_level:i32,_optname:i32,optval:u64,_optlen:u64)->i64{
 if netlink_fd(fd){return 0}
 let _=match sock_fd(fd){Ok(s)=>s,Err(e)=>return errno::neg(e)};
 0
}
fn sys_getsockopt(fd:usize,_level:i32,_optname:i32,optval:u64,optlen:u64)->i64{
 if netlink_fd(fd){
  if optval!=0 && optlen!=0{let v=0i32;let _=crate::usercopy::copy_to_user(optval as usize,&v.to_ne_bytes());let _=crate::usercopy::copy_to_user(optlen as usize,&4u32.to_ne_bytes());}
  return 0
 }
 let _=match sock_fd(fd){Ok(s)=>s,Err(e)=>return errno::neg(e)};
 if optval!=0 && optlen!=0{
  let v=0i32;
  let _=crate::usercopy::copy_to_user(optval as usize,&v.to_ne_bytes());
  let _=crate::usercopy::copy_to_user(optlen as usize,&4u32.to_ne_bytes());
 }
 0
}
fn parse_msghdr_iov(msg:u64)->Result<(u64,u64,u64,usize,i32),i64>{
 // returns (msg_name, msg_namelen, iov_base, iov_count, flags)
 if msg==0{return Err(errno::EFAULT)}
 let mut hdr=[0u8;56];
 if let Err(e)=crate::usercopy::copy_from_user(&mut hdr,msg as usize){return Err(e)}
 let name=u64::from_ne_bytes(hdr[0..8].try_into().unwrap());
 let namelen=u32::from_ne_bytes(hdr[8..12].try_into().unwrap()) as u64;
 let iov=u64::from_ne_bytes(hdr[16..24].try_into().unwrap());
 let iovlen=usize::from_ne_bytes(hdr[24..32].try_into().unwrap());
 let flags=i32::from_ne_bytes(hdr[48..52].try_into().unwrap()) as usize;
 Ok((name,namelen,iov,iovlen as usize,flags as i32))
}
fn sys_sendmsg(fd:usize,msg:u64,_flags:i32)->i64{
 let nonblock=fd_id(fd as u64).map(crate::fd::is_nonblock).unwrap_or(false)||(_flags as u64)&0x40!=0;
 if netlink_fd(fd){
  let (iov,iovlen)=match parse_msghdr_iov(msg){Ok(v)=>(v.2,v.3),Err(e)=>return errno::neg(e)};
  let mut total=0usize;
  for i in 0..iovlen.min(16){
   let mut hdr=[0u8;16];
   if let Err(e)=crate::usercopy::copy_from_user(&mut hdr,iov as usize + i*16){return errno::neg(e)}
   total += usize::from_ne_bytes(hdr[8..16].try_into().unwrap());
  }
  return total as i64;
 }
 let sock=match sock_fd(fd){Ok(s)=>s,Err(e)=>return errno::neg(e)};
 // aggregate
 let (name,namelen,iov,iovlen,_flags)=match parse_msghdr_iov(msg){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let mut total=0usize;
 let mut buf=[0u8;512];
 for i in 0..iovlen.min(16){
  let mut hdr=[0u8;16];
  if let Err(e)=crate::usercopy::copy_from_user(&mut hdr,iov as usize + i*16){return errno::neg(e)}
  let base=u64::from_ne_bytes(hdr[0..8].try_into().unwrap());
  let len=usize::from_ne_bytes(hdr[8..16].try_into().unwrap());
  if len==0{continue}
  let n=core::cmp::min(len,buf.len()-total);
  if n>0{
   if let Err(e)=crate::usercopy::copy_from_user(&mut buf[total..total+n],base as usize){return errno::neg(e)}
   total+=n;
  }
 }
 if total==0{return 0}
 if let Some(tcp)=crate::socket::inet_kind(sock){
  if tcp{
   return match crate::net::tcp_send(sock-crate::socket::INET_TCP_BASE,&buf[..total]){Ok(n)=>n as i64,Err(e)=>errno::neg(e)};
  } else {
   let mut ip=0u32;let mut port=0u16;
   if name!=0 && namelen>=16{
    if let Ok((i,p))=sockaddr_in_of(name,namelen as u64){ip=i;port=p;}
   }
   return match crate::net::udp_send(sock-crate::socket::INET_UDP_BASE,if name!=0{Some((ip,port))}else{None},&buf[..total]){Ok(n)=>n as i64,Err(e)=>errno::neg(e)};
  }
 }
 // unix
 match crate::socket::send(sock,&buf[..total],nonblock){Ok(n)=>n as i64,Err(e)=>errno::neg(e)}
}
fn sys_recvmsg(fd:usize,msg:u64,_flags:i32)->i64{
 let nonblock=fd_id(fd as u64).map(crate::fd::is_nonblock).unwrap_or(false)||(_flags as u64)&0x40!=0;
 if netlink_fd(fd){return 0;}
 let sock=match sock_fd(fd){Ok(s)=>s,Err(e)=>return errno::neg(e)};
 let (name,_namelen,iov,iovlen,_flags)=match parse_msghdr_iov(msg){Ok(v)=>v,Err(e)=>return errno::neg(e)};
 let mut total_cap=0usize;
 for i in 0..iovlen.min(16){
  let mut hdr=[0u8;16];
  if let Err(e)=crate::usercopy::copy_from_user(&mut hdr,iov as usize + i*16){return errno::neg(e)}
  total_cap += usize::from_ne_bytes(hdr[8..16].try_into().unwrap());
 }
 if total_cap==0{return 0}
 let mut buf=[0u8;512];
 let n=core::cmp::min(total_cap,buf.len());
 let got=if let Some(tcp)=crate::socket::inet_kind(sock){
  if tcp{match crate::net::tcp_recv(sock-crate::socket::INET_TCP_BASE,&mut buf[..n]){Ok(g)=>g,Err(e)=>return errno::neg(e)}}
  else{match crate::net::udp_recv(sock-crate::socket::INET_UDP_BASE,&mut buf[..n]){Ok((g,_,_))=>g,Err(e)=>return errno::neg(e)}}
 }else{
  match crate::socket::recv(sock,&mut buf[..n],nonblock){Ok(g)=>g,Err(e)=>return errno::neg(e)}
 };
 // write iov
 let mut off=0usize;
 for i in 0..iovlen.min(16){
  let mut hdr=[0u8;16];
  if let Err(e)=crate::usercopy::copy_from_user(&mut hdr,iov as usize + i*16){return errno::neg(e)}
  let base=u64::from_ne_bytes(hdr[0..8].try_into().unwrap());
  let len=usize::from_ne_bytes(hdr[8..16].try_into().unwrap());
  if len==0{continue}
  let chunk=core::cmp::min(len,got.saturating_sub(off));
  if chunk>0{
   if let Err(e)=crate::usercopy::copy_to_user(base as usize,&buf[off..off+chunk]){return errno::neg(e)}
   off+=chunk;
  }
 }
 if name!=0{
  // best-effort peer address for inet
  if crate::socket::inet_kind(sock).is_some(){
   let mut sa=[0u8;16];
   sa[0..2].copy_from_slice(&2u16.to_ne_bytes());
   let _=crate::usercopy::copy_to_user(name as usize,&sa[..core::cmp::min(16,usize::from_ne_bytes([0;8]) )]);
  }
 }
 got as i64
}
fn sys_accept4(fd:usize,addr:u64,addrlen:u64,flags:i32)->i64{
 if flags & !0x80800 !=0{return errno::neg(errno::EINVAL)}
 accept_impl(fd,addr,addrlen,flags as u32)
}

static MEMFD_COUNTER:core::sync::atomic::AtomicU64=core::sync::atomic::AtomicU64::new(1);
fn sys_memfd_create(name:u64,flags:u64)->i64{
 let _=(name,flags);
 let n=MEMFD_COUNTER.fetch_add(1,core::sync::atomic::Ordering::Relaxed);
 let mut path=[0u8;64];
 let prefix=b"/memfd:";
 path[..prefix.len()].copy_from_slice(prefix);
 // append number
 let mut off=prefix.len();
 let mut v=n; let mut tmp=[0u8;20]; let mut i=0;
 if v==0{tmp[0]=b'0';i=1}else{while v>0{tmp[i]=b'0'+(v%10)as u8;v/=10;i+=1}for j in 0..i/2{tmp.swap(j,i-1-j)}}
 path[off..off+i].copy_from_slice(&tmp[..i]); off+=i; path[off]=0;
 let p=match core::str::from_utf8(&path[..off]){Ok(v)=>v,Err(_)=>return errno::neg(errno::EINVAL)};
 let _=crate::vfs::create(p,0o666);
 match crate::fd::create_open(p,0o666){Ok(fd)=>fd as i64,Err(e)=>errno::neg(e)}
}

fn sys_sendmmsg(fd:usize,msgvec:u64,vlen:u32,flags:i32)->i64{
 let mut sent=0i64;
 for i in 0..vlen.min(16){
  let r=sys_sendmsg(fd,msgvec+i as u64*56,flags);
  if r<0{return if sent>0{sent}else{r}}
  sent+=1;
  let off=msgvec as usize+i as usize*56+52;
  let _=crate::usercopy::copy_to_user(off,&(r as u32).to_ne_bytes());
 }
 sent
}
fn sys_recvmmsg(fd:usize,msgvec:u64,vlen:u32,flags:i32,_timeout:u64)->i64{
 let mut recvd=0i64;
 for i in 0..vlen.min(16){
  let r=sys_recvmsg(fd,msgvec+i as u64*56,flags);
  if r<0{return if recvd>0{recvd}else{r}}
  if r==0{break}
  let off=msgvec as usize+i as usize*56+52;
  let _=crate::usercopy::copy_to_user(off,&(r as u32).to_ne_bytes());
  recvd+=1;
 }
 recvd
}

fn sys_pidfd_open(pid:u32,flags:u64)->i64{
 if flags!=0{return errno::neg(errno::EINVAL)}
 if !crate::task::sched_pids().iter().any(|&p|p==pid){return errno::neg(errno::ESRCH)}
 let id=match crate::fd::alloc_file(crate::fd::OpenKind::PidFd(pid)){Ok(d)=>d,Err(e)=>return errno::neg(e)};
 match crate::task::with_current_process(|p|p.fds.install(id)){Some(Ok(fd))=>fd as i64,Some(Err(e))=>{crate::fd::release(id);errno::neg(e)},None=>{crate::fd::release(id);errno::neg(errno::ESRCH)}}
}
fn sys_pidfd_send_signal(fd:usize,sig:i32,info:u64,flags:u64)->i64{
 if flags!=0{return errno::neg(errno::EINVAL)}
 let id=match crate::task::with_current_process(|p|p.fds.get(fd)).flatten(){Some(id)=>id,None=>return errno::neg(errno::EBADF)};
 let pid=match crate::fd::kind(id){Some(crate::fd::OpenKind::PidFd(p))=>p,_=>return errno::neg(errno::EBADF)};
 if sig==0{return 0}
 match crate::signal::send(pid,sig){Ok(())=>{
  /* A fatal signal aimed at the caller ends the caller before the syscall
     returns, exactly like Linux. A signal with a handler is delivered
     normally, so the check only applies to the default disposition. */
  let blocked=crate::task::with_current_process(|p|p.sigmask&crate::signal::bit(sig)!=0).unwrap_or(false);
  if pid==crate::task::current_pid()&&!blocked&&crate::signal::default_terminates(sig)&&crate::signal::action_get(sig).map(|a|a.handler==crate::signal::SIG_DFL).unwrap_or(true){crate::task::exit_signal(sig);}
  0
 } Err(e)=>errno::neg(e)}
}

#[repr(C)] #[derive(Clone,Copy)] struct SockFilter{code:u16,jt:u8,jf:u8,k:u32}
#[repr(C)] #[derive(Clone,Copy)] struct SeccompData{nr:i32,arch:u32,ip:u64,args:[u64;6]}

fn seccomp_load_abs(data:&SeccompData,off:usize,size:usize)->u32{
 // build a 64-byte view
 let mut buf=[0u8;64];
 unsafe{core::ptr::copy_nonoverlapping(data as *const SeccompData as *const u8,buf.as_mut_ptr(),64)}
 if off+size>64{return 0}
 let mut v=0u32;
 for i in 0..size{v |= (buf[off+i] as u32) << (i*8); }
 v
}
fn seccomp_eval(filter:*const SockFilter,len:usize,data:&SeccompData)->u32{
 if filter.is_null() || len==0 {return 0x80000000}
 let mut a=0u32;let mut x=0u32;let mut mem=[0u32;16];
 let mut pc=0usize;
 let mut steps=0usize;
 while pc < len && steps < 4096 {
  steps+=1;
  let insn=unsafe{filter.add(pc).read()};
  let class=insn.code & 0x07;
  match class {
   0 => { // LD
     let mode=insn.code & 0xe0;
     match mode {
      0x00 => { a=insn.k; }
      0x20 => { a=seccomp_load_abs(data,insn.k as usize,4); }
      0x40 => { a=seccomp_load_abs(data,(insn.k as usize).wrapping_add(x as usize),4); }
      0x60 => { let idx=(insn.k as usize)&15; a=mem[idx]; }
      0x80 => { a=64; } // LEN
      _ => { return 0x80000000; }
     }
     pc+=1;
   }
   1 => { // LDX
     let mode=insn.code & 0xe0;
     match mode {
      0x00 => { x=insn.k; }
      0x20 => { x=seccomp_load_abs(data,insn.k as usize,4); }
       0x40 => { x=seccomp_load_abs(data,(insn.k as usize).wrapping_add(x as usize),4); }
       0x60 => { let idx=(insn.k as usize)&15; x=mem[idx]; }
       0x80 => { x=64; }
       0xa0 => { // MSH: x = (data byte at k & 0xf)<<2
       let b=seccomp_load_abs(data,insn.k as usize,1) as u8;
       x=((b & 0x0f) as u32)<<2;
      }
      _ => { return 0x80000000; }
     }
     pc+=1;
   }
   2 => { // ST
     let idx=(insn.k as usize)&15; mem[idx]=a; pc+=1;
   }
   3 => { // STX
     let idx=(insn.k as usize)&15; mem[idx]=x; pc+=1;
   }
   4 => { // ALU
     let op=insn.code & 0xf0;
     let src=if insn.code & 0x08 !=0 {x}else{insn.k};
     match op {
      0x00 => a=a.wrapping_add(src),
      0x10 => a=a.wrapping_sub(src),
      0x20 => a=a.wrapping_mul(src),
      0x30 => a=if src==0{0}else{a/src},
      0x90 => a=if src==0{0}else{a%src},
      0x40 => a |= src,
      0x50 => a &= src,
       0x60 => a=if src>=32{0}else{a<<src},
       0x70 => a=if src>=32{0}else{a>>src},
      0x80 => a = (!a).wrapping_add(1), // NEG (two's complement)
      0xa0 => a ^= src,
      _ => { return 0x80000000; }
     }
     pc+=1;
   }
   5 => { // JMP
     let op=insn.code & 0xf0;
     let src=if insn.code & 0x08 !=0 {x}else{insn.k};
     match op {
       0x00 => { pc += insn.k as usize+1; }
      0x10 => { if a==src { pc += insn.jt as usize + 1 } else { pc += insn.jf as usize + 1 } }
      0x20 => { if a>src { pc += insn.jt as usize + 1 } else { pc += insn.jf as usize + 1 } }
      0x30 => { if a>=src { pc += insn.jt as usize + 1 } else { pc += insn.jf as usize + 1 } }
      0x40 => { if a & src !=0 { pc += insn.jt as usize + 1 } else { pc += insn.jf as usize + 1 } }
      _ => { return 0x80000000; }
     }
     if op==0x00 { continue; }
     // for conditional we already advanced pc
     continue;
    }
    6 => { // RET
      let v=if insn.code==0x16{a}else{insn.k};
     return v;
   }
   7 => { // MISC
      match insn.code {
       0x07=>{x=a;}
       0x87=>{a=x;}
       _=>{return 0x80000000;}
     }
     pc+=1;
   }
   _ => { return 0x80000000; }
  }
 }
 0x80000000
}

fn seccomp_validate(filter:*const SockFilter,len:usize)->bool{
 if filter.is_null() || len==0 || len>512 {return false}
 let last=unsafe{filter.add(len-1).read()};
 if last.code!=0x06&&last.code!=0x16{return false}
 for i in 0..len {
  let insn=unsafe{filter.add(i).read()};
  if insn.code&0xff00!=0{return false}
  let class=insn.code&0x07;
  match class {
   0=>match insn.code{0x00|0x20|0x40|0x80=>{},0x60 if insn.k<16=>{},_=>return false},
   1=>match insn.code{0x01|0x21|0x41|0x81=>{},0x61 if insn.k<16=>{},0xb1=>{},_=>return false},
   2|3=>if insn.code!=class||insn.k>=16{return false},
   4=>{let op=insn.code&0xf0;let src_x=insn.code&0x08!=0;if !matches!(op,0x00|0x10|0x20|0x30|0x40|0x50|0x60|0x70|0x80|0x90|0xa0)||op==0x80&&src_x{return false}},
   5 => {
      let op=insn.code & 0xf0;
      if op==0x00 {
        if insn.code!=0x05||i+(insn.k as usize)+1>=len{return false}
      } else {
        if !matches!(op,0x10|0x20|0x30|0x40){return false}
        let t=i + (insn.jt as usize) + 1;
        let f=i + (insn.jf as usize) + 1;
        if t>=len||f>=len{return false}
      }
   }
   6=>if insn.code!=0x06&&insn.code!=0x16{return false},
   7=>if insn.code!=0x07&&insn.code!=0x87{return false},
   _=>return false,
  }
 }
 true
}
