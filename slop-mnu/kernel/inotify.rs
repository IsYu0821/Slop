//! Fixed-capacity inotify watches and record-preserving event queues.
use crate::{errno,sync::SpinLock};
pub const IN_ACCESS:u32=0x00000001;pub const IN_MODIFY:u32=0x00000002;pub const IN_ATTRIB:u32=0x00000004;pub const IN_CLOSE_WRITE:u32=0x00000008;pub const IN_CLOSE_NOWRITE:u32=0x00000010;pub const IN_OPEN:u32=0x00000020;pub const IN_MOVED_FROM:u32=0x00000040;pub const IN_MOVED_TO:u32=0x00000080;pub const IN_CREATE:u32=0x00000100;pub const IN_DELETE:u32=0x00000200;pub const IN_DELETE_SELF:u32=0x00000400;pub const IN_MOVE_SELF:u32=0x00000800;pub const IN_UNMOUNT:u32=0x00002000;pub const IN_Q_OVERFLOW:u32=0x00004000;pub const IN_IGNORED:u32=0x00008000;pub const IN_ISDIR:u32=0x40000000;
pub const IN_ONLYDIR:u32=0x01000000;pub const IN_DONT_FOLLOW:u32=0x02000000;pub const IN_EXCL_UNLINK:u32=0x04000000;pub const IN_MASK_ADD:u32=0x20000000;pub const IN_ONESHOT:u32=0x80000000;
pub const IN_NONBLOCK:u64=0x00000800;pub const IN_CLOEXEC:u64=0x00080000;
const MAX_WATCHES:usize=64;const MAX_OPEN_FILES:usize=64;const BUF_SZ:usize=4096;
const EVENT_MASK:u32=IN_ACCESS|IN_MODIFY|IN_ATTRIB|IN_CLOSE_WRITE|IN_CLOSE_NOWRITE|IN_OPEN|IN_MOVED_FROM|IN_MOVED_TO|IN_CREATE|IN_DELETE|IN_DELETE_SELF|IN_MOVE_SELF|IN_UNMOUNT;
const CONTROL_MASK:u32=IN_ONLYDIR|IN_DONT_FOLLOW|IN_EXCL_UNLINK|IN_MASK_ADD|IN_ONESHOT;
const INPUT_MASK:u32=EVENT_MASK|CONTROL_MASK;
#[derive(Clone,Copy)]struct Watch{used:bool,fd:u32,wd:i32,path:[u8;128],plen:usize,mask:u32}
const EMPTY_WATCH:Watch=Watch{used:false,fd:0,wd:0,path:[0;128],plen:0,mask:0};
struct State{watches:[Watch;MAX_WATCHES],next_wd:i32,buffers:[[u8;BUF_SZ];MAX_OPEN_FILES],lengths:[usize;MAX_OPEN_FILES],overflow:[bool;MAX_OPEN_FILES]}
static STATE:SpinLock<State>=SpinLock::new(State{watches:[EMPTY_WATCH;MAX_WATCHES],next_wd:1,buffers:[[0;BUF_SZ];MAX_OPEN_FILES],lengths:[0;MAX_OPEN_FILES],overflow:[false;MAX_OPEN_FILES]});

fn parent_of(path:&str)->(&str,&str){
 if path=="/"{return ("/","")}
 let path=path.trim_end_matches('/');
 match path.rfind('/') {Some(i) if i>0=>(&path[..i],&path[i+1..]),Some(0)=>("/",&path[1..]),_=>("/",path)}
}
fn queue_event(state:&mut State,fd:u32,wd:i32,mask:u32,cookie:u32,name:&str)->bool{
 let i=fd as usize;if i>=MAX_OPEN_FILES{return false}
 let name_bytes=name.as_bytes();let name_len=if name_bytes.is_empty(){0}else{(name_bytes.len()+1+3)&!3};let total=16+name_len;let mut off=state.lengths[i];
 if off+total>BUF_SZ{state.overflow[i]=true;return false}
 let buf=&mut state.buffers[i];buf[off..off+4].copy_from_slice(&wd.to_ne_bytes());off+=4;buf[off..off+4].copy_from_slice(&mask.to_ne_bytes());off+=4;buf[off..off+4].copy_from_slice(&cookie.to_ne_bytes());off+=4;buf[off..off+4].copy_from_slice(&(name_len as u32).to_ne_bytes());off+=4;
 if name_len>0{buf[off..off+name_bytes.len()].copy_from_slice(name_bytes);buf[off+name_bytes.len()]=0;buf[off+name_bytes.len()+1..off+name_len].fill(0)}
 state.lengths[i]=off+name_len;true
}
fn queue_overflow(state:&mut State,fd:u32){
 let i=fd as usize;if i>=MAX_OPEN_FILES||!state.overflow[i]||state.lengths[i]+16>BUF_SZ{return}
 let off=state.lengths[i];let buf=&mut state.buffers[i];buf[off..off+4].copy_from_slice(&(-1i32).to_ne_bytes());buf[off+4..off+8].copy_from_slice(&IN_Q_OVERFLOW.to_ne_bytes());buf[off+8..off+12].copy_from_slice(&0u32.to_ne_bytes());buf[off+12..off+16].copy_from_slice(&0u32.to_ne_bytes());state.lengths[i]+=16;state.overflow[i]=false;
}

