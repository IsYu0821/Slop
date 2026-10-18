//! Minimal character device registry and default /dev nodes.
use crate::errno;
pub const NULL_MAJOR:u32=1; pub const NULL_MINOR:u32=3;
pub const ZERO_MAJOR:u32=1; pub const ZERO_MINOR:u32=5;
pub const FULL_MAJOR:u32=1; pub const FULL_MINOR:u32=7;
pub const RANDOM_MAJOR:u32=1; pub const RANDOM_MINOR:u32=8;
pub const URANDOM_MAJOR:u32=1; pub const URANDOM_MINOR:u32=9;
pub const TTY_MAJOR:u32=5; pub const TTY_MINOR:u32=0;
pub const CONSOLE_MAJOR:u32=5; pub const CONSOLE_MINOR:u32=1;

pub fn read(major:u32,minor:u32,dst:&mut[u8])->Result<usize,i64>{
 match (major,minor){
  (NULL_MAJOR,NULL_MINOR)=>Ok(0),
  (ZERO_MAJOR,ZERO_MINOR)=>{for b in dst.iter_mut(){*b=0;}Ok(dst.len())},
  (FULL_MAJOR,FULL_MINOR)=>Err(errno::ENOSPC),
  (RANDOM_MAJOR,RANDOM_MINOR)|(URANDOM_MAJOR,URANDOM_MINOR)=>{crate::time::fill_random(dst);Ok(dst.len())},
  (TTY_MAJOR,TTY_MINOR)|(CONSOLE_MAJOR,CONSOLE_MINOR)=>Ok(0),
  _=>Err(errno::ENODEV),
 }
}
pub fn write(major:u32,minor:u32,src:&[u8])->Result<usize,i64>{
 match (major,minor){
  (NULL_MAJOR,NULL_MINOR)=>Ok(src.len()),
  (ZERO_MAJOR,ZERO_MINOR)|(RANDOM_MAJOR,RANDOM_MINOR)|(URANDOM_MAJOR,URANDOM_MINOR)=>Ok(src.len()),
  (FULL_MAJOR,FULL_MINOR)=>Err(errno::ENOSPC),
  (TTY_MAJOR,TTY_MINOR)|(CONSOLE_MAJOR,CONSOLE_MINOR)=>{crate::console::write_bytes(src);Ok(src.len())},
  _=>Err(errno::ENODEV),
 }
}
pub fn ioctl(major:u32,minor:u32,cmd:u64,arg:u64)->i64{
 match (major,minor){
  (254,_)=>{
   match cmd{
    0x80081272=>{ // BLKGETSIZE64
     if arg==0{return errno::neg(errno::EFAULT)}
     let size=crate::driver::virtio_blk::capacity().saturating_mul(512);
     if let Err(e)=crate::usercopy::copy_to_user(arg as usize,&size.to_ne_bytes()){return errno::neg(e)}
     0
    }
    0x00001268|0x00001260=>{ // BLKSSZGET / BLKBSZGET
     if arg==0{return errno::neg(errno::EFAULT)}
     let v=512i32;
     if let Err(e)=crate::usercopy::copy_to_user(arg as usize,&v.to_ne_bytes()){return errno::neg(e)}
     0
    }
    _=>0,
   }
  }
  (5,_)=>{
   match cmd{
    0x5401=>{if arg!=0{let zeros=[0u8;36];let _=crate::usercopy::copy_to_user(arg as usize,&zeros);}0}
    0x5413=>{if arg!=0{let zeros=[0u8;8];let _=crate::usercopy::copy_to_user(arg as usize,&zeros);}0}
    _=>0,
   }
  }
  _=>0,
 }
}
pub fn init_devtmpfs(){
 let _=crate::vfs::mkdir("/dev",0o755);
 let nodes=[
  ("/dev/null",0o20666,NULL_MAJOR,NULL_MINOR),
  ("/dev/zero",0o20666,ZERO_MAJOR,ZERO_MINOR),
  ("/dev/full",0o20666,FULL_MAJOR,FULL_MINOR),
  ("/dev/random",0o20666,RANDOM_MAJOR,RANDOM_MINOR),
  ("/dev/urandom",0o20666,URANDOM_MAJOR,URANDOM_MINOR),
  ("/dev/tty",0o20666,TTY_MAJOR,TTY_MINOR),
  ("/dev/console",0o20666,CONSOLE_MAJOR,CONSOLE_MINOR),
 ];
 for (p,mode,maj,min) in nodes{
  let dev=((maj as u64)<<8)|(min as u64);
  let _=crate::vfs::mknod(p,0o020000|mode,dev);
 }
 if crate::driver::virtio_blk::ready(){let _=crate::vfs::mknod("/dev/vda",0o060660,(254u64<<8)|0);}
}

pub fn blk_read(major:u32,minor:u32,off:u64,dst:&mut[u8])->Result<usize,i64>{
 let _=(major,minor);
 if !crate::driver::virtio_blk::ready(){return Err(errno::ENODEV)}
 let cap=crate::driver::virtio_blk::capacity()*512;
 if off>=cap{return Ok(0)}
 let mut done=0usize;
 let mut sector=off/512;
 let mut pos=off%512;
 while done<dst.len() && off as usize+done < cap as usize{
  let mut buf=[0u8;512];
  let avail=512-pos as usize;
  let n=core::cmp::min(avail,dst.len()-done);
  dst[done..done+n].copy_from_slice(&buf[pos as usize..pos as usize+n]);
  done+=n; pos=0; sector+=1;
 }
 Ok(done)
}
pub fn blk_write(major:u32,minor:u32,off:u64,src:&[u8])->Result<usize,i64>{
 let _=(major,minor);
 if !crate::driver::virtio_blk::ready(){return Err(errno::ENODEV)}
 let cap=crate::driver::virtio_blk::capacity()*512;
 if off as usize+src.len() > cap as usize{return Err(errno::ENOSPC)}
 let mut done=0usize;
 let mut sector=off/512;
 let mut pos=off%512;
 while done<src.len(){
  // read-modify-write if partial
  let mut buf=[0u8;512];
  if pos!=0 || src.len()-done <512 {
   if crate::driver::virtio_blk::read_sector(sector,&mut buf).is_err(){return if done==0{Err(errno::EIO)}else{Ok(done)}}
  }
  let n=core::cmp::min(512-pos as usize,src.len()-done);
  buf[pos as usize..pos as usize+n].copy_from_slice(&src[done..done+n]);
  if crate::driver::virtio_blk::write_sector(sector,&buf).is_err(){return if done==0{Err(errno::EIO)}else{Ok(done)}}
  done+=n; pos=0; sector+=1;
 }
 Ok(done)
}