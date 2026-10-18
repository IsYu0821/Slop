//! AF_UNIX stream sockets: bind/listen/connect/accept over a fixed table.
//! Each endpoint owns a ring buffer its peer writes into; read and write
//! block on wait queues exactly like pipes, with EOF when the peer closes.
use crate::{errno,sync::SpinLock,wait_queue::WaitQueue};
pub const MAX_SOCKETS:usize=16;
/* AF_INET endpoints are dispatched by id range: 16..48 hand off to net.rs. */
pub const INET_UDP_BASE:u8=16;pub const INET_TCP_BASE:u8=32;
pub fn inet_kind(id:u8)->Option<bool>{
 if id>=INET_TCP_BASE{Some(true)}else if id>=INET_UDP_BASE{Some(false)}else{None}
}pub const CAPACITY:usize=4096;pub const PATH_MAX:usize=108;
pub const SOCK_STREAM:u32=1;pub const SOCK_DGRAM:u32=2;
#[derive(Clone,Copy,PartialEq)]pub enum SockState{Unbound,Listening,Connected}
#[derive(Clone,Copy,PartialEq)]pub enum SockKind{Stream,Dgram}
struct Socket{
 used:bool,kind:SockKind,state:SockState,
 path:[u8;PATH_MAX],path_len:usize,
 peer:Option<u8>,
 buf:[u8;CAPACITY],head:usize,len:usize,
 peer_closed:bool,
 readq:WaitQueue,writeq:WaitQueue,
 acceptq:WaitQueue,
}
impl Socket{
 const fn new()->Self{Self{used:false,kind:SockKind::Stream,state:SockState::Unbound,path:[0;PATH_MAX],path_len:0,peer:None,buf:[0;CAPACITY],head:0,len:0,peer_closed:false,readq:WaitQueue::new(),writeq:WaitQueue::new(),acceptq:WaitQueue::new()}}
 fn path_str(&self)->&str{core::str::from_utf8(&self.path[..self.path_len]).unwrap_or("")}
 fn free(&mut self){*self=Socket::new();}
}
static SOCKS:[SpinLock<Socket>;MAX_SOCKETS]=[const{SpinLock::new(Socket::new())};MAX_SOCKETS];
/* Server-side pending-connection requests: (requester socket, requester pid). */
static PENDING:[SpinLock<[(Option<u8>,Option<u8>);16]>;MAX_SOCKETS]=[const{SpinLock::new([(None,None);16])};MAX_SOCKETS];
fn valid(id:u8)->bool{(id as usize)<MAX_SOCKETS}
pub fn socket(kind:u32)->Result<u8,i64>{
 let sk=match kind{SOCK_STREAM=>SockKind::Stream,SOCK_DGRAM=>SockKind::Dgram,_=>return Err(errno::EINVAL)};
 for i in 0..MAX_SOCKETS{
  let mut s=SOCKS[i].lock();
  if !s.used{s.used=true;s.kind=sk;s.state=SockState::Unbound;return Ok(i as u8)}
 }
 Err(errno::EMFILE)
}
pub fn kind_of(id:u8)->Option<SockKind>{if !valid(id){return None}Some(SOCKS[id as usize].lock().kind)}
pub fn bind(id:u8,path:&str)->Result<(),i64>{
 if !valid(id){return Err(errno::EBADF)}
 if path.is_empty()||path.len()>=PATH_MAX{return Err(errno::EINVAL)}
 for i in 0..MAX_SOCKETS{
  let s=SOCKS[i].lock();
  if s.used&&s.path_len==path.len()&&s.path_str()==path{return Err(errno::EADDRINUSE)}
 }
 let mut s=SOCKS[id as usize].lock();
 let b=path.as_bytes();s.path[..b.len()].copy_from_slice(b);s.path_len=b.len();
 s.state=SockState::Unbound;
 Ok(())
}
pub fn listen(id:u8)->Result<(),i64>{
 if !valid(id){return Err(errno::EBADF)}
 let mut s=SOCKS[id as usize].lock();
 if s.path_len==0{return Err(errno::EINVAL)}
 s.state=SockState::Listening;
 Ok(())
}
/* Finds the listening socket bound to path. */
fn find_listener(path:&str)->Option<u8>{
 for i in 0..MAX_SOCKETS{
  let s=SOCKS[i].lock();
  if s.used&&s.state==SockState::Listening&&s.path_len==path.len()&&s.path_str()==path{return Some(i as u8)}
 }
 None
}
pub fn connect(id:u8,path:&str)->Result<(),i64>{
 if !valid(id){return Err(errno::EBADF)}
 let listener=match find_listener(path){Some(l)=>l,None=>return Err(errno::ECONNREFUSED)};
 /* Establish the pair immediately so the client can send before the server
    accepts; accept hands the already-connected endpoint over. */
 let server=match socket(SOCK_STREAM){Ok(s)=>s,Err(e)=>return Err(e)};
 {
  let mut c=SOCKS[id as usize].lock();
  let mut sv=SOCKS[server as usize].lock();
  c.peer=Some(server);sv.peer=Some(id);
  c.state=SockState::Connected;sv.state=SockState::Connected;
 }
 let mut q=PENDING[listener as usize].lock();
 for slot in q.iter_mut(){
  if slot.0.is_none(){*slot=(Some(id),Some(server));drop(q);SOCKS[listener as usize].lock().acceptq.wake_all();return Ok(())}
 }
 drop(q);
 let _=server;
 Err(errno::ECONNREFUSED)
}
/* Takes the first pending request, creating a connected pair for it.
   Blocks while the queue is empty, mirroring a listening socket with no
   connection yet; the caller passes false for non-blocking sockets. */