pub fn add_watch(fd_id:u32,path:&str,mask:u32)->Result<i32,i64>{
 if fd_id as usize>=MAX_OPEN_FILES{return Err(errno::EBADF)}
 if path.is_empty(){return Err(errno::EINVAL)}if path.len()>127{return Err(errno::ENAMETOOLONG)}
 if mask&!INPUT_MASK!=0||mask&EVENT_MASK==0{return Err(errno::EINVAL)}
 let mut state=STATE.lock();
 if let Some(watch)=state.watches.iter_mut().find(|w|w.used&&w.fd==fd_id&&w.plen==path.len()&&&w.path[..w.plen]==path.as_bytes()){
  if mask&IN_MASK_ADD!=0{watch.mask|=mask}else{watch.mask=mask}
  return Ok(watch.wd)
 }
 let slot=state.watches.iter().position(|w|!w.used).ok_or(errno::ENOSPC)?;
 let mut wd=state.next_wd;loop{if wd<=0{wd=1}if !state.watches.iter().any(|w|w.used&&w.wd==wd){break}wd=wd.wrapping_add(1);}
 state.next_wd=wd.wrapping_add(1).max(1);let mut bytes=[0u8;128];bytes[..path.len()].copy_from_slice(path.as_bytes());state.watches[slot]=Watch{used:true,fd:fd_id,wd,path:bytes,plen:path.len(),mask};Ok(wd)
}
pub fn rm_watch(fd_id:u32,wd:i32)->Result<(),i64>{
 let mut state=STATE.lock();if fd_id as usize>=MAX_OPEN_FILES{return Err(errno::EBADF)}
 let watch=state.watches.iter_mut().find(|w|w.used&&w.fd==fd_id&&w.wd==wd).ok_or(errno::EINVAL)?;*watch=EMPTY_WATCH;
 queue_event(&mut state,fd_id,wd,IN_IGNORED,0,"");Ok(())
}
pub fn close(fd_id:u32){
 let i=fd_id as usize;if i>=MAX_OPEN_FILES{return}let mut state=STATE.lock();for watch in &mut state.watches{if watch.used&&watch.fd==fd_id{*watch=EMPTY_WATCH}}
 state.buffers[i]=[0;BUF_SZ];state.lengths[i]=0;state.overflow[i]=false;
}
pub fn notify(path:&str,mask:u32){
 let(parent,name)=parent_of(path);let mut state=STATE.lock();
 for i in 0..MAX_WATCHES{let watch=state.watches[i];if !watch.used||watch.mask&mask&EVENT_MASK==0{continue}
  let watched=core::str::from_utf8(&watch.path[..watch.plen]).unwrap_or("");let(event_name,event_mask)=if watched==parent{(name,mask)}else if watched==path{("",mask)}else{continue};
  if queue_event(&mut state,watch.fd,watch.wd,event_mask,0,event_name)&&watch.mask&IN_ONESHOT!=0{state.watches[i]=EMPTY_WATCH;queue_event(&mut state,watch.fd,watch.wd,IN_IGNORED,0,"");}
 }
}
pub fn read(fd_id:u32,dst:&mut[u8])->Result<usize,i64>{
 if fd_id as usize>=MAX_OPEN_FILES{return Err(errno::EBADF)}if dst.is_empty(){return Ok(0)}
 let mut state=STATE.lock();let i=fd_id as usize;let available=state.lengths[i];if available==0{return Err(errno::EAGAIN)}
 let mut copied=0usize;
 while copied<available{
  if available-copied<16{return Err(errno::EIO)}let name_len=u32::from_ne_bytes(state.buffers[i][copied+12..copied+16].try_into().unwrap())as usize;let record=16usize.checked_add(name_len).ok_or(errno::EIO)?;
  if record>available-copied{return Err(errno::EIO)}if copied+record>dst.len(){if copied==0{return Err(errno::EINVAL)}break}
  dst[copied..copied+record].copy_from_slice(&state.buffers[i][copied..copied+record]);copied+=record;
 }
 let remain=available-copied;if remain>0{state.buffers[i].copy_within(copied..available,0)}state.lengths[i]=remain;queue_overflow(&mut state,fd_id);Ok(copied)
}
