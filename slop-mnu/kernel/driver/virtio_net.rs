//! virtio-net driver.
//!
//! The driver speaks both virtio interfaces: the legacy port I/O device
//! (version 0.9.5) and the modern memory mapped device (virtio 1.0), so the
//! default `virtio-net-pci` device model works without a compatibility
//! switch. The device is polled: the receive queue is drained whenever the
//! network stack asks for a frame.
//!
//! The driver state sits behind an interrupt safe lock. The timer interrupt
//! polls the link, so a plain spin lock would deadlock: an interrupt that
//! arrives while the interrupted code holds the lock spins forever on a lock
//! the interrupted code cannot release.
use crate::arch::x86_64::phys::virt_to_phys;
use crate::driver::virtio::{self,Transport};
use crate::sync::IrqSpinLock;
use core::sync::atomic::Ordering;

const RXQ:u16=0;
const TXQ:u16=1;
/* The queue size a modern device is configured with. */
const QSIZE:usize=256;
const DESC_WRITE:u16=2;

struct Net{ready:bool,transport:Transport,mac:[u8;6],rx_used:u16,tx_used:u16,tx_next:u8,rx_size:usize,tx_size:usize}
static NET:IrqSpinLock<Net>=IrqSpinLock::new(Net{ready:false,transport:Transport::legacy(0),mac:[0;6],rx_used:0,tx_used:0,tx_next:0,rx_size:0,tx_size:0});

const RING_BYTES:usize=12288;
const RX_BUFS:usize=32;
const TX_BUFS:usize=8;
pub const BUF_SZ:usize=2048;

#[repr(C,align(4096))]struct Rings([u8;RING_BYTES]);
static mut RX_RING:Rings=Rings([0;RING_BYTES]);
static mut TX_RING:Rings=Rings([0;RING_BYTES]);
#[repr(align(64))]struct Buf([u8;BUF_SZ]);
static mut RX_MEM:[Buf;RX_BUFS]=[const{Buf([0;BUF_SZ])};RX_BUFS];
static mut TX_MEM:[Buf;TX_BUFS]=[const{Buf([0;BUF_SZ])};TX_BUFS];

fn rx_ring()->*mut u8{unsafe{core::ptr::addr_of_mut!(RX_RING.0)as*mut u8}}
fn tx_ring()->*mut u8{unsafe{core::ptr::addr_of_mut!(TX_RING.0)as*mut u8}}
fn rx_buf(i:usize)->*mut u8{unsafe{core::ptr::addr_of_mut!(RX_MEM[i].0)as*mut u8}}
fn tx_buf(i:usize)->*mut u8{unsafe{core::ptr::addr_of_mut!(TX_MEM[i].0)as*mut u8}}

/* The split virtqueue layout inside one ring buffer: the descriptor table
   comes first, the available ring follows it and the used ring starts at
   the next page boundary. The legacy interface demands exactly this layout,
   the modern one names the three parts separately but accepts any address. */
fn avail_off(size:usize)->usize{16*size}
fn used_off(size:usize)->usize{((avail_off(size)+6+2*size)+4095)&!4095}
/* The available ring of a split virtqueue follows the descriptor table. */
fn avail_idx(ring:*mut u8,size:usize)->*mut u16{unsafe{ring.add(avail_off(size)+2).cast()}}
fn avail_slot(ring:*mut u8,size:usize,i:usize)->*mut u16{unsafe{ring.add(avail_off(size)+4+2*(i%size)).cast()}}
fn used_idx(ring:*mut u8,size:usize)->*mut u16{unsafe{ring.add(used_off(size)+2).cast()}}
fn used_entry(ring:*mut u8,size:usize,i:usize)->(u32,u32){unsafe{let p=ring.add(used_off(size)+4+8*(i%size));((p as*const u32).read_volatile(),(p.add(4)as*const u32).read_volatile())}}
fn desc_set(ring:*mut u8,size:usize,i:usize,addr:u64,len:u32,flags:u16){
 unsafe{
  let p=ring.add(16*(i%size));
  (p as*mut u64).write_volatile(addr);
  (p.add(8)as*mut u32).write_volatile(len);
  (p.add(12)as*mut u16).write_volatile(flags);
  (p.add(14)as*mut u16).write_volatile(0);
 }
}
/* Publishes one descriptor on a queue's available ring and notifies the device. */
unsafe fn post(t:&Transport,q:u16,ring:*mut u8,size:usize,id:usize){
 unsafe{
  let ai=avail_idx(ring,size).read_volatile();
  avail_slot(ring,size,ai as usize).write_volatile(id as u16);
  core::sync::atomic::compiler_fence(Ordering::SeqCst);
  avail_idx(ring,size).write_volatile(ai.wrapping_add(1));
  core::sync::atomic::compiler_fence(Ordering::SeqCst);
  t.notify(q);
 }
}