pub fn accept(listener:u8,nonblocking:bool)->Result<u8,i64>{
 if !valid(listener){return Err(errno::EBADF)}
 loop{
  let requester={
   let mut q=PENDING[listener as usize].lock();
   match q.iter_mut().find(|s|s.0.is_some()){
    Some(slot)=>{let c=slot.0.take();let sv=slot.1.take();*slot=(None,None);(c,sv)},
    None=>(None,None),
   }
  };
  if let (Some(client),Some(server))=requester{
   {
    let mut sv=SOCKS[server as usize].lock();
    let c=SOCKS[client as usize].lock();
    sv.path[..c.path_len.min(PATH_MAX)].copy_from_slice(&c.path[..c.path_len.min(PATH_MAX)]);
   }
   return Ok(server);
  }
    if nonblocking{return Err(errno::EAGAIN)}
  /* Register before re-checking so a connect that lands between the check
     and the sleep cannot be lost. */
  let registered={let s=SOCKS[listener as usize].lock();s.acceptq.register().is_ok()};
  let pending=PENDING[listener as usize].lock().iter().any(|s|s.0.is_some());
  if pending{continue}
  if !registered{return Err(errno::EAGAIN)}
  crate::task::block_current();
 }
}
/* True when a pending connection request is waiting. */
pub fn has_pending(listener:u8)->bool{
 if !valid(listener){return false}
 PENDING[listener as usize].lock().iter().any(|s|s.0.is_some())
}
/* Creates a pre-connected pair for socketpair. */
pub fn socketpair()->Result<(u8,u8),i64>{
 let a=socket(SOCK_STREAM)?;let b=socket(SOCK_STREAM)?;
 {
  let mut sa=SOCKS[a as usize].lock();let mut sb=SOCKS[b as usize].lock();
  sa.peer=Some(b);sb.peer=Some(a);
  sa.state=SockState::Connected;sb.state=SockState::Connected;
 }
 Ok((a,b))
}
pub fn send(id:u8,data:&[u8])->Result<usize,i64>{
 if let Some(tcp)=inet_kind(id){
  return if tcp{crate::net::tcp_send(id-INET_TCP_BASE,data)}else{crate::net::udp_send(id-INET_UDP_BASE,None,data)};
 }
 if !valid(id){return Err(errno::EBADF)}
 let peer=match SOCKS[id as usize].lock().peer{Some(p)=>p,None=>return Err(errno::ENOTCONN)};
  let mut done=0usize;
 while done<data.len(){
  let registered={
   let mut p=SOCKS[peer as usize].lock();
   if p.peer_closed{return if done>0{Ok(done)}else{Err(errno::EPIPE)}}
   if p.len<CAPACITY{
    let n=core::cmp::min(CAPACITY-p.len,data.len()-done);
    let base=p.head+p.len;
    for i in 0..n{p.buf[(base+i)%CAPACITY]=data[done+i];}
    p.len+=n;done+=n;p.readq.wake_all();
    false
   }else{p.writeq.register().is_ok()}
  };
  if done==data.len(){break}
  if !registered{return Err(errno::EAGAIN)}
  crate::task::block_current();
 }
 Ok(done)
}
pub fn recv(id:u8,dst:&mut[u8])->Result<usize,i64>{
 if let Some(tcp)=inet_kind(id){
  return if tcp{crate::net::tcp_recv(id-INET_TCP_BASE,dst)}else{crate::net::udp_recv(id-INET_UDP_BASE,dst).map(|(n,_,_)|n)};
 }
 if dst.is_empty(){return Ok(0)}
 loop{
  let registered={
   let mut s=SOCKS[id as usize].lock();
      if s.len>0{
    let n=core::cmp::min(s.len,dst.len());
    for i in 0..n{dst[i]=s.buf[s.head];s.head=(s.head+1)%CAPACITY;}
    s.len-=n;s.writeq.wake_all();
    return Ok(n);
   }
   if s.peer_closed{return Ok(0)}
   s.readq.register().is_ok()
  };
  if !registered{return Err(errno::EAGAIN)}
  crate::task::block_current();
 }
}
/* Half-closes: the peer observes EOF while this end may still read. */
pub fn shutdown(id:u8)->Result<(),i64>{
 if let Some(tcp)=inet_kind(id){
  return if tcp{crate::net::tcp_shutdown(id-INET_TCP_BASE,1)}else{Ok(())};
 }
 if !valid(id){return Err(errno::EBADF)}
 let peer=SOCKS[id as usize].lock().peer;
 SOCKS[id as usize].lock().peer_closed=true;
 if let Some(p)=peer{
  let mut ps=SOCKS[p as usize].lock();
  ps.peer_closed=true;
  ps.readq.wake_all();
 }
 Ok(())
}
pub fn close(id:u8){
 if let Some(tcp)=inet_kind(id){
  if tcp{crate::net::tcp_close(id-INET_TCP_BASE)}else{crate::net::udp_close(id-INET_UDP_BASE)};
  return;
 }
 if !valid(id){return}
 let mut s=SOCKS[id as usize].lock();
 if !s.used{return}
 let peer=s.peer;
 s.free();
 drop(s);
 if let Some(p)=peer{
  let mut ps=SOCKS[p as usize].lock();
  ps.peer_closed=true;
  ps.readq.wake_all();
 }
}
pub fn path_of(id:u8)->Result<[u8;PATH_MAX],i64>{
 if !valid(id){return Err(errno::EBADF)}
 let s=SOCKS[id as usize].lock();
 Ok(s.path)
}
pub fn path_len_of(id:u8)->usize{SOCKS[id as usize].lock().path_len}
pub fn is_sockaddr_un(family:u16)->bool{family==1}
