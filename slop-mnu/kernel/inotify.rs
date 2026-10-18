//! Minimal inotify watch and event queue.
use crate::errno;
pub const IN_CREATE:u32=0x00000100;pub const IN_DELETE:u32=0x00000200;pub const IN_MODIFY:u32=0x00000002;pub const IN_MOVED_FROM:u32=0x00000040;pub const IN_MOVED_TO:u32=0x00000080;pub const IN_ISDIR:u32=0x40000000;
const MAX_WATCHES:usize=64;const BUF_SZ:usize=4096;
struct Watch{used:bool,fd:u32,wd:i32,path:[u8;128],plen:usize,mask:u32}
const EMPTY_WATCH:Watch=Watch{used:false,fd:0,wd:0,path:[0;128],plen:0,mask:0};
static mut WATCHES:[Watch;MAX_WATCHES]=[EMPTY_WATCH;MAX_WATCHES];
static mut NEXT_WD:i32=1;
static mut BUFS:[[u8;BUF_SZ];64]=[[0;BUF_SZ];64];
static mut LENS:[usize;64]=[0;64];

fn parent_of(path:&str)->(&str,&str){
 if path=="/"{return ("/","")}
 let p=path.trim_end_matches('/');
 match p.rfind('/') {
  Some(i) if i>0 => (&p[..i],&p[i+1..]),
  Some(0) => ("/",&p[1..]),
  _ => ("/",p),
 }
}

pub fn add_watch(fd_id:u32,path:&str,mask:u32)->i32{
 if path.is_empty(){return -1}
 let w=unsafe{&mut *(&raw mut WATCHES)};
 let slot=match w.iter().position(|x|!x.used){Some(i)=>i,None=>return -1};
 let wd=unsafe{let v=NEXT_WD;NEXT_WD+=1;v};
 let b=path.as_bytes();
 let pl=b.len().min(127);
 w[slot].used=true;w[slot].fd=fd_id;w[slot].wd=wd;w[slot].path[..pl].copy_from_slice(&b[..pl]);w[slot].plen=pl;w[slot].mask=mask;
 wd
}
pub fn rm_watch(fd_id:u32,wd:i32)->i64{
 let w=unsafe{&mut *(&raw mut WATCHES)};
 for e in w.iter_mut(){if e.used && e.fd==fd_id && e.wd==wd{*e=EMPTY_WATCH;return 0}}
 crate::errno::ENOENT
}
pub fn push_event(fd_id:u32,mask:u32,name:&str){
 for w in unsafe{&mut *(&raw mut WATCHES)}.iter(){
  if !w.used || w.fd!=fd_id {continue}
  if w.mask & mask ==0 {continue}
  // build event
  let nameb=name.as_bytes();
  let namelen=(nameb.len()+1+3)&!3;
  let total=16+namelen;
  let off=unsafe{(*(&raw const LENS))[fd_id as usize]};
  if off+total > BUF_SZ {continue}
  let buf=unsafe{&mut (*(&raw mut BUFS))[fd_id as usize]};
  let mut pos=off;
  buf[pos..pos+4].copy_from_slice(&w.wd.to_ne_bytes()); pos+=4;
  buf[pos..pos+4].copy_from_slice(&mask.to_ne_bytes()); pos+=4;
  buf[pos..pos+4].copy_from_slice(&0u32.to_ne_bytes()); pos+=4; // cookie
  buf[pos..pos+4].copy_from_slice(&(namelen as u32).to_ne_bytes()); pos+=4;
  for i in 0..nameb.len(){buf[pos+i]=nameb[i];}
  buf[pos+nameb.len()]=0;
  unsafe{(*(&raw mut LENS))[fd_id as usize]+=total;}
 }
}
fn path_matches(watch_path:&str,changed:&str)->bool{
 // watch_path is directory being watched; changed is full path of changed entry
 let (parent,name)=parent_of(changed);
 parent==watch_path || watch_path==changed
}
pub fn notify(path:&str,mask:u32){
 // path is full path of changed object; notify watches on parent
 let (parent,name)=parent_of(path);
 for w in unsafe{&*(&raw const WATCHES)}.iter(){
  if !w.used {continue}
  let wp=core::str::from_utf8(&w.path[..w.plen]).unwrap_or("");
  if wp==parent && (w.mask & mask)!=0 {
   push_event(w.fd,mask,name);
  }
 }
}
pub fn read(fd_id:u32,dst:&mut[u8])->Result<usize,i64>{
 let off=unsafe{(*(&raw const LENS))[fd_id as usize]};
 if off==0{return Err(crate::errno::EAGAIN)}
 let n=core::cmp::min(dst.len(),off);
 let src=unsafe{&(*(&raw const BUFS))[fd_id as usize]}; dst[..n].copy_from_slice(&src[..n]);
 // shift remaining
 let rem=off-n;
 if rem>0{let dstb=unsafe{&mut (*(&raw mut BUFS))[fd_id as usize]}; let srcb=unsafe{&(*(&raw const BUFS))[fd_id as usize]}; dstb[..rem].copy_from_slice(&srcb[n..off]);}
 unsafe{(*(&raw mut LENS))[fd_id as usize]=rem;}
 Ok(n)
}
