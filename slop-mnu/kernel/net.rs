//! Ethernet, ARP, IPv4, ICMP, UDP and a minimal TCP client over virtio-net.
//!
//! This is the native side of networking. kernel/socket.rs dispatches AF_INET
//! endpoints here by id range; the Linux ABI layer above it only parses
//! sockaddr structures. Delivery is poll based: the timer tick calls `poll`,
//! and every blocking wait loops `poll` plus a `sti;hlt;cli` nap so the tick
//! (which polls too) provides the wakeup without needing a device IRQ.
use crate::{errno,sync::SpinLock,wait_queue::WaitQueue};
use core::sync::atomic::{AtomicU32,Ordering};

pub const OUR_IP:u32=0x0A00_020F;  /* 10.0.2.15, QEMU user networking */
pub const GATEWAY:u32=0x0A00_0202; /* 10.0.2.2 */
const NETMASK:u32=0xFFFF_FF00;
const ETH_ARP:u16=0x0806;const ETH_IPV4:u16=0x0800;
const IP_ICMP:u8=1;const IP_TCP:u8=6;const IP_UDP:u8=17;
const MSS:usize=1400;
const BROADCAST:[u8;6]=[0xff;6];
/* Retransmission timeout in timer ticks (NS_PER_TICK is 1ms nominal). */
const RTO_TICKS:u64=50;

static IP_ID:AtomicU32=AtomicU32::new(1);
static EPHEMERAL:AtomicU32=AtomicU32::new(0xC350);

#[inline]fn be16(b:&[u8],o:usize)->u16{u16::from_be_bytes([b[o],b[o+1]])}
#[inline]fn be32(b:&[u8],o:usize)->u32{u32::from_be_bytes([b[o],b[o+1],b[o+2],b[o+3]])}
fn csum_fold(mut sum:u32)->u16{while sum>>16!=0{sum=(sum&0xffff)+(sum>>16)}!(sum as u16)}
fn csum_bytes(init:u32,data:&[u8])->u16{
 let mut sum=init;let mut i=0;
 while i+1<data.len(){sum+=be16(data,i)as u32;i+=2}
 if i<data.len(){sum+=(data[i]as u32)<<8}
 csum_fold(sum)
}
fn csum_pseudo(src:u32,dst:u32,proto:u8,len:usize,data:&[u8])->u16{
 let init=(src>>16)+(src&0xffff)+(dst>>16)+(dst&0xffff)+(proto as u32)+(len as u32);
 csum_bytes(init,data)
}
fn seq_gt(a:u32,b:u32)->bool{(a.wrapping_sub(b)as i32)>0}
fn seq_ge(a:u32,b:u32)->bool{a==b||seq_gt(a,b)}
#[inline]fn ticks()->u64{crate::arch::x86_64::apic::ticks()}
/* Puts the CPU to sleep for one tick; the timer handler polls the link, so
   data that arrives while napping is dispatched by the time we resume. */
fn idle_hlt(){unsafe{core::arch::asm!("sti","hlt","cli",options(nomem,nostack,preserves_flags));}}
fn nap(round:u32){
 for _ in 0..20_000{core::hint::spin_loop();}
 if round%8==7{idle_hlt();}
 /* Cooperative scheduling has no preemption inside one syscall, so a
    spinning waiter (connect/send/recv) would otherwise starve the peer
    task it is rendezvousing with. Yield regularly to let it run. */
 if round%16==15{crate::task::yield_now();}
}

/* ---------- link layer ---------- */

fn eth_send(ethertype:u16,dst:&[u8;6],payload:&[u8])->Result<(),i64>{
 let mut frame=[0u8;1600];
 if payload.len()+14>frame.len(){return Err(errno::EMSGSIZE)}
 frame[..6].copy_from_slice(dst);
 frame[6..12].copy_from_slice(&crate::driver::virtio_net::mac());
 frame[12..14].copy_from_slice(&ethertype.to_be_bytes());
 frame[14..14+payload.len()].copy_from_slice(payload);
 crate::driver::virtio_net::send_frame(&frame[..14+payload.len()])
}
fn ip_emit(proto:u8,dst_ip:u32,dst_mac:&[u8;6],payload:&[u8])->Result<(),i64>{
 let total=20+payload.len();
 if total>1500{return Err(errno::EMSGSIZE)}
 let mut pkt=[0u8;1500];
 pkt[0]=0x45;
 pkt[2..4].copy_from_slice(&(total as u16).to_be_bytes());
 let id=(IP_ID.fetch_add(1,Ordering::Relaxed)&0xffff)as u16;
 pkt[4..6].copy_from_slice(&id.to_be_bytes());
 pkt[6..8].copy_from_slice(&0x4000u16.to_be_bytes());
 pkt[8]=64;
 pkt[9]=proto;
 pkt[12..16].copy_from_slice(&OUR_IP.to_be_bytes());
 pkt[16..20].copy_from_slice(&dst_ip.to_be_bytes());
 pkt[20..total].copy_from_slice(payload);
 let h=csum_bytes(0,&pkt[..20]);
 pkt[10..12].copy_from_slice(&h.to_be_bytes());
 eth_send(ETH_IPV4,dst_mac,&pkt[..total])
}

/* ---------- ARP ---------- */

