//! AF_UNIX sockets: stream (bind/listen/connect/accept) and datagram
//! (bind/sendto/recvfrom, connected or unconnected) over a fixed table.
//! Stream endpoints own a ring buffer its peer writes into; read and write
//! block on wait queues exactly like pipes, with EOF when the peer closes.
//! Datagram endpoints own a bounded message queue; each message carries its
//! sender's bound path so recvfrom can report it like Linux. Paths are raw
//! bytes to cover filesystem names and abstract names (leading NUL).
use crate::{errno,sync::SpinLock,wait_queue::WaitQueue};
pub const MAX_SOCKETS:usize=16;
/* AF_INET endpoints are dispatched by id range: 16..48 hand off to net.rs. */
pub const INET_UDP_BASE:u8=16;pub const INET_TCP_BASE:u8=32;
pub fn inet_kind(id:u8)->Option<bool>{
 if id>=INET_TCP_BASE{Some(true)}else if id>=INET_UDP_BASE{Some(false)}else{None}
}pub const CAPACITY:usize=4096;pub const PATH_MAX:usize=108;
pub const SOCK_STREAM:u32=1;pub const SOCK_DGRAM:u32=2;
pub const DGRAM_DATA_MAX:usize=4096;pub const DGRAM_QUEUE:usize=8;
#[derive(Clone,Copy,PartialEq)]pub enum SockState{Unbound,Listening,Connected}
#[derive(Clone,Copy,PartialEq)]pub enum SockKind{Stream,Dgram}
#[derive(Clone,Copy)]struct DgramMsg{len:usize,src_len:usize,src:[u8;PATH_MAX],data:[u8;DGRAM_DATA_MAX],
 /* SCM_RIGHTS: global fd-table ids (incref'd at send, installed or released
    at recv) plus the sender's credentials. Honest subset: at most
    ANC_FDS_MAX descriptors per message; more is rejected at send. */
 nfds:usize,fds:[u32;ANC_FDS_MAX],cred:bool,cred_pid:u32,cred_uid:u32,cred_gid:u32}
pub const ANC_FDS_MAX:usize=8;
pub const ANC_QUEUE:usize=8;
#[derive(Clone,Copy)]pub struct AncData{pub nfds:usize,pub fds:[u32;ANC_FDS_MAX],
 pub cred:bool,pub cred_pid:u32,pub cred_uid:u32,pub cred_gid:u32}
