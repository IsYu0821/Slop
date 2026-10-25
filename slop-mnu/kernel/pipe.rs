//! Anonymous pipes: a fixed pool of ring buffers with blocking ends.
//! Readers and writers count open descriptions, giving Linux EOF/EPIPE behavior.
use crate::{sync::SpinLock,wait_queue::WaitQueue};
pub const CAPACITY:usize=4096;const MAX_PIPES:usize=16;
struct Pipe{used:bool,buf:[u8;CAPACITY],head:usize,len:usize,readers:u32,writers:u32,readq:WaitQueue,writeq:WaitQueue}
impl Pipe{
 const fn new()->Self{Self{used:false,buf:[0;CAPACITY],head:0,len:0,readers:0,writers:0,readq:WaitQueue::new(),writeq:WaitQueue::new()}}
 fn reset(&mut self){self.head=0;self.len=0;self.readers=0;self.writers=0;self.used=true;}
}
static PIPES:[SpinLock<Pipe>;MAX_PIPES]=[const{SpinLock::new(Pipe::new())};MAX_PIPES];
pub fn alloc()->Result<u8,i64>{for i in 0..MAX_PIPES{let mut p=PIPES[i].lock();if !p.used{p.reset();return Ok(i as u8)}}Err(crate::errno::EMFILE)}
pub fn add_reader(id:u8){PIPES[id as usize].lock().readers+=1}
pub fn add_writer(id:u8){PIPES[id as usize].lock().writers+=1}
pub fn close_reader(id:u8){let mut p=PIPES[id as usize].lock();p.readers=p.readers.saturating_sub(1);let empty=p.readers==0&&p.writers==0;if p.readers==0{p.writeq.wake_all();}if empty{p.used=false;}}
pub fn has_data(id:u8)->bool{PIPES[id as usize].lock().len>0}
pub fn has_space(id:u8)->bool{PIPES[id as usize].lock().len<CAPACITY}
pub fn close_writer(id:u8){let mut p=PIPES[id as usize].lock();p.writers=p.writers.saturating_sub(1);let empty=p.readers==0&&p.writers==0;if p.writers==0{p.readq.wake_all();}if empty{p.used=false;}}
fn copy_into(p:&mut Pipe,src:&[u8],n:usize){for i in 0..n{p.buf[(p.head+p.len+i)%CAPACITY]=src[i];}}
fn copy_out(p:&mut Pipe,dst:&mut[u8],n:usize){for i in 0..n{dst[i]=p.buf[p.head];p.head=(p.head+1)%CAPACITY;}p.len-=n;}
/* Blocking reads: EOF once every writer has closed, otherwise sleep on readq.
   The waiter is registered while the pipe lock is held, so a concurrent wake
   can never slip between the readiness check and the sleep. */
pub fn read(id:u8,dst:&mut[u8],nonblock:bool)->Result<usize,i64>{
 if dst.is_empty(){return Ok(0)}
 loop{
  let registered={let mut p=PIPES[id as usize].lock();
   if p.len>0{let n=core::cmp::min(p.len,dst.len());copy_out(&mut p,dst,n);p.writeq.wake_all();return Ok(n)}
   if p.writers==0{return Ok(0)}
   if nonblock{return Err(crate::errno::EAGAIN)}
   p.readq.register().is_ok()};
  if !registered{return Err(crate::errno::EAGAIN)}
  if crate::task::has_deliverable_signal(){return Err(crate::errno::EINTR)}
  crate::task::block_current();
 }
}
/* Non-consuming reads for tee(2): reports the bytes that a read would
   return without advancing the head, so the data stays queued. Writers are
   not woken because no space is freed; readers sleep on readq like read. */
pub fn peek(id:u8,dst:&mut[u8],nonblock:bool)->Result<usize,i64>{
 if dst.is_empty(){return Ok(0)}
 loop{
  let registered={let mut p=PIPES[id as usize].lock();
   if p.len>0{let n=core::cmp::min(p.len,dst.len());for i in 0..n{dst[i]=p.buf[(p.head+i)%CAPACITY];}return Ok(n)}
   if p.writers==0{return Ok(0)}
   if nonblock{return Err(crate::errno::EAGAIN)}
   p.readq.register().is_ok()};
  if !registered{return Err(crate::errno::EAGAIN)}
  if crate::task::has_deliverable_signal(){return Err(crate::errno::EINTR)}
  crate::task::block_current();
 }
}
/* Blocking writes: all bytes or an error; EPIPE when every reader has closed. */
pub fn write(id:u8,src:&[u8],nonblock:bool)->Result<usize,i64>{
 let mut done=0usize;
 while done<src.len(){
  let registered={let mut p=PIPES[id as usize].lock();
   if p.readers==0{let _=crate::signal::send_self(crate::signal::SIGPIPE);return if done>0{Ok(done)}else{Err(crate::errno::EPIPE)}}
   if p.len<CAPACITY{let n=core::cmp::min(CAPACITY-p.len,src.len()-done);copy_into(&mut p,&src[done..],n);p.len+=n;done+=n;p.readq.wake_all();false}else if nonblock{return if done>0{Ok(done)}else{Err(crate::errno::EAGAIN)}}else{p.writeq.register().is_ok()}};
  if done==src.len(){break}
  if !registered{return Err(crate::errno::EAGAIN)}
  if crate::task::has_deliverable_signal(){return Err(crate::errno::EINTR)}
  crate::task::block_current();
 }
 Ok(done)
}