static ARP:SpinLock<[(u32,[u8;6]);4]>=SpinLock::new([(0,[0;6]);4]);
fn arp_lookup(ip:u32)->Option<[u8;6]>{
 if ip==0xFFFF_FFFF{return Some(BROADCAST)}
 ARP.lock().iter().find(|e|e.0==ip).map(|e|e.1)
}
fn arp_insert(ip:u32,mac:[u8;6]){
 if ip==0{return}
 let mut t=ARP.lock();
 if let Some(e)=t.iter_mut().find(|e|e.0==ip){e.1=mac;return}
 if let Some(e)=t.iter_mut().find(|e|e.0==0){e.0=ip;e.1=mac;return}
 t[0]=(ip,mac);
}
fn arp_emit(op:u16,target_ip:u32,dst_mac:&[u8;6],target_mac:&[u8;6]){
 let mut p=[0u8;28];
 p[0..2].copy_from_slice(&1u16.to_be_bytes());
 p[2..4].copy_from_slice(&ETH_IPV4.to_be_bytes());
 p[4]=6;p[5]=4;
 p[6..8].copy_from_slice(&op.to_be_bytes());
 p[8..14].copy_from_slice(&crate::driver::virtio_net::mac());
 p[14..18].copy_from_slice(&OUR_IP.to_be_bytes());
 p[18..24].copy_from_slice(target_mac);
 p[24..28].copy_from_slice(&target_ip.to_be_bytes());
 let _=eth_send(ETH_ARP,dst_mac,&p);
}
fn arp_request(ip:u32){arp_emit(1,ip,&BROADCAST,&[0;6])}
fn arp_reply(ip:u32,mac:&[u8;6]){arp_emit(2,ip,mac,mac)}
fn resolve(hop:u32)->Result<[u8;6],i64>{
 if let Some(m)=arp_lookup(hop){return Ok(m)}
 arp_request(hop);
 let mut round=0u32;
 loop{
  poll();
  if let Some(m)=arp_lookup(hop){return Ok(m)}
  round+=1;
  if round>2_000{return Err(errno::EHOSTUNREACH)}
  nap(round);
 }
}
pub fn mac_for(ip:u32)->Result<[u8;6],i64>{
 if ip==0xFFFF_FFFF{return Ok(BROADCAST)}
 let hop=if ip&NETMASK==OUR_IP&NETMASK{ip}else{GATEWAY};
 resolve(hop)
}

/* ---------- ICMP ---------- */

fn icmp_input(src_ip:u32,src_mac:&[u8;6],icmp:&[u8]){
 if icmp.len()<8||icmp[0]!=8{return}
 let n=core::cmp::min(icmp.len(),1500);
 let mut seg=[0u8;1500];
 seg[..n].copy_from_slice(&icmp[..n]);
 seg[0]=0;
 seg[2..4].copy_from_slice(&0u16.to_be_bytes());
 let h=csum_bytes(0,&seg[..n]);
 seg[2..4].copy_from_slice(&h.to_be_bytes());
 let _=ip_emit(IP_ICMP,src_ip,src_mac,&seg[..n]);
}

/* ---------- UDP ---------- */