/// Installs one queue of the device.
fn setup_queue(t:&Transport,q:u16,ring:*mut u8,min:usize)->Result<usize,&'static str>{
 let size=t.queue_size(q,QSIZE);
 if size<min{return Err("virtio queue smaller than requested")}
 if size>QSIZE{return Err("virtio queue larger than the ring buffer")}
 if used_off(size)+6+8*size>RING_BYTES{return Err("virtio queue does not fit the ring buffer")}
 let qd=virtio::Queue{desc:ring,avail:unsafe{ring.add(avail_off(size))},used:unsafe{ring.add(used_off(size))},size};
 t.queue_setup(q,&qd);
 Ok(size)
}
pub fn init()->Result<[u8;6],&'static str>{
 let t=match virtio::probe(virtio::MODERN_NET).or_else(||virtio::probe(virtio::LEGACY_NET)){Some(t)=>t,None=>return Err("virtio-net device not found")};
 t.set_status(0);
 t.set_status(virtio::S_ACK);
 t.set_status(virtio::S_ACK|virtio::S_DRIVER);
 if t.modern{
  if !t.negotiate_modern(){return Err("virtio-net feature negotiation failed")}
 }else{
  let _=t.read_features(0);
  t.set_features(0,0);
 }
 let rx_size=setup_queue(&t,RXQ,rx_ring(),RX_BUFS)?;
 let tx_size=setup_queue(&t,TXQ,tx_ring(),TX_BUFS)?;
 let mut net=NET.lock();
 unsafe{
  for i in 0..6{net.mac[i]=t.device_config(i as u64)as u8}
  for i in 0..RX_BUFS{desc_set(rx_ring(),rx_size,i,virt_to_phys(rx_buf(i)as u64),BUF_SZ as u32,DESC_WRITE)}
  let ai=avail_idx(rx_ring(),rx_size);
  for i in 0..RX_BUFS{avail_slot(rx_ring(),rx_size,i).write_volatile(i as u16)}
  ai.write_volatile(RX_BUFS as u16);
  core::sync::atomic::compiler_fence(Ordering::SeqCst);
  t.set_status(virtio::S_ACK|virtio::S_DRIVER|virtio::S_DRIVER_OK);
  t.notify(RXQ);
 }
 net.transport=t;
 net.rx_used=0;net.tx_used=0;net.tx_next=0;
 net.rx_size=rx_size;net.tx_size=tx_size;
 net.ready=true;
 let mac=net.mac;
 Ok(mac)
}
pub fn ready()->bool{NET.lock().ready}
pub fn mac()->[u8;6]{NET.lock().mac}
/* Moves one received frame out of the RX ring into `out`, recycling the
   buffer immediately so the device can keep filling it. */
/* Legacy virtio-net always prefixes each buffer with a 10-byte
   virtio_net_hdr (flags, gso_type, hdr_len, gso_size, csum_start,
   csum_offset). We negotiate no offloads, so it is all zeroes and the
   frame data starts after it. */
const VNET_HDR:usize=10;
pub fn poll_frame(out:&mut[u8])->Option<usize>{
 let mut net=NET.lock();
 if !net.ready{return None}
 let size=net.rx_size;
 unsafe{
  let ring=rx_ring();
  let used=used_idx(ring,size).read_volatile();
  if used==net.rx_used{return None}
  let(id,len)=used_entry(ring,size,net.rx_used as usize);
  net.rx_used=net.rx_used.wrapping_add(1);
  let id=id as usize;
  if id>=RX_BUFS{return None}
  if (len as usize)<VNET_HDR{post(&net.transport,RXQ,ring,size,id);return None}
  let n=core::cmp::min(len as usize-VNET_HDR,out.len());
  core::ptr::copy_nonoverlapping(rx_buf(id).add(VNET_HDR),out.as_mut_ptr(),n);
  post(&net.transport,RXQ,ring,size,id);
  Some(n)
 }
}
/* Copies one frame into a TX buffer and transmits it, draining the used ring
   entry before returning. */
pub fn send_frame(frame:&[u8])->Result<(),i64>{
 if frame.len()+VNET_HDR>BUF_SZ{return Err(crate::errno::EMSGSIZE)}
 let (i,t,size)={
  let mut net=NET.lock();
  if !net.ready{return Err(crate::errno::ENETDOWN)}
  let i=net.tx_next as usize;
  net.tx_next=((i+1)%TX_BUFS)as u8;
  (i,net.transport,net.tx_size)
 };
 unsafe{
  for b in 0..VNET_HDR{tx_buf(i).add(b).write_volatile(0)}
  core::ptr::copy_nonoverlapping(frame.as_ptr(),tx_buf(i).add(VNET_HDR),frame.len());
  desc_set(tx_ring(),size,i,virt_to_phys(tx_buf(i)as u64),(frame.len()+VNET_HDR)as u32,0);
  post(&t,TXQ,tx_ring(),size,i);
  /* Wait for the device to consume the buffer. The lock is not held while
     waiting: the timer interrupt polls the link and would deadlock on it. */
  let mut spins=0u32;
  loop{
   let used=used_idx(tx_ring(),size).read_volatile();
   let done={let mut net=NET.lock();if used.wrapping_sub(net.tx_used)>=1{net.tx_used=used;true}else{false}};
   if done{crate::net::note_tx_ok();return Ok(())}
   spins+=1;
   if spins>4_000_000{crate::net::note_tx_timeout();return Err(crate::errno::EIO)}
   core::hint::spin_loop();
  }
 }
}