impl AncData{pub const fn empty()->Self{Self{nfds:0,fds:[0;ANC_FDS_MAX],cred:false,cred_pid:0,cred_uid:0,cred_gid:0}}}
/* Stream ancillary item: byte-stream position the data belongs to plus payload. */
#[derive(Clone,Copy)]struct AncItem{pos:u64,data:AncData}
impl AncItem{const fn empty()->Self{Self{pos:0,data:AncData::empty()}}}
impl DgramMsg{const fn empty()->Self{Self{len:0,src_len:0,src:[0;PATH_MAX],data:[0;DGRAM_DATA_MAX],nfds:0,fds:[0;ANC_FDS_MAX],cred:false,cred_pid:0,cred_uid:0,cred_gid:0}}}
struct Socket{
 used:bool,kind:SockKind,state:SockState,
 path:[u8;PATH_MAX],path_len:usize,
 peer:Option<u8>,
 buf:[u8;CAPACITY],head:usize,len:usize,
 peer_closed:bool,
 dgram_peer:[u8;PATH_MAX],dgram_peer_len:usize,has_dgram_peer:bool,
 dgram_q:[DgramMsg;DGRAM_QUEUE],dgram_head:usize,dgram_len:usize,
 readq:WaitQueue,writeq:WaitQueue,
 acceptq:WaitQueue,
 /* Byte-stream accounting for SCM placement (stream only). write_total is
    the sender-side count of bytes ever queued; read_total the receiver-side
    count of bytes ever consumed. An anc item rides with the bytes written
    at send time and is delivered to the recvmsg whose byte range covers it. */
 write_total:u64,read_total:u64,
 anc_q:[AncItem;ANC_QUEUE],anc_head:usize,anc_len:usize,
 passcred:bool,
}
impl Socket{
 const fn new()->Self{Self{used:false,kind:SockKind::Stream,state:SockState::Unbound,path:[0;PATH_MAX],path_len:0,peer:None,buf:[0;CAPACITY],head:0,len:0,peer_closed:false,dgram_peer:[0;PATH_MAX],dgram_peer_len:0,has_dgram_peer:false,dgram_q:[DgramMsg::empty();DGRAM_QUEUE],dgram_head:0,dgram_len:0,readq:WaitQueue::new(),writeq:WaitQueue::new(),acceptq:WaitQueue::new(),write_total:0,read_total:0,anc_q:[AncItem::empty();ANC_QUEUE],anc_head:0,anc_len:0,passcred:false}}
 fn path_bytes(&self)->&[u8]{&self.path[..self.path_len]}
 fn free(&mut self){
  /* Release any in-flight passed fds so they do not leak when the socket
     dies with undelivered ancillary data. */
  for i in 0..self.anc_len{
   let it=self.anc_q[(self.anc_head+i)%ANC_QUEUE];
   for j in 0..it.data.nfds{crate::fd::release(it.data.fds[j]);}
  }
  for i in 0..self.dgram_len{
   let m=self.dgram_q[(self.dgram_head+i)%DGRAM_QUEUE];
   for j in 0..m.nfds{crate::fd::release(m.fds[j]);}
  }
  *self=Socket::new();
 }
}
/* SO_PASSCRED flag for credential delivery on recvmsg. */
pub fn set_passcred(id:u8,on:bool)->Result<(),i64>{
 if !valid(id){return Err(errno::EBADF)}
 let mut s=SOCKS[id as usize].lock();
 if !s.used{return Err(errno::EBADF)}
 s.passcred=on;Ok(())
}
pub fn get_passcred(id:u8)->Result<bool,i64>{
 if !valid(id){return Err(errno::EBADF)}
 let s=SOCKS[id as usize].lock();
 if !s.used{return Err(errno::EBADF)}
 Ok(s.passcred)
}
/* Releases one anc payload (used when the receiver cannot take it). */
pub fn anc_release(a:&AncData){for j in 0..a.nfds{crate::fd::release(a.fds[j]);}}
static SOCKS:[SpinLock<Socket>;MAX_SOCKETS]=[const{SpinLock::new(Socket::new())};MAX_SOCKETS];
/* Server-side pending-connection requests: (requester socket, requester pid). */
static PENDING:[SpinLock<[(Option<u8>,Option<u8>);16]>;MAX_SOCKETS]=[const{SpinLock::new([(None,None);16])};MAX_SOCKETS];
fn valid(id:u8)->bool{(id as usize)<MAX_SOCKETS}
pub fn socket(kind:u32)->Result<u8,i64>{
 let sk=match kind{SOCK_STREAM=>SockKind::Stream,SOCK_DGRAM=>SockKind::Dgram,_=>return Err(errno::EINVAL)};
 for i in 0..MAX_SOCKETS{
  let mut s=SOCKS[i].lock();
  /* Fully reset: a reused slot must not inherit peer_closed, buffers or
     waiters from its previous incarnation (stale peer_closed caused
     EPIPE on fresh socketpairs). */
  if !s.used{*s=Socket::new();s.used=true;s.kind=sk;s.state=SockState::Unbound;*PENDING[i as usize].lock()=[(None,None);16];return Ok(i as u8)}
 }
 Err(errno::EMFILE)
}
pub fn kind_of(id:u8)->Option<SockKind>{if !valid(id){return None}Some(SOCKS[id as usize].lock().kind)}
fn path_eq(a:&[u8],b:&[u8])->bool{a.len()==b.len()&&a==b}
pub fn bind(id:u8,path:&str)->Result<(),i64>{bind_bytes(id,path.as_bytes())}
pub fn bind_bytes(id:u8,raw:&[u8])->Result<(),i64>{
 if !valid(id){return Err(errno::EBADF)}
 if raw.is_empty()||raw.len()>=PATH_MAX{return Err(errno::EINVAL)}
 {
  let me=SOCKS[id as usize].lock();
  if !me.used{return Err(errno::EBADF)}
  if me.path_len!=0{return Err(errno::EINVAL)}
 }
 for i in 0..MAX_SOCKETS{
  let s=SOCKS[i].lock();
  if s.used&&path_eq(s.path_bytes(),raw){return Err(errno::EADDRINUSE)}
 }
 let mut s=SOCKS[id as usize].lock();
 s.path[..raw.len()].copy_from_slice(raw);s.path_len=raw.len();
 s.state=SockState::Unbound;
 Ok(())
}
pub fn listen(id:u8)->Result<(),i64>{
 if !valid(id){return Err(errno::EBADF)}
 let mut s=SOCKS[id as usize].lock();
 if !s.used{return Err(errno::EBADF)}
 if s.kind!=SockKind::Stream{return Err(errno::EOPNOTSUPP)}
 if s.path_len==0{return Err(errno::EINVAL)}
 s.state=SockState::Listening;
 Ok(())
}
/* Finds the listening stream socket bound to path. */
fn find_listener(path:&[u8])->Option<u8>{
 for i in 0..MAX_SOCKETS{
  let s=SOCKS[i].lock();
  if s.used&&s.kind==SockKind::Stream&&s.state==SockState::Listening&&path_eq(s.path_bytes(),path){return Some(i as u8)}
 }
 None
}
/* Finds any bound socket (stream or datagram) at path. */
fn find_bound(path:&[u8])->Option<u8>{
 for i in 0..MAX_SOCKETS{
  let s=SOCKS[i].lock();
  if s.used&&s.path_len!=0&&path_eq(s.path_bytes(),path){return Some(i as u8)}
 }
 None
}
fn find_bound_dgram(path:&[u8])->Option<u8>{
 for i in 0..MAX_SOCKETS{
  let s=SOCKS[i].lock();
  if s.used&&s.kind==SockKind::Dgram&&s.path_len!=0&&path_eq(s.path_bytes(),path){return Some(i as u8)}
 }
 None
}
pub fn connect(id:u8,path:&str)->Result<(),i64>{connect_bytes(id,path.as_bytes())}
pub fn connect_bytes(id:u8,raw:&[u8])->Result<(),i64>{
 if !valid(id){return Err(errno::EBADF)}
 let kind=match kind_of(id){Some(k)=>k,None=>return Err(errno::EBADF)};
 if kind==SockKind::Dgram{return dgram_connect(id,raw)}
 let listener=match find_listener(raw){Some(l)=>l,None=>return Err(errno::ECONNREFUSED)};
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
pub const SOCK_NONBLOCK:u32=0o4000;
pub const SOCK_CLOEXEC:u32=0o2000000;
pub fn accept(listener:u8,nonblocking:bool)->Result<u8,i64>{
 if !valid(listener){return Err(errno::EBADF)}
 {
  let s=SOCKS[listener as usize].lock();
  if !s.used{return Err(errno::EBADF)}
  if s.kind!=SockKind::Stream{return Err(errno::EOPNOTSUPP)}
  if s.state!=SockState::Listening{return Err(errno::EINVAL)}
 }
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
    let n=c.path_len.min(PATH_MAX);
    sv.path[..n].copy_from_slice(&c.path[..n]);
    sv.path_len=n;
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
pub fn socketpair()->Result<(u8,u8),i64>{socketpair_kind(SOCK_STREAM)}
pub fn socketpair_kind(kind:u32)->Result<(u8,u8),i64>{
 let sk=match kind{SOCK_STREAM=>SockKind::Stream,SOCK_DGRAM=>SockKind::Dgram,_=>return Err(errno::EINVAL)};
 let a=socket(kind)?;let b=socket(kind)?;
 {
  let mut sa=SOCKS[a as usize].lock();let mut sb=SOCKS[b as usize].lock();
  if sk==SockKind::Stream{
   sa.peer=Some(b);sb.peer=Some(a);
   sa.state=SockState::Connected;sb.state=SockState::Connected;
  }else{
   /* Datagram pairs stay unbound but connected to each other: each end
      reports the other as its peer through an internal marker. Linux
      socketpair(AF_UNIX, SOCK_DGRAM) behaves as connected sockets. */
   sa.state=SockState::Connected;sb.state=SockState::Connected;
   sa.peer=Some(b);sb.peer=Some(a);
  }
 }
 Ok((a,b))
}
/* Datagram connect: records the destination path. Linux requires the target
   to exist for pathname addresses; abstract targets may be created later, so
   only pathname targets are checked. */
pub fn dgram_connect(id:u8,raw:&[u8])->Result<(),i64>{
 if !valid(id){return Err(errno::EBADF)}
 if raw.len()>=PATH_MAX{return Err(errno::EINVAL)}
 let is_abstract=!raw.is_empty()&&raw[0]==0;
 if !is_abstract&&!raw.is_empty()&&find_bound_dgram(raw).is_none()&&find_bound(raw).is_none(){
  return Err(errno::ENOENT);
 }
 let mut s=SOCKS[id as usize].lock();
 if !s.used{return Err(errno::EBADF)}
 if s.kind!=SockKind::Dgram{return Err(errno::EINVAL)}
 s.dgram_peer[..raw.len()].copy_from_slice(raw);
 s.dgram_peer_len=raw.len();s.has_dgram_peer=!raw.is_empty();
 s.state=SockState::Connected;
 Ok(())
}
fn dgram_enqueue_anc(dest:u8,src_path:&[u8],data:&[u8],nonblock:bool,mut anc:AncData)->Result<usize,i64>{
 if data.len()>DGRAM_DATA_MAX{anc_release(&anc);return Err(errno::EMSGSIZE)}
 loop{
  let registered={
   let mut d=SOCKS[dest as usize].lock();
   if !d.used{anc_release(&anc);return Err(errno::ENOENT)}
   if d.kind!=SockKind::Dgram{anc_release(&anc);return Err(errno::ECONNREFUSED)}
   if d.dgram_len<DGRAM_QUEUE{
    let idx=(d.dgram_head+d.dgram_len)%DGRAM_QUEUE;
    d.dgram_q[idx].len=data.len();
    d.dgram_q[idx].data[..data.len()].copy_from_slice(data);
    let n=src_path.len().min(PATH_MAX);
    d.dgram_q[idx].src[..n].copy_from_slice(&src_path[..n]);
    d.dgram_q[idx].src_len=n;
    /* Kernel-verified sender identity (never the sender's bytes). */
    let (pid,uid,gid)=crate::task::with_current_process(|p|(crate::task::current_pid(),p.euid,p.egid)).unwrap_or((0,0,0));
    d.dgram_q[idx].nfds=anc.nfds;
    d.dgram_q[idx].fds=anc.fds;
    d.dgram_q[idx].cred=anc.cred||d.passcred;
    d.dgram_q[idx].cred_pid=pid;
    d.dgram_q[idx].cred_uid=uid;
    d.dgram_q[idx].cred_gid=gid;
    d.dgram_len+=1;
    d.readq.wake_all();
    return Ok(data.len());
   }
   if nonblock{anc_release(&anc);return Err(errno::EAGAIN)}
   d.writeq.register().is_ok()
  };
  if !registered{anc_release(&anc);return Err(errno::EAGAIN)}
  crate::task::block_current();
 }
}
fn dgram_enqueue(dest:u8,src_path:&[u8],data:&[u8],nonblock:bool)->Result<usize,i64>{
 dgram_enqueue_anc(dest,src_path,data,nonblock,AncData::empty())
}
pub fn dgram_sendto(from:u8,dest_path:&[u8],data:&[u8],nonblock:bool)->Result<usize,i64>{
 if !valid(from){return Err(errno::EBADF)}
 if dest_path.len()>=PATH_MAX{return Err(errno::EINVAL)}
 let dest=match find_bound_dgram(dest_path){
  Some(d)=>d,
  None=>{
   /* A stream listener at the path is not a datagram peer. */
   if find_bound(dest_path).is_some(){return Err(errno::ECONNREFUSED)}
   return Err(errno::ENOENT);
  }
 };
 let src_len=SOCKS[from as usize].lock().path_len;
 let mut src=[0u8;PATH_MAX];
 if src_len>0{src[..src_len].copy_from_slice(&SOCKS[from as usize].lock().path[..src_len]);}
 dgram_enqueue(dest,&src[..src_len],data,nonblock)
}
pub fn dgram_recvfrom(id:u8,dst:&mut[u8],nonblock:bool)->Result<(usize,[u8;PATH_MAX],usize),i64>{
 dgram_recvfrom_anc(id,dst,nonblock).map(|(n,s,sl,_)| (n,s,sl))
}
pub fn dgram_recvfrom_anc(id:u8,dst:&mut[u8],nonblock:bool)->Result<(usize,[u8;PATH_MAX],usize,AncData),i64>{
 if !valid(id){return Err(errno::EBADF)}
 loop{
  let registered={
   let mut s=SOCKS[id as usize].lock();
   if !s.used{return Err(errno::EBADF)}
   if s.kind!=SockKind::Dgram{return Err(errno::EINVAL)}
   if s.dgram_len>0{
    let msg=s.dgram_q[s.dgram_head];
    let n=core::cmp::min(msg.len,dst.len());
    dst[..n].copy_from_slice(&msg.data[..n]);
    let mut src=[0u8;PATH_MAX];
    src[..msg.src_len].copy_from_slice(&msg.src[..msg.src_len]);
    let sl=msg.src_len;
    let anc=AncData{nfds:msg.nfds,fds:msg.fds,cred:msg.cred||s.passcred,cred_pid:msg.cred_pid,cred_uid:msg.cred_uid,cred_gid:msg.cred_gid};
    s.dgram_head=(s.dgram_head+1)%DGRAM_QUEUE;
    s.dgram_len-=1;
    s.writeq.wake_all();
    return Ok((n,src,sl,anc));
   }
   if nonblock{return Err(errno::EAGAIN)}
   s.readq.register().is_ok()
  };
  if !registered{return Err(errno::EAGAIN)}
  crate::task::block_current();
 }
}
pub fn dgram_has_data(id:u8)->bool{
 if !valid(id){return false}
 SOCKS[id as usize].lock().dgram_len>0
}
pub fn dgram_peer_of(id:u8)->Result<([u8;PATH_MAX],usize),i64>{
 if !valid(id){return Err(errno::EBADF)}
 let s=SOCKS[id as usize].lock();
 if s.kind!=SockKind::Dgram||!s.has_dgram_peer{return Err(errno::ENOTCONN)}
 Ok((s.dgram_peer,s.dgram_peer_len))
}
/* Stream send with ancillary data. Bytes flow into the peer's ring as usual;
   the anc item is stamped with the post-send stream position so the recvmsg
   whose byte range covers it receives the fds/creds. Anc-queue-full blocks
   like a full byte buffer (the receiver drains it by reading, which wakes
   the peer writeq). */
pub fn send_anc(id:u8,data:&[u8],nonblock:bool,mut anc:AncData)->Result<usize,i64>{
 if let Some(tcp)=inet_kind(id){
  anc_release(&anc);
  return if tcp{crate::net::tcp_send(id-INET_TCP_BASE,data)}else{crate::net::udp_send(id-INET_UDP_BASE,None,data)};
 }
 if !valid(id){anc_release(&anc);return Err(errno::EBADF)}
 let kind=match kind_of(id){Some(k)=>k,None=>{anc_release(&anc);return Err(errno::EBADF)}};
 if kind==SockKind::Dgram{
  /* Connected datagram send with anc. */
  let peer_direct=SOCKS[id as usize].lock().peer;
  let (has_named,peer_path,peer_len)={
   let s=SOCKS[id as usize].lock();
   (s.has_dgram_peer,s.dgram_peer,s.dgram_peer_len)
  };
  if has_named{
   let dest_path=[peer_path];
   return dgram_send_anc_to(id,&dest_path[0][..peer_len],data,nonblock,anc);
  }
  if let Some(p)=peer_direct{
   let src_len=SOCKS[id as usize].lock().path_len;
   let mut src=[0u8;PATH_MAX];
   if src_len>0{src[..src_len].copy_from_slice(&SOCKS[id as usize].lock().path[..src_len]);}
   return dgram_enqueue_anc(p,&src[..src_len],data,nonblock,anc);
  }
  anc_release(&anc);
  return Err(errno::ENOTCONN);
 }
 let peer=match SOCKS[id as usize].lock().peer{Some(p)=>p,None=>{anc_release(&anc);return Err(errno::ENOTCONN)}};
 /* Fast path only when there is truly nothing to attach AND the peer does
    not want credentials (SO_PASSCRED auto-attaches even to plain sends). */
 if anc.nfds==0&&!anc.cred&&!SOCKS[peer as usize].lock().passcred{
  return send(id,data,nonblock);
 }
 /* Ancillary path: reserve a slot first so the byte write and the stamp are
    consistent, then write bytes, then publish the item. */
 loop{
  let slot_ok={
   let p=SOCKS[peer as usize].lock();
   if p.peer_closed{return Err(errno::EPIPE)}
   p.anc_len<ANC_QUEUE
  };
  if slot_ok{break}
  if nonblock{anc_release(&anc);return Err(errno::EAGAIN)}
  let registered={SOCKS[peer as usize].lock().writeq.register().is_ok()};
  if !registered{anc_release(&anc);return Err(errno::EAGAIN)}
  crate::task::block_current();
 }
 let n=match send(id,data,nonblock){Ok(n)=>n,Err(e)=>{anc_release(&anc);return Err(e)}};
 {
  /* Kernel-verified sender identity (never the sender's bytes). */
  let (pid,uid,gid)=crate::task::with_current_process(|pr|(crate::task::current_pid(),pr.euid,pr.egid)).unwrap_or((0,0,0));
  anc.cred_pid=pid;anc.cred_uid=uid;anc.cred_gid=gid;
  let mut p=SOCKS[peer as usize].lock();
  anc.cred=anc.cred||p.passcred;
  /* Position AFTER the bytes just written; the receiver delivers this item
     to the first recvmsg whose consumed range reaches it. Zero-byte sends
     still advance nothing, so they are delivered on the next recvmsg. */
  let pos=p.write_total;
  if p.anc_len<ANC_QUEUE{
   let idx=(p.anc_head+p.anc_len)%ANC_QUEUE;
   p.anc_q[idx]=AncItem{pos,data:anc};
   p.anc_len+=1;
  }else{
   /* Raced full: drop (fds released, no leak) rather than misdeliver. */
   anc_release(&anc);
  }
 }
 Ok(n)
}
fn dgram_send_anc_to(from:u8,dest_path:&[u8],data:&[u8],nonblock:bool,anc:AncData)->Result<usize,i64>{
 if !valid(from){anc_release(&anc);return Err(errno::EBADF)}
 if dest_path.len()>=PATH_MAX{anc_release(&anc);return Err(errno::EINVAL)}
 let dest=match find_bound_dgram(dest_path){
  Some(d)=>d,
  None=>{
   anc_release(&anc);
   if find_bound(dest_path).is_some(){return Err(errno::ECONNREFUSED)}
   return Err(errno::ENOENT);
  }
 };
 let src_len=SOCKS[from as usize].lock().path_len;
 let mut src=[0u8;PATH_MAX];
 if src_len>0{src[..src_len].copy_from_slice(&SOCKS[from as usize].lock().path[..src_len]);}
 dgram_enqueue_anc(dest,&src[..src_len],data,nonblock,anc)
}
/* Stream recv with ancillary data. Returns (bytes, anc, anc_truncated).
   Covered anc items (pos <= new read_total) are merged into one payload;
   on fd overflow the excess ids are released and truncated=true. */
pub fn recv_anc(id:u8,dst:&mut[u8],nonblock:bool)->Result<(usize,AncData,bool),i64>{
 if let Some(tcp)=inet_kind(id){
  return if tcp{crate::net::tcp_recv(id-INET_TCP_BASE,dst).map(|n|(n,AncData::empty(),false))}else{crate::net::udp_recv(id-INET_UDP_BASE,dst).map(|(n,_,_)|(n,AncData::empty(),false))};
 }
 if dst.is_empty(){return Ok((0,AncData::empty(),false))}
 if matches!(kind_of(id),Some(SockKind::Dgram)){
  return dgram_recvfrom_anc(id,dst,nonblock).map(|(n,_,_,a)|(n,a,false));
 }
 loop{
  let registered={
   let mut s=SOCKS[id as usize].lock();
   if s.len>0{
    let n=core::cmp::min(s.len,dst.len());
    for i in 0..n{dst[i]=s.buf[s.head];s.head=(s.head+1)%CAPACITY;}
    s.len-=n;s.read_total+=n as u64;
    /* Collect covered anc items. */
    let mut out=AncData::empty();
    let mut truncated=false;
    while s.anc_len>0{
     let head=s.anc_q[s.anc_head];
     if head.pos>s.read_total{break}
     /* Merge, releasing on overflow. */
     for j in 0..head.data.nfds{
      if out.nfds<ANC_FDS_MAX{out.fds[out.nfds]=head.data.fds[j];out.nfds+=1;}
      else{crate::fd::release(head.data.fds[j]);truncated=true;}
     }
     if head.data.cred&&!out.cred{out.cred=true;out.cred_pid=head.data.cred_pid;out.cred_uid=head.data.cred_uid;out.cred_gid=head.data.cred_gid;}
     s.anc_head=(s.anc_head+1)%ANC_QUEUE;
     s.anc_len-=1;
    }
    if s.passcred&&!out.cred{
     /* SO_PASSCRED without sender creds: no sender identity is known for
        stream bytes already in flight, so only flag cred if an item
        carried it. (Dgram always knows its sender.) */
    }
    s.writeq.wake_all();
    return Ok((n,out,truncated));
   }
   /* No bytes but covered anc (zero-byte send): deliver without blocking. */
   if s.anc_len>0&&s.anc_q[s.anc_head].pos<=s.read_total{
    let mut out=AncData::empty();
    let mut truncated=false;
    while s.anc_len>0{
     let head=s.anc_q[s.anc_head];
     if head.pos>s.read_total{break}
     for j in 0..head.data.nfds{
      if out.nfds<ANC_FDS_MAX{out.fds[out.nfds]=head.data.fds[j];out.nfds+=1;}
      else{crate::fd::release(head.data.fds[j]);truncated=true;}
     }
     if head.data.cred&&!out.cred{out.cred=true;out.cred_pid=head.data.cred_pid;out.cred_uid=head.data.cred_uid;out.cred_gid=head.data.cred_gid;}
     s.anc_head=(s.anc_head+1)%ANC_QUEUE;
     s.anc_len-=1;
    }
    s.writeq.wake_all();
    return Ok((0,out,truncated));
   }
   if s.peer_closed{return Ok((0,AncData::empty(),false))}
   if nonblock{return Err(errno::EAGAIN)}
   s.readq.register().is_ok()
  };
  if !registered{return Err(errno::EAGAIN)}
  crate::task::block_current();
 }
}
pub fn send(id:u8,data:&[u8],nonblock:bool)->Result<usize,i64>{
 if let Some(tcp)=inet_kind(id){
  return if tcp{crate::net::tcp_send(id-INET_TCP_BASE,data)}else{crate::net::udp_send(id-INET_UDP_BASE,None,data)};
 }
 if !valid(id){return Err(errno::EBADF)}
 let kind=match kind_of(id){Some(k)=>k,None=>return Err(errno::EBADF)};
 if kind==SockKind::Dgram{
  /* Connected datagram send: either a named peer from connect(), or the
     anonymous peer from socketpair(). */
  let peer_direct=SOCKS[id as usize].lock().peer;
  let (has_named,peer_path,peer_len)={
   let s=SOCKS[id as usize].lock();
   (s.has_dgram_peer,s.dgram_peer,s.dgram_peer_len)
  };
  if has_named{
   let dest_path=[peer_path];
   return dgram_sendto(id,&dest_path[0][..peer_len],data,nonblock);
  }
  if let Some(p)=peer_direct{
   let src_len=SOCKS[id as usize].lock().path_len;
   let mut src=[0u8;PATH_MAX];
   if src_len>0{src[..src_len].copy_from_slice(&SOCKS[id as usize].lock().path[..src_len]);}
   return dgram_enqueue(p,&src[..src_len],data,nonblock);
  }
  return Err(errno::ENOTCONN);
 }
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
   }else if nonblock{return if done>0{Ok(done)}else{Err(errno::EAGAIN)}}else{p.writeq.register().is_ok()}
  };
  if done==data.len(){break}
  if !registered{return Err(errno::EAGAIN)}
   crate::task::block_current();
  }
  SOCKS[peer as usize].lock().write_total+=done as u64;
  Ok(done)
}
pub fn recv(id:u8,dst:&mut[u8],nonblock:bool)->Result<usize,i64>{
 if let Some(tcp)=inet_kind(id){
  return if tcp{crate::net::tcp_recv(id-INET_TCP_BASE,dst)}else{crate::net::udp_recv(id-INET_UDP_BASE,dst).map(|(n,_,_)|n)};
 }
 if dst.is_empty(){return Ok(0)}
 if matches!(kind_of(id),Some(SockKind::Dgram)){
  return dgram_recvfrom(id,dst,nonblock).map(|(n,_,_)|n);
 }
 loop{
  let registered={
   let mut s=SOCKS[id as usize].lock();
   if s.len>0{
     let n=core::cmp::min(s.len,dst.len());
     for i in 0..n{dst[i]=s.buf[s.head];s.head=(s.head+1)%CAPACITY;}
     s.len-=n;s.read_total+=n as u64;s.writeq.wake_all();
     return Ok(n);
    }
    if s.peer_closed{return Ok(0)}
    if nonblock{return Err(errno::EAGAIN)}
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
 if matches!(kind_of(id),Some(SockKind::Dgram)){return Ok(());}
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
  if tcp{crate::net::tcp_close(id-INET_TCP_BASE)}else{crate::net::udp_close(id-INET_TCP_BASE)};
  return;
 }
 if !valid(id){return};
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
/* Closes whatever a file description owns; called when its last reference
   goes away (close, exit, unshare), so INET sockets are freed too. */
pub fn close_by_id(id:u32){
 match crate::fd::kind(id){
  Some(crate::fd::OpenKind::Socket(s))=>close(s),
  _=>{},
 }
}
pub fn path_of(id:u8)->Result<[u8;PATH_MAX],i64>{
 if !valid(id){return Err(errno::EBADF)}
 let s=SOCKS[id as usize].lock();
 Ok(s.path)
}
pub fn path_len_of(id:u8)->usize{SOCKS[id as usize].lock().path_len}
pub fn peer_of(id:u8)->Option<u8>{if !valid(id){return None}SOCKS[id as usize].lock().peer}
pub fn is_sockaddr_un(family:u16)->bool{family==1}