pub const MAX_UDP:usize=8;
const UDPQ:usize=4;const DGRAM_MAX:usize=1472;
struct UdpSocket{
 used:bool,laddr:u32,lport:u16,connected:bool,raddr:u32,rport:u16,
 rx:[[u8;DGRAM_MAX];UDPQ],rxlen:[usize;UDPQ],rxsrc:[(u32,u16);UDPQ],rxhead:usize,rxn:usize,
 readq:WaitQueue,
}
impl UdpSocket{
 const fn new()->Self{Self{used:false,laddr:0,lport:0,connected:false,raddr:0,rport:0,rx:[[0;DGRAM_MAX];UDPQ],rxlen:[0;UDPQ],rxsrc:[(0,0);UDPQ],rxhead:0,rxn:0,readq:WaitQueue::new()}}
}
static UDPS:[SpinLock<UdpSocket>;MAX_UDP]=[const{SpinLock::new(UdpSocket::new())};MAX_UDP];
fn port_in_use(skip:usize,port:u16)->bool{
 for i in 0..MAX_UDP{
  if i==skip{continue}
  let s=UDPS[i].lock();
  if s.used&&s.lport==port{return true}
 }
 false
}
fn autobind(s:&mut UdpSocket,id:usize){
 while s.lport==0{
  let p=((EPHEMERAL.fetch_add(1,Ordering::Relaxed)&0x3fff)as u16).wrapping_add(0xC000);
  if p>=0xC000&&!port_in_use(id,p){s.lport=p;}
 }
}
pub fn udp_open()->Result<u8,i64>{
 for i in 0..MAX_UDP{
  let mut s=UDPS[i].lock();
  if !s.used{*s=UdpSocket::new();s.used=true;return Ok(i as u8)}
 }
 Err(errno::EMFILE)
}
pub fn udp_close(id:u8){if(id as usize)<MAX_UDP{*UDPS[id as usize].lock()=UdpSocket::new()}}
pub fn udp_bind(id:u8,ip:u32,port:u16)->Result<(),i64>{
 let i=id as usize;
 if i>=MAX_UDP{return Err(errno::EBADF)}
 if port!=0{
  for j in 0..MAX_UDP{
   if j==i{continue}
   let s=UDPS[j].lock();
   if s.used&&s.lport==port&&(s.laddr==0||ip==0||s.laddr==ip){return Err(errno::EADDRINUSE)}
  }
 }
 let mut s=UDPS[i].lock();
 if !s.used{return Err(errno::EBADF)}
 s.laddr=ip;
 s.lport=port;
 Ok(())
}
pub fn udp_connect(id:u8,ip:u32,port:u16)->Result<(),i64>{
 let i=id as usize;
 if i>=MAX_UDP{return Err(errno::EBADF)}
 let mut s=UDPS[i].lock();
 if !s.used{return Err(errno::EBADF)}
 if s.lport==0{autobind(&mut s,i)}
 s.connected=true;
 s.raddr=ip;
 s.rport=port;
 Ok(())
}
pub fn udp_send(id:u8,to:Option<(u32,u16)>,data:&[u8])->Result<usize,i64>{
 let i=id as usize;
 if i>=MAX_UDP{return Err(errno::EBADF)}
 if data.len()>DGRAM_MAX{return Err(errno::EMSGSIZE)}
 let(dst_ip,dst_port,src_port)={
  let mut s=UDPS[i].lock();
  if !s.used{return Err(errno::EBADF)}
  let(ip,port)=match to{
   Some(t)=>t,
   None=>if s.connected{(s.raddr,s.rport)}else{return Err(errno::EDESTADDRREQ)},
  };
  if s.lport==0{autobind(&mut s,i)}
  (ip,port,s.lport)
 };
 /* Loopback datagrams are delivered locally without ARP or the wire. */
 if is_loopback(dst_ip){
  return match loopback_udp(dst_ip,src_port,dst_port,data){Ok(())=>Ok(data.len()),Err(e)=>Err(e)};
 }
 let mac=resolve_peer(dst_ip)?;
 udp_emit(src_port,dst_ip,dst_port,&mac,data)?;
 Ok(data.len())
}
fn resolve_peer(ip:u32)->Result<[u8;6],i64>{
 if let Some(m)=arp_lookup(ip){return Ok(m)}
 mac_for(ip)
}
fn udp_emit(sp:u16,di:u32,dp:u16,dm:&[u8;6],data:&[u8])->Result<(),i64>{
 let len=8+data.len();
 let mut seg=[0u8;1500];
 seg[0..2].copy_from_slice(&sp.to_be_bytes());
 seg[2..4].copy_from_slice(&dp.to_be_bytes());
 seg[4..6].copy_from_slice(&(len as u16).to_be_bytes());
 seg[8..len].copy_from_slice(data);
 let mut c=csum_pseudo(OUR_IP,di,IP_UDP,len,&seg[..len]);
 if c==0{c=0xffff}
 seg[6..8].copy_from_slice(&c.to_be_bytes());
 ip_emit(IP_UDP,di,dm,&seg[..len])
}
fn udp_input(src_ip:u32,seg:&[u8]){
 if seg.len()<8{return}
 let sp=be16(seg,0);let dp=be16(seg,2);
 let ulen=be16(seg,4)as usize;
 if ulen<8||seg.len()<ulen{return}
 let sum=be16(seg,6);
 if sum!=0&&csum_pseudo(src_ip,OUR_IP,IP_UDP,ulen,&seg[..ulen])!=0{return}
 udp_deliver(src_ip,sp,OUR_IP,dp,&seg[8..ulen]);
}
fn udp_addr_match(bound:u32,dst:u32)->bool{
 if bound==0||bound==dst{return true}
 /* A socket bound to our address takes wire unicast and broadcast, but not
    loopback traffic aimed at 127/8. */
 bound==OUR_IP&&dst>>24!=127
}
fn udp_deliver(src_ip:u32,src_port:u16,dst_ip:u32,dst_port:u16,data:&[u8]){
 for i in 0..MAX_UDP{
  let mut s=UDPS[i].lock();
  if !s.used||s.lport!=dst_port||!udp_addr_match(s.laddr,dst_ip){continue}
  if s.connected&&!(s.raddr==src_ip&&s.rport==src_port){continue}
  if s.rxn==UDPQ{continue}
  let slot=(s.rxhead+s.rxn)%UDPQ;
  let n=core::cmp::min(data.len(),DGRAM_MAX);
  s.rx[slot][..n].copy_from_slice(&data[..n]);
  s.rxlen[slot]=n;
  s.rxsrc[slot]=(src_ip,src_port);
  s.rxn+=1;
  s.readq.wake_all();
  return;
 }
}
pub fn udp_recv(id:u8,dst:&mut[u8])->Result<(usize,u32,u16),i64>{
 let i=id as usize;
 if i>=MAX_UDP{return Err(errno::EBADF)}
 let mut round=0u32;
 loop{
  poll();
  {
   let mut s=UDPS[i].lock();
   if !s.used{return Err(errno::EBADF)}
   if s.rxn>0{
    let slot=s.rxhead;
    let n=core::cmp::min(s.rxlen[slot],dst.len());
    dst[..n].copy_from_slice(&s.rx[slot][..n]);
    let src=s.rxsrc[slot];
    s.rxhead=(s.rxhead+1)%UDPQ;
    s.rxn-=1;
    return Ok((n,src.0,src.1));
   }
  }
  round+=1;
  if round>4_000{return Err(errno::ETIMEDOUT)}
  nap(round);
 }
}
pub fn udp_local(id:u8)->Result<(u32,u16),i64>{
 let s=UDPS[id as usize].lock();
 if !s.used{return Err(errno::EBADF)}
 Ok((s.laddr,s.lport))
}
pub fn udp_peername(id:u8)->Result<(u32,u16),i64>{
 let s=UDPS[id as usize].lock();
 if !s.used{return Err(errno::EBADF)}
 if !s.connected{return Err(errno::ENOTCONN)}
 Ok((s.raddr,s.rport))
}

/* ---------- TCP (client) ---------- */

pub const MAX_TCP:usize=8;
const TCP_RCV_CAP:usize=8192;const TCP_SND_CAP:usize=4096;
const SYN:u8=0x02;const ACK:u8=0x10;const FIN:u8=0x01;const RST:u8=0x04;const PSH:u8=0x08;
#[derive(Clone,Copy,PartialEq)]enum TcpState{Closed,SynSent,Listen,SynRcvd,Established,FinWait1,FinWait2,CloseWait,LastAck,TimeWait}
#[derive(Clone,Copy)]struct TcpConn{
 used:bool,state:TcpState,
 laddr:u32,lport:u16,rip:u32,rport:u16,
 dmac:[u8;6],
 snd_una:u32,snd_nxt:u32,rlen:usize,
 rmem:[u8;TCP_SND_CAP],
 wmem:[u8;TCP_RCV_CAP],wlen:usize,rcv_nxt:u32,adv_window:u16,
 peer_window:u16,
 fin_pending:bool,fin_sent:bool,fin_seq:u32,peer_fin:bool,reset:bool,
 retrans_at:u64,retrans_budget:u32,
}
impl TcpConn{
 const fn new()->Self{Self{used:false,state:TcpState::Closed,laddr:0,lport:0,rip:0,rport:0,dmac:[0;6],snd_una:0,snd_nxt:0,rlen:0,rmem:[0;TCP_SND_CAP],wmem:[0;TCP_RCV_CAP],wlen:0,rcv_nxt:0,adv_window:0,peer_window:0,fin_pending:false,fin_sent:false,fin_seq:0,peer_fin:false,reset:false,retrans_at:0,retrans_budget:0}}
 fn window(&self)->u16{(TCP_RCV_CAP-self.wlen)as u16}
}
static TCPS:[SpinLock<TcpConn>;MAX_TCP]=[const{SpinLock::new(TcpConn::new())};MAX_TCP];
/* Server-side accept queues: one pending-child list plus a waiter queue per
   connection slot. Only slots in Listen state use them. Kept outside
   TcpConn so the connection table stays Copy. */
