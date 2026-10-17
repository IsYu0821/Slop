pub mod nr; pub mod stack; pub mod types;
use crate::errno; use core::mem::size_of; use types::*;

#[repr(C)] struct UtsName { sysname:[u8;65], nodename:[u8;65], release:[u8;65], version:[u8;65], machine:[u8;65], domainname:[u8;65] }

pub fn dispatch(n:u64,a:[u64;6])->i64 { match n {
    nr::READ=>sys_read(a[0],a[1],a[2]), nr::WRITE=>sys_write(a[0],a[1],a[2]), nr::CLOSE=>sys_close(a[0]),
    nr::FSTAT=>sys_fstat(a[0],a[1]), nr::LSEEK=>sys_lseek(a[0],a[1] as i64,a[2]),
    nr::OPENAT=>sys_openat(a[0] as i64,a[1],a[2]), nr::NEWFSTATAT=>sys_newfstatat(a[0] as i64,a[1],a[2],a[3]),
    nr::MMAP=>result(crate::process::mmap(a[0] as usize,a[1] as usize,a[2],a[3],a[4] as i64,a[5])),
    nr::MPROTECT=>unit(crate::process::mprotect(a[0] as usize,a[1] as usize,a[2])),
    nr::MUNMAP=>unit(crate::process::munmap(a[0] as usize,a[1] as usize)), nr::BRK=>crate::process::brk(a[0]) as i64,
    nr::SCHED_YIELD=>0, nr::GETPID|nr::GETTID=>crate::task::current_pid() as i64, nr::GETPPID=>crate::task::current_ppid() as i64,
    nr::EXECVE=>sys_execve(a[0],a[1],a[2]),
    nr::GETUID|nr::GETGID|nr::GETEUID|nr::GETEGID=>0, nr::UNAME=>sys_uname(a[0]), nr::ARCH_PRCTL=>sys_arch_prctl(a[0],a[1]),
    nr::SET_TID_ADDRESS=>{crate::process::set_clear_child_tid(a[0]);crate::task::current_pid() as i64},
    nr::SET_ROBUST_LIST=>if a[1]==24 {0}else{errno::neg(errno::EINVAL)}, nr::PRLIMIT64=>sys_prlimit(a),
    nr::CLOCK_GETTIME=>sys_clock_gettime(a[0],a[1]), nr::NANOSLEEP=>sys_nanosleep(a[0],a[1]),
    nr::GETRANDOM=>sys_getrandom(a[0],a[1],a[2]),
    nr::EXIT|nr::EXIT_GROUP=>crate::task::exit(a[0] as i32), _=>errno::neg(errno::ENOSYS),
}}
fn sys_read(fd:u64,ptr:u64,len:u64)->i64 {if len>isize::MAX as u64{return errno::neg(errno::EFAULT)}let mut done=0usize;let mut buf=[0u8;256];while done<len as usize{let wanted=core::cmp::min(buf.len(),len as usize-done);let n=match crate::fd::read(fd as usize,&mut buf[..wanted]){Ok(n)=>n,Err(e)=>{return if done==0{errno::neg(e)}else{done as i64}}};if n==0{break}let dst=match (ptr as usize).checked_add(done){Some(v)=>v,None=>return errno::neg(errno::EFAULT)};if let Err(e)=crate::usercopy::copy_to_user(dst,&buf[..n]){return if done==0{errno::neg(e)}else{done as i64}}done+=n;}done as i64}
fn sys_write(fd:u64,ptr:u64,len:u64)->i64 {
    if fd!=1&&fd!=2{return errno::neg(errno::EBADF)} if len>isize::MAX as u64{return errno::neg(errno::EFAULT)}
    let mut done=0usize; let mut buf=[0u8;256]; while done<len as usize {let n=core::cmp::min(buf.len(),len as usize-done);
        let src=match (ptr as usize).checked_add(done){Some(v)=>v,None=>return errno::neg(errno::EFAULT)};
        if let Err(e)=crate::usercopy::copy_from_user(&mut buf[..n],src){return errno::neg(e)}
        crate::console::write_bytes(&buf[..n]); done+=n;} done as i64
}
fn sys_close(fd:u64)->i64{if fd<=2{0}else{unit(crate::fd::close(fd as usize))}}
fn sys_fstat(fd:u64,ptr:u64)->i64 {let mut st=Stat::default();if fd<=2{st.st_mode=S_IFCHR|0o666;st.st_nlink=1;}else{let n=match crate::fd::inode(fd as usize){Ok(n)=>n,Err(e)=>return errno::neg(e)};st.st_ino=n.ino;st.st_mode=n.mode;st.st_nlink=1;st.st_size=n.size as i64;st.st_blocks=((n.size+511)/512)as i64;}st.st_blksize=4096;copy_obj(ptr,&st)}
fn sys_lseek(fd:u64,off:i64,whence:u64)->i64{match crate::fd::seek(fd as usize,off,whence){Ok(v)=>v as i64,Err(e)=>errno::neg(e)}}
fn copy_path<'a>(ptr:u64,out:&'a mut[u8])->Result<&'a str,i64>{for i in 0..out.len(){crate::usercopy::copy_from_user(&mut out[i..i+1],ptr as usize+i)?;if out[i]==0{return core::str::from_utf8(&out[..i]).map_err(|_|errno::EINVAL)}}Err(errno::ENAMETOOLONG)}
fn sys_openat(dirfd:i64,path:u64,flags:u64)->i64{const AT_FDCWD:i64=-100;if dirfd!=AT_FDCWD||flags&3!=0{return errno::neg(errno::EINVAL)}let mut b=[0u8;128];let p=match copy_path(path,&mut b){Ok(v)=>v,Err(e)=>return errno::neg(e)};match crate::fd::open(p){Ok(fd)=>fd as i64,Err(e)=>errno::neg(e)}}
fn sys_newfstatat(dirfd:i64,path:u64,ptr:u64,flags:u64)->i64{const AT_FDCWD:i64=-100;if dirfd!=AT_FDCWD||flags!=0{return errno::neg(errno::EINVAL)}let mut b=[0u8;128];let p=match copy_path(path,&mut b){Ok(v)=>v,Err(e)=>return errno::neg(e)};let n=match crate::vfs::lookup(p){Ok(n)=>n,Err(e)=>return errno::neg(e)};let mut st=Stat::default();st.st_ino=n.ino;st.st_mode=n.mode;st.st_nlink=1;st.st_size=n.size as i64;st.st_blksize=4096;st.st_blocks=((n.size+511)/512)as i64;copy_obj(ptr,&st)}
fn field(s:&str)->[u8;65]{let mut x=[0;65];let b=s.as_bytes();let n=core::cmp::min(64,b.len());x[..n].copy_from_slice(&b[..n]);x}
fn sys_uname(ptr:u64)->i64{let u=UtsName{sysname:field("Linux"),nodename:field("slop"),release:field("6.6.0-mnu"),version:field("Slop/MNU 0.26.0"),machine:field("x86_64"),domainname:field("localdomain")};copy_obj(ptr,&u)}
fn sys_arch_prctl(code:u64,addr:u64)->i64 {const SET_FS:u64=0x1002;const GET_FS:u64=0x1003;match code{SET_FS=>if crate::process::set_fs_base(addr){0}else{errno::neg(errno::EPERM)},GET_FS=>copy_obj(addr,&crate::process::fs_base()),_=>errno::neg(errno::EINVAL)}}
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
 let loaded=match unsafe{crate::exec_runtime::load_static_args(image,argv,&ev[..ec],path,crate::process_runtime::bootstrap_cr3())}{Ok(v)=>v,Err(_)=>return errno::neg(errno::ENOEXEC)};
 if let Err(e)=crate::process_runtime::replace_image(loaded.cr3,loaded.image_slot,loaded.stack_slot){return errno::neg(e)}
 crate::process::reset_exec_state();unsafe{crate::arch::x86_64::syscall::commit_exec_return(loaded.entry,loaded.rsp)}
 crate::println!("execve: committed {} rip={:#x} rsp={:#x}",path,loaded.entry,loaded.rsp);0
}