const ACCEPTQ:usize=4;
static TCP_ACCEPTQ:[SpinLock<[Option<u8>;ACCEPTQ]>;MAX_TCP]=[const{SpinLock::new([None;ACCEPTQ])};MAX_TCP];
static TCP_ACCEPTWQ:[WaitQueue;MAX_TCP]=[const{WaitQueue::new()};MAX_TCP];
fn acceptq_push(listener:usize,child:u8)->bool{
 let mut q=TCP_ACCEPTQ[listener].lock();
 if let Some(slot)=q.iter_mut().find(|s|s.is_none()){*slot=Some(child);true}else{false}
}
fn acceptq_pop(listener:usize)->Option<u8>{
 let mut q=TCP_ACCEPTQ[listener].lock();
 match q.iter_mut().find(|s|s.is_some()){Some(slot)=>{let c=slot.take();*slot=None;c},None=>None}
}
fn acceptq_len(listener:usize)->usize{TCP_ACCEPTQ[listener].lock().iter().filter(|s|s.is_some()).count()}
fn acceptq_clear(listener:usize){*TCP_ACCEPTQ[listener].lock()=[None;ACCEPTQ];}
/* Loopback delivery: frames for 127/8 or our own address never reach the
   wire. They are queued here and dispatched from poll(), which runs without
   connection locks held, so a looped-back segment can never deadlock on the
   sender's own slot lock. The source is presented as the destination, so
   both directions of a loopback connection use one consistent address. */
const LOOPQ:usize=8;const LOOP_SEGSZ:usize=1536;
#[derive(Clone,Copy)]enum LoopItem{Empty,Udp{src:u32,sp:u16,dp:u16,len:usize,data:[u8;DGRAM_MAX]},Tcp{src:u32,dst:u32,len:usize,seg:[u8;LOOP_SEGSZ]}}
struct LoopBuf{items:[LoopItem;LOOPQ],head:usize,count:usize}
impl LoopBuf{const fn new()->Self{Self{items:[LoopItem::Empty;LOOPQ],head:0,count:0}}}
static LOOPBACK:SpinLock<LoopBuf>=SpinLock::new(LoopBuf::new());
fn is_loopback(ip:u32)->bool{ip>>24==127||ip==OUR_IP}
fn loopback_push(item:LoopItem)->Result<(),i64>{
 let mut q=LOOPBACK.lock();
 if q.count>=LOOPQ{return Err(errno::ENOMEM)}
 let slot=(q.head+q.count)%LOOPQ;
 q.items[slot]=item;
 q.count+=1;
 Ok(())
}
fn loopback_udp(src:u32,sp:u16,dp:u16,data:&[u8])->Result<(),i64>{
 if data.len()>DGRAM_MAX{return Err(errno::EMSGSIZE)}
 let mut buf=[0u8;DGRAM_MAX];
 buf[..data.len()].copy_from_slice(data);
 loopback_push(LoopItem::Udp{src,sp,dp,len:data.len(),data:buf})
}
fn loopback_tcp(src:u32,dst:u32,seg:&[u8])->Result<(),i64>{
 if seg.len()>LOOP_SEGSZ{return Err(errno::EMSGSIZE)}
 let mut buf=[0u8;LOOP_SEGSZ];
 buf[..seg.len()].copy_from_slice(seg);
 loopback_push(LoopItem::Tcp{src,dst,len:seg.len(),seg:buf})
}
fn tcp_emit(t:&TcpConn,seq:u32,ack:u32,flags:u8,payload:&[u8]){
 let len=20+payload.len();
 let mut seg=[0u8;1500];
 seg[0..2].copy_from_slice(&t.lport.to_be_bytes());
 seg[2..4].copy_from_slice(&t.rport.to_be_bytes());
 seg[4..8].copy_from_slice(&seq.to_be_bytes());
 seg[8..12].copy_from_slice(&ack.to_be_bytes());
 seg[12]=0x50;
 seg[13]=flags;
 seg[14..16].copy_from_slice(&t.window().to_be_bytes());
 seg[20..len].copy_from_slice(payload);
 let c=csum_pseudo(OUR_IP,t.rip,IP_TCP,len,&seg[..len]);
 seg[16..18].copy_from_slice(&c.to_be_bytes());
 /* Loopback and self sends never reach the wire; they re-enter through
    poll() so arrival processing never runs under a connection lock. */
 if is_loopback(t.rip){
  let _=loopback_tcp(t.rip,t.rip,&seg[..len]);
  return;
 }
 let _=ip_emit(IP_TCP,t.rip,&t.dmac,&seg[..len]);
}
fn tcp_ack(t:&TcpConn){tcp_emit(t,t.snd_nxt,t.rcv_nxt,ACK,&[])}
/* Sends as much buffered data as the peer window allows, then a pending FIN.
   Returns true when something was transmitted. */
fn tcp_flush(t:&mut TcpConn)->bool{
 let mut sent=false;
 if t.reset{return false}
 if t.state==TcpState::SynSent||t.state==TcpState::Closed{return false}
 let win=if t.peer_window==0{0usize}else{t.peer_window as usize};
 while t.rlen>0&&(t.snd_nxt.wrapping_sub(t.snd_una)as usize)<t.rlen{
  let inflight=t.snd_nxt.wrapping_sub(t.snd_una)as usize;
  if inflight>=win{break}
  let avail=core::cmp::min(t.rlen-inflight,win-inflight);
  let n=core::cmp::min(MSS,avail);
  if n==0{break}
  let mut tmp=[0u8;MSS];
  tmp[..n].copy_from_slice(&t.rmem[inflight..inflight+n]);
  tcp_emit(t,t.snd_nxt,t.rcv_nxt,ACK|PSH,&tmp[..n]);
  t.snd_nxt=t.snd_nxt.wrapping_add(n as u32);
  t.retrans_at=ticks()+RTO_TICKS;
  t.retrans_budget=0;
  sent=true;
 }
 if t.fin_pending&&!t.fin_sent&&(t.snd_nxt.wrapping_sub(t.snd_una)as usize)==t.rlen&&win>0{
  t.fin_seq=t.snd_nxt;
  tcp_emit(t,t.fin_seq,t.rcv_nxt,ACK|FIN,&[]);
  t.snd_nxt=t.snd_nxt.wrapping_add(1);
  t.fin_sent=true;
  t.fin_pending=false;
  t.retrans_at=ticks()+RTO_TICKS;
  t.retrans_budget=0;
  sent=true;
 }
 sent
}
pub fn tcp_open()->Result<u8,i64>{
 for i in 0..MAX_TCP{
  let mut t=TCPS[i].lock();
  if !t.used{*t=TcpConn::new();t.used=true;return Ok(i as u8)}
 }
 Err(errno::EMFILE)
}
/* Resets a half-open or queued server-side connection by slot. */
fn tcp_reset_child(ci:usize){
 if ci>=MAX_TCP{return}
 let (rip,rport,lport,dmac,sn,rn)={
  let mut c=TCPS[ci].lock();
  if !c.used{return}
  let v=(c.rip,c.rport,c.lport,c.dmac,c.snd_nxt,c.rcv_nxt);
  c.reset=true;c.state=TcpState::Closed;
  v
 };
 let mut tmp=TcpConn::new();
 tmp.lport=lport;tmp.rport=rport;tmp.rip=rip;tmp.dmac=dmac;
 tcp_emit(&tmp,sn,rn,RST|ACK,&[]);
 *TCPS[ci].lock()=TcpConn::new();
}
pub fn tcp_close(id:u8){
 let i=id as usize;
 if i>=MAX_TCP{return}
 /* A listener goes away at once; queued children are reset so no slot
    leaks. An embryonic connection is reset outright. */
 {
  let (used,state)={let t=TCPS[i].lock();(t.used,t.state)};
  if !used{return}
  if state==TcpState::Listen{
   loop{
    match acceptq_pop(i){
     Some(child)=>tcp_reset_child(child as usize),
     None=>break,
    }
   }
   *TCPS[i].lock()=TcpConn::new();
   return;
  }
  if state==TcpState::SynRcvd{
   tcp_reset_child(i);
   return;
  }
 }
  let mut t=TCPS[i].lock();
  if !t.used{return}
  if !t.fin_sent&&!t.reset&&matches!(t.state,TcpState::Established|TcpState::CloseWait){
   t.fin_pending=true;
   tcp_flush(&mut t);
  }
  drop(t);
 let mut round=0u32;
 loop{
  poll();
  {
   let t=TCPS[i].lock();
   if !t.used||t.reset||t.state==TcpState::Closed||t.state==TcpState::TimeWait||round>2_000{break}
  }
  round+=1;
  nap(round);
 }
 *TCPS[i].lock()=TcpConn::new();
}
pub fn tcp_bind(id:u8,ip:u32,port:u16)->Result<(),i64>{
 let i=id as usize;
 if i>=MAX_TCP{return Err(errno::EBADF)}
 let mut t=TCPS[i].lock();
 if !t.used{return Err(errno::EBADF)}
 /* A bound, listening or connected socket cannot be rebound. */
 if t.state!=TcpState::Closed{return Err(errno::EINVAL)}
 if port!=0{
  for j in 0..MAX_TCP{
   if j==i{continue}
   let o=TCPS[j].lock();
   if !o.used||o.lport!=port{continue}
   if o.laddr==0||ip==0||o.laddr==ip{return Err(errno::EADDRINUSE)}
  }
 }
 t.laddr=ip;
 t.lport=port;
 Ok(())
}
fn tcp_autobind(t:&mut TcpConn,id:usize){
 while t.lport==0{
  let p=((EPHEMERAL.fetch_add(1,Ordering::Relaxed)&0x3fff)as u16).wrapping_add(0xC000);
  if p>=0xC000&&!port_in_use(id,p)&&!tcp_port_in_use(id,p){t.lport=p;}
 }
}
fn tcp_port_in_use(skip:usize,port:u16)->bool{
 for j in 0..MAX_TCP{
  if j==skip{continue}
  let o=TCPS[j].lock();
  if o.used&&o.lport==port{return true}
 }
 false
}
/* Marks a bound socket listening. An unbound socket is auto-bound to the
   wildcard first, matching Linux; re-listening is idempotent. */