fn sys_clock_gettime(clock:u64,ptr:u64)->i64{if clock!=0&&clock!=1{return errno::neg(errno::EINVAL)}let ns=crate::time::monotonic_ns();let ts=Timespec{tv_sec:(ns/1_000_000_000)as i64,tv_nsec:(ns%1_000_000_000)as i64};copy_obj(ptr,&ts)}
fn sys_nanosleep(req:u64,rem:u64)->i64{let mut ts=Timespec::default();let b=unsafe{core::slice::from_raw_parts_mut((&mut ts as*mut Timespec).cast::<u8>(),core::mem::size_of::<Timespec>())};if let Err(e)=crate::usercopy::copy_from_user(b,req as usize){return errno::neg(e)}if ts.tv_sec<0||ts.tv_nsec<0||ts.tv_nsec>=1_000_000_000{return errno::neg(errno::EINVAL)}let ns=(ts.tv_sec as u64).saturating_mul(1_000_000_000).saturating_add(ts.tv_nsec as u64);crate::task::sleep_until(crate::time::ticks_after_ns(ns));if rem!=0{let zero=Timespec::default();let _=copy_obj(rem,&zero);}0}
fn sys_getrandom(ptr:u64,len:u64,flags:u64)->i64{if flags&!3!=0{return errno::neg(errno::EINVAL)}if len>isize::MAX as u64{return errno::neg(errno::EFAULT)}let mut done=0usize;let mut buf=[0u8;64];while done<len as usize{let n=core::cmp::min(buf.len(),len as usize-done);crate::time::fill_random(&mut buf[..n]);if let Err(e)=crate::usercopy::copy_to_user(ptr as usize+done,&buf[..n]){return if done==0{errno::neg(e)}else{done as i64}}done+=n;}done as i64}