pub fn tcp_listen(id:u8)->Result<(),i64>{
 let i=id as usize;
 if i>=MAX_TCP{return Err(errno::EBADF)}
 let mut t=TCPS[i].lock();
 if !t.used{return Err(errno::EBADF)}
 match t.state{
  TcpState::Closed=>{
   if t.lport==0{t.laddr=0;tcp_autobind(&mut t,i);}
   t.state=TcpState::Listen;
   t.rip=0;t.rport=0;
   acceptq_clear(i);
   Ok(())
  }
  TcpState::Listen=>Ok(()),
  _=>Err(errno::EINVAL),
 }
}
/* Takes the next completed connection. Blocks while the queue is empty. */
pub fn tcp_accept(id:u8,nonblock:bool)->Result<u8,i64>{
 let i=id as usize;
 if i>=MAX_TCP{return Err(errno::EBADF)}
 loop{
  {
   let t=TCPS[i].lock();
   if !t.used{return Err(errno::EBADF)}
   if t.state!=TcpState::Listen{return Err(errno::EINVAL)}
  }
  if let Some(child)=acceptq_pop(i){return Ok(child)}
  if nonblock{return Err(errno::EAGAIN)}
  let registered=TCP_ACCEPTWQ[i].register().is_ok();
  if acceptq_len(i)>0{continue}
  if !registered{return Err(errno::EAGAIN)}
  crate::task::block_current();
 }
}
pub fn tcp_pending(id:u8)->bool{
 let i=id as usize;
 if i>=MAX_TCP{return false}
 let t=TCPS[i].lock();
 t.used&&t.state==TcpState::Listen&&acceptq_len(i)>0
}
pub fn tcp_connect(id:u8,ip:u32,port:u16)->Result<(),i64>{
 let i=id as usize;
 if i>=MAX_TCP{return Err(errno::EBADF)}
 /* Loopback needs no ARP: the peer is local by construction. */
 let mac=if is_loopback(ip){[0;6]}else{mac_for(ip)?};
 {
  let mut t=TCPS[i].lock();
  if !t.used{return Err(errno::EBADF)}
  if t.state!=TcpState::Closed{return Err(errno::EISCONN)}
  if t.lport==0{tcp_autobind(&mut t,i)}
  if t.laddr==0{t.laddr=OUR_IP}
  t.rip=ip;
  t.rport=port;
  t.dmac=mac;
  let isn=0x4000_0000u32^(ticks()as u32).wrapping_mul(0x0100_0101);
  t.snd_una=isn;
  t.snd_nxt=isn;
  t.rlen=0;
  t.wlen=0;
  t.rcv_nxt=0;
  t.peer_window=TCP_RCV_CAP as u16;
  t.state=TcpState::SynSent;
  tcp_emit(&t,isn,0,SYN,&[]);
  t.snd_nxt=isn.wrapping_add(1);
  t.retrans_at=ticks()+RTO_TICKS;
  t.retrans_budget=0;
 }
 let mut round=0u32;
 loop{
  poll();
  {
   let t=TCPS[i].lock();
   if !t.used{return Err(errno::EBADF)}
   if t.reset{return Err(errno::ECONNREFUSED)}
   if t.state==TcpState::Established{return Ok(())}
  }
  round+=1;
  if round>4_000{return Err(errno::ETIMEDOUT)}
  nap(round);
 }
}
pub fn tcp_send(id:u8,data:&[u8])->Result<usize,i64>{
 let i=id as usize;
 if i>=MAX_TCP{return Err(errno::EBADF)}
 let mut off=0usize;let mut round=0u32;
 while off<data.len(){
  let mut progressed=false;
  {
   let mut t=TCPS[i].lock();
   if !t.used{return Err(errno::EBADF)}
   if t.reset{return Err(errno::ECONNRESET)}
   if t.fin_sent||t.fin_pending||t.state==TcpState::Closed{return Err(errno::EPIPE)}
   if t.rlen<TCP_SND_CAP{
    let n=core::cmp::min(TCP_SND_CAP-t.rlen,data.len()-off);
    {let rlen=t.rlen;t.rmem[rlen..rlen+n].copy_from_slice(&data[off..off+n]);}
    t.rlen+=n;
    off+=n;
    tcp_flush(&mut t);
    progressed=true;
   }
  }
  if off==data.len(){break}
  if !progressed{
   poll();
   round+=1;
   if round>4_000{return Err(errno::ETIMEDOUT)}
   nap(round);
  }
 }
 Ok(off)
}
pub fn tcp_recv(id:u8,dst:&mut[u8])->Result<usize,i64>{
 let i=id as usize;
 if i>=MAX_TCP{return Err(errno::EBADF)}
 let mut round=0u32;
 loop{
  poll();
  {
   let mut t=TCPS[i].lock();
   if !t.used{return Err(errno::EBADF)}
   if t.reset{return Err(errno::ECONNRESET)}
   if t.wlen>0{
    let n=core::cmp::min(t.wlen,dst.len());
    dst[..n].copy_from_slice(&t.wmem[..n]);
    let wlen=t.wlen;
    t.wmem.copy_within(n..wlen,0);
    t.wlen-=n;
    let win=t.window();
    if win as usize>=MSS&&win.saturating_sub(t.adv_window)>=MSS as u16/2{
     t.adv_window=win;
     tcp_ack(&*t);
    }
    return Ok(n);
   }
   if t.peer_fin{return Ok(0)}
  }
  round+=1;
  if round>4_000{return Err(errno::ETIMEDOUT)}
  nap(round);
 }
}
pub fn tcp_shutdown(id:u8,how:i32)->Result<(),i64>{
 let i=id as usize;
 if i>=MAX_TCP{return Err(errno::EBADF)}
 if how==0{return Ok(())}
 let mut t=TCPS[i].lock();
 if !t.used{return Err(errno::EBADF)}
 if !t.fin_sent&&!t.reset&&matches!(t.state,TcpState::Established|TcpState::CloseWait){
  t.fin_pending=true;
  tcp_flush(&mut t);
 }
 Ok(())
}
pub fn tcp_local(id:u8)->Result<(u32,u16),i64>{
 let t=TCPS[id as usize].lock();
 if !t.used{return Err(errno::EBADF)}
 Ok((t.laddr,t.lport))
}
pub fn tcp_peername(id:u8)->Result<(u32,u16),i64>{
 let t=TCPS[id as usize].lock();
 if !t.used{return Err(errno::EBADF)}
 if !matches!(t.state,TcpState::SynRcvd|TcpState::Established|TcpState::FinWait1|TcpState::FinWait2|TcpState::CloseWait|TcpState::LastAck){return Err(errno::ENOTCONN)}
 Ok((t.rip,t.rport))
}
fn tcp_listen_match(dp:u16,dst:u32)->Option<usize>{
 for i in 0..MAX_TCP{
  let t=TCPS[i].lock();
  if t.used&&t.state==TcpState::Listen&&t.lport==dp&&(t.laddr==0||t.laddr==dst){return Some(i)}
 }
 None
}
/* Allocates an embryonic server-side connection for an incoming SYN. */
fn tcp_syn_rcvd(listener:usize,src_ip:u32,sp:u16,dp:u16,seq:u32,window:u16,src_mac:[u8;6])->Result<(),i64>{
 if acceptq_len(listener)>=ACCEPTQ{return Err(errno::ENOMEM)}
 let mut child:Option<usize>=None;
 for j in 0..MAX_TCP{
  let mut c=TCPS[j].lock();
  if !c.used{
   let (lladdr,_)={let l=TCPS[listener].lock();(l.laddr,l.lport)};
   *c=TcpConn::new();
   c.used=true;c.state=TcpState::SynRcvd;
   c.laddr=lladdr;c.lport=dp;c.rip=src_ip;c.rport=sp;c.dmac=src_mac;
   let isn=0x5000_0000u32^(ticks()as u32).wrapping_mul(0x0100_0101).wrapping_add(j as u32);
   c.snd_una=isn;c.snd_nxt=isn.wrapping_add(1);
   c.rcv_nxt=seq.wrapping_add(1);
   c.peer_window=window;{let w=c.window();c.adv_window=w;}
   c.retrans_at=ticks()+RTO_TICKS;c.retrans_budget=0;
   child=Some(j);
   break;
  }
 }
 let j=match child{Some(v)=>v,None=>return Err(errno::EMFILE)};
 let snap=*TCPS[j].lock();
 tcp_emit(&snap,snap.snd_una,snap.rcv_nxt,SYN|ACK,&[]);
 Ok(())
}
fn tcp_send_rst(dp:u16,sp:u16,dst_ip:u32,src_mac:[u8;6],seq:u32,ack:u32,has_ack:bool){
 /* Refusing a connection: RST|ACK acknowledging the SYN. Anything else is
    ignored to avoid reset wars. */
 let mut tmp=TcpConn::new();
 tmp.lport=dp;tmp.rport=sp;tmp.rip=dst_ip;tmp.dmac=src_mac;
 let (s,a)=if has_ack{(ack,0)}else{(0,seq.wrapping_add(1))};
 tcp_emit(&tmp,s,a,RST|ACK,&[]);
}
fn tcp_input(src_ip:u32,dst_ip:u32,seg:&[u8],src_mac:[u8;6]){
 if seg.len()<20{return}
 let sp=be16(seg,0);let dp=be16(seg,2);
 let seq=be32(seg,4);let ack=be32(seg,8);
 let doff=((seg[12]>>4)as usize)*4;
 if doff<20||seg.len()<doff{return}
 let flags=seg[13];
 let window=be16(seg,14)as usize;
 let payload=&seg[doff..];
 for i in 0..MAX_TCP{
  let mut guard=TCPS[i].lock();
  if !guard.used||guard.lport!=dp||guard.rip!=src_ip||guard.rport!=sp{continue}
  let t=&mut *guard;
  t.dmac=src_mac;
  /* Server-side embryonic connection. */
  if t.state==TcpState::SynRcvd{
   if flags&RST!=0{t.reset=true;t.state=TcpState::Closed;return}
   if flags&SYN!=0&&flags&ACK==0{
    /* Retransmitted SYN: the SYN-ACK was lost. Resend it. */
    let snap=*t;
    tcp_emit(&snap,snap.snd_una,snap.rcv_nxt,SYN|ACK,&[]);
    return;
   }
   if flags&ACK!=0&&flags&SYN==0&&ack==t.snd_nxt{
    t.snd_una=ack;
    t.state=TcpState::Established;
    t.retrans_at=0;
    tcp_ack(t);
    let child=i as u8;
    drop(guard);
    /* Find the listener that owns this port and queue the child. */
    let mut li:Option<usize>=None;
    for l in 0..MAX_TCP{
     let o=TCPS[l].lock();
     if o.used&&o.state==TcpState::Listen&&o.lport==dp&&(o.laddr==0||o.laddr==dst_ip){li=Some(l);break}
    }
    match li{
     Some(l)=>{
      if acceptq_push(l,child){TCP_ACCEPTWQ[l].wake_all();}
      else{tcp_reset_child(child as usize);}
     }
     None=>tcp_reset_child(child as usize),
    }
    return;
   }
   return;
  }
  if flags&RST!=0{
   t.reset=true;
   t.state=TcpState::Closed;
   return;
  }
  let mut ack_needed=false;
  if flags&SYN!=0{
   if t.state==TcpState::SynSent&&flags&ACK!=0&&ack==t.snd_nxt{
    t.snd_una=ack;
    t.snd_nxt=ack;
    t.rcv_nxt=seq.wrapping_add(1);
    t.peer_window=window as u16;
    t.adv_window=t.window();
    t.state=TcpState::Established;
    t.retrans_at=0;
    tcp_ack(&t);
   }
   return;
  }
  if t.state==TcpState::SynSent{return}
  /* Payload acceptance: strictly in order; anything else is ACKed as-is. */
  if !payload.is_empty(){
   if seq==t.rcv_nxt{
    let space=TCP_RCV_CAP-t.wlen;
    if payload.len()<=space{
     t.wmem[t.wlen..t.wlen+payload.len()].copy_from_slice(payload);
     t.wlen+=payload.len();
     t.rcv_nxt=seq.wrapping_add(payload.len()as u32);
    }
   }
   ack_needed=true;
  }
  if flags&FIN!=0&&seq.wrapping_add(payload.len()as u32)==t.rcv_nxt{
   t.rcv_nxt=t.rcv_nxt.wrapping_add(1);
   t.peer_fin=true;
   ack_needed=true;
   if t.state==TcpState::Established{t.state=TcpState::CloseWait}
   else if t.state==TcpState::FinWait2{t.state=TcpState::TimeWait}
   else if t.state==TcpState::FinWait1{t.state=TcpState::TimeWait}
  }
  if flags&ACK!=0{
   let una=t.snd_una;
   t.peer_window=window as u16;
   if seq_gt(ack,una)&&!seq_gt(ack,una.wrapping_add(t.rlen as u32)){
    let delta=(ack.wrapping_sub(una))as usize;
    let rlen=t.rlen;
    t.rmem.copy_within(delta..rlen,0);
    t.rlen-=delta;
    /* The acknowledged bytes leave the send window, so the base of the
       window moves forward. Without this the window never shrinks and the
       same bytes are retransmitted for ever. */
    t.snd_una=ack;
    if seq_gt(t.snd_nxt,ack){t.snd_nxt=ack}
    t.retrans_at=ticks()+RTO_TICKS;
    t.retrans_budget=0;
    tcp_flush(t);
   }
   if t.fin_sent&&ack==t.fin_seq.wrapping_add(1){
    match t.state{
     TcpState::FinWait1=>t.state=if t.peer_fin{TcpState::TimeWait}else{TcpState::FinWait2},
     TcpState::LastAck|TcpState::TimeWait=>t.state=TcpState::Closed,
     _=>{}
    }
   }
  }
  if ack_needed{
   t.adv_window=t.window();
   tcp_ack(t);
  }
  return;
 }
 /* No connection owns this segment. An initial SYN goes to a listener, or
    is refused with RST so connect() reports ECONNREFUSED instead of timing
    out. Anything else is ignored. */
 if flags&SYN!=0&&flags&ACK==0{
  if let Some(l)=tcp_listen_match(dp,dst_ip){
   let _=tcp_syn_rcvd(l,src_ip,sp,dp,seq,window as u16,src_mac);
  }else{
   tcp_send_rst(dp,sp,dst_ip,src_mac,seq,ack,false);
  }
 }
}
fn tcp_retransmit(t:&mut TcpConn){
 let now=ticks();
 if t.retrans_at==0{t.retrans_at=now+RTO_TICKS;return}
 if now<t.retrans_at{return}
 t.retrans_budget+=1;
 if t.retrans_budget>16{
  t.reset=true;
  t.state=TcpState::Closed;
  return;
 }
 t.retrans_at=now+RTO_TICKS;
 match t.state{
  TcpState::SynSent=>{
   tcp_emit(t,t.snd_una,0,SYN,&[]);
  }
  TcpState::SynRcvd=>{
   tcp_emit(t,t.snd_una,t.rcv_nxt,SYN|ACK,&[]);
  }
  TcpState::Established|TcpState::CloseWait|TcpState::FinWait1|TcpState::FinWait2|TcpState::LastAck=>{
   let inflight=t.snd_nxt.wrapping_sub(t.snd_una)as usize;
   if inflight>0{
    let n=core::cmp::min(MSS,inflight);
    let mut tmp=[0u8;MSS];
    tmp[..n].copy_from_slice(&t.rmem[..n]);
    tcp_emit(t,t.snd_una,t.rcv_nxt,ACK|PSH,&tmp[..n]);
    t.snd_nxt=t.snd_una.wrapping_add(n as u32);
   }else if t.fin_sent{
    tcp_emit(t,t.fin_seq,t.rcv_nxt,ACK|FIN,&[]);
   }
  }
  _=>{}
 }
}
fn tcp_tick(){
 let now=ticks();
 for i in 0..MAX_TCP{
  let mut t=TCPS[i].lock();
  if !t.used||t.reset{continue}
  let active=matches!(t.state,TcpState::SynSent|TcpState::SynRcvd|TcpState::Established|TcpState::FinWait1|TcpState::FinWait2|TcpState::CloseWait|TcpState::LastAck);
  if active&&now>=t.retrans_at{tcp_retransmit(&mut t);}
 }
}

/* ---------- receive path ---------- */

fn dispatch_arp(frame:&[u8]){
 let p=&frame[14..];
 if p.len()<28{return}
 let op=be16(p,6);
 let sha=[p[8],p[9],p[10],p[11],p[12],p[13]];
 let spa=be32(p,14);
 let tpa=be32(p,24);
 arp_insert(spa,sha);
 if op==1&&tpa==OUR_IP{arp_reply(spa,&sha)}
}
fn dispatch_ip(frame:&[u8]){
 let ip=&frame[14..];
 if ip.len()<20{return}
 let ihl=((ip[0]&0x0f)as usize)*4;
 if ihl<20||ip.len()<ihl{return}
 let proto=ip[9];
 let src=be32(ip,12);
 let dst=be32(ip,16);
 if dst!=OUR_IP&&dst!=0xFFFF_FFFF{return}
 let payload=&ip[ihl..];
 match proto{
  IP_ICMP=>icmp_input(src,&[frame[6],frame[7],frame[8],frame[9],frame[10],frame[11]],payload),
  IP_UDP=>udp_input(src,payload),
  IP_TCP=>tcp_input(src,dst,payload,[frame[6],frame[7],frame[8],frame[9],frame[10],frame[11]]),
  _=>{}
 }
}
/* Drains queued loopback segments with no locks held. */
fn poll_loopback(){
 loop{
  let (head,count)={let q=LOOPBACK.lock();(q.head,q.count)};
  if count==0{break}
  let item={let q=LOOPBACK.lock();q.items[head]};
  {
   let mut q=LOOPBACK.lock();
   q.items[head]=LoopItem::Empty;
   q.head=(head+1)%LOOPQ;
   q.count-=1;
  }
  match item{
   LoopItem::Empty=>break,
   LoopItem::Udp{src,sp,dp,len,data}=>{
    let n=core::cmp::min(len,DGRAM_MAX);
    udp_deliver(src,sp,src,dp,&data[..n]);
   }
   LoopItem::Tcp{src,dst,len,seg}=>{
    let n=core::cmp::min(len,LOOP_SEGSZ);
    tcp_input(src,dst,&seg[..n],[0;6]);
   }
  }
 }
}
fn dispatch(frame:&[u8]){
 if frame.len()<14{return}
 match be16(frame,12){
  ETH_ARP=>dispatch_arp(frame),
  ETH_IPV4=>dispatch_ip(frame),
  _=>{}
 }
}
/* Drains the RX ring and services TCP timers. Safe to call from the timer
   interrupt, syscall context and idle loops; all state is behind spin locks
   and never held across nested polls. */
pub fn poll(){
 if !crate::driver::virtio_net::ready(){return}
  let mut frame=[0u8;2048];
  while let Some(len)=crate::driver::virtio_net::poll_frame(&mut frame){
   dispatch(&frame[..len]);
  }
  poll_loopback();
  tcp_tick();
 }
