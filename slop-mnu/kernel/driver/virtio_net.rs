//! Legacy virtio-net (virtio 0.9.5) PCI driver, poll driven.
//!
//! No interrupts: the periodic timer and blocking socket waits call
//! `poll_frame`, and TX completion is observed by spinning on the used ring
//! after notifications (QEMU processes a notification synchronously when it
//! is written, so the used ring is current by the time the port write
//! returns).
use crate::arch::x86_64::phys::virt_to_phys;
use crate::arch::x86_64::port;
use crate::sync::SpinLock;
use core::sync::atomic::Ordering;

const VIRTIO_VENDOR:u16=0x1af4;
const NET_LEGACY_ID:u16=0x1000;

/* Legacy virtio-pci header, BAR0 I/O offsets. */
const HOST_FEATURES:u16=0x00;const GUEST_FEATURES:u16=0x04;
const QUEUE_ADDR:u16=0x08;const QUEUE_SIZE:u16=0x0c;const QUEUE_SELECT:u16=0x0e;
const QUEUE_NOTIFY:u16=0x10;const STATUS:u16=0x12;const CONFIG_MAC:u16=0x14;
const S_ACK:u8=1;const S_DRIVER:u8=2;const S_DRIVER_OK:u8=4;

const RXQ:u16=0;const TXQ:u16=1;
/* Legacy QUEUE_SIZE is read-only; QEMU advertises 256 for both queues. */
const QSIZE:usize=256;
/* desc 16*256=4096, avail 6+512=518, used aligned at 4096: 8192..10246. */
const RING_BYTES:usize=12288;
const USED_OFF:usize=8192;
const RX_BUFS:usize=32;const TX_BUFS:usize=8;pub const BUF_SZ:usize=2048;
const DESC_WRITE:u16=2;

struct Net{ready:bool,io:u16,mac:[u8;6],rx_used:u16,tx_used:u16,tx_next:u8}
static NET:SpinLock<Net>=SpinLock::new(Net{ready:false,io:0,mac:[0;6],rx_used:0,tx_used:0,tx_next:0});

#[repr(C,align(4096))]struct Rings([u8;RING_BYTES]);
#[repr(align(64))]struct Buf([u8;BUF_SZ]);
static mut RX_RING:Rings=Rings([0;RING_BYTES]);
static mut TX_RING:Rings=Rings([0;RING_BYTES]);
static mut RX_MEM:[Buf;RX_BUFS]=[const{Buf([0;BUF_SZ])};RX_BUFS];
static mut TX_MEM:[Buf;TX_BUFS]=[const{Buf([0;BUF_SZ])};TX_BUFS];

fn rx_ring()->*mut u8{unsafe{core::ptr::addr_of_mut!(RX_RING.0)as*mut u8}}
fn tx_ring()->*mut u8{unsafe{core::ptr::addr_of_mut!(TX_RING.0)as*mut u8}}
fn rx_buf(i:usize)->*mut u8{unsafe{core::ptr::addr_of_mut!(RX_MEM[i].0)as*mut u8}}
fn tx_buf(i:usize)->*mut u8{unsafe{core::ptr::addr_of_mut!(TX_MEM[i].0)as*mut u8}}

const AVAIL_OFF:usize=4096;
unsafe fn avail_idx(ring:*mut u8)->*mut u16{unsafe{ring.add(AVAIL_OFF+2).cast()}}
unsafe fn avail_slot(ring:*mut u8,i:usize)->*mut u16{unsafe{ring.add(AVAIL_OFF+4+2*(i%QSIZE)).cast()}}
unsafe fn used_idx(ring:*mut u8)->*mut u16{unsafe{ring.add(USED_OFF+2).cast()}}
unsafe fn used_entry(ring:*mut u8,i:usize)->(u32,u32){
 unsafe{
  let p=ring.add(USED_OFF+4+8*(i%QSIZE));
  ((p as *const u32).read_volatile(),(p.add(4) as *const u32).read_volatile())
 }
}
unsafe fn desc_set(ring:*mut u8,i:usize,addr:u64,len:u32,flags:u16){
 unsafe{
  let p=ring.add(16*(i%QSIZE));
  (p as *mut u64).write_volatile(addr);
  (p.add(8) as *mut u32).write_volatile(len);
  (p.add(12) as *mut u16).write_volatile(flags);
  (p.add(14) as *mut u16).write_volatile(0);
 }
}
/* Publishes one descriptor on a queue's available ring and notifies the device. */
unsafe fn post(io:u16,q:u16,ring:*mut u8,id:usize){
 unsafe{
  let ai=avail_idx(ring).read_volatile();
  avail_slot(ring,ai as usize).write_volatile(id as u16);
  core::sync::atomic::compiler_fence(Ordering::SeqCst);
  avail_idx(ring).write_volatile(ai.wrapping_add(1));
  core::sync::atomic::compiler_fence(Ordering::SeqCst);
  port::outw(io+QUEUE_NOTIFY,q);
 }
}
fn setup_queue(io:u16,q:u16,ring:*mut u8,min:usize)->Result<(),&'static str>{
 unsafe{
  port::outw(io+QUEUE_SELECT,q);
  let size=port::inw(io+QUEUE_SIZE)as usize;
  if size<min{return Err("virtio queue smaller than requested")}
  port::outl(io+QUEUE_ADDR,(virt_to_phys(ring as u64)>>12)as u32);
 }
 Ok(())
}
pub fn init()->Result<[u8;6],&'static str>{
 let found=crate::driver::pci::find(VIRTIO_VENDOR,NET_LEGACY_ID).ok_or("virtio-net device not found")?;
 if !found.bars[0].io{return Err("virtio-net BAR0 is not an I/O bar")}
 found.enable(&crate::driver::pci::Cam);
 let io=found.bars[0].base as u16;
 let mut net=NET.lock();
 unsafe{
  port::outb(io+STATUS,0);
  port::outb(io+STATUS,S_ACK|S_DRIVER);
  let _host=port::inl(io+HOST_FEATURES);
  port::outl(io+GUEST_FEATURES,0);
 }
 setup_queue(io,RXQ,rx_ring(),RX_BUFS)?;
 setup_queue(io,TXQ,tx_ring(),TX_BUFS)?;
 unsafe{
  for i in 0..6{net.mac[i]=port::inb(io+CONFIG_MAC+i as u16)}
  for i in 0..RX_BUFS{desc_set(rx_ring(),i,virt_to_phys(rx_buf(i)as u64),BUF_SZ as u32,DESC_WRITE)}
  let ai=avail_idx(rx_ring());
  for i in 0..RX_BUFS{avail_slot(rx_ring(),i).write_volatile(i as u16)}
  ai.write_volatile(RX_BUFS as u16);
  core::sync::atomic::compiler_fence(Ordering::SeqCst);
  port::outb(io+STATUS,S_ACK|S_DRIVER|S_DRIVER_OK);
  port::outw(io+QUEUE_NOTIFY,RXQ);
 }
 net.io=io;
 net.rx_used=0;net.tx_used=0;net.tx_next=0;
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
 unsafe{
  let ring=rx_ring();
  let used=used_idx(ring).read_volatile();
  if used==net.rx_used{return None}
  let(id,len)=used_entry(ring,net.rx_used as usize);
  net.rx_used=net.rx_used.wrapping_add(1);
  let id=id as usize;
  if id>=RX_BUFS{return None}
  if (len as usize)<VNET_HDR{post(net.io,RXQ,ring,id);return None}
  let n=core::cmp::min(len as usize-VNET_HDR,out.len());
  core::ptr::copy_nonoverlapping(rx_buf(id).add(VNET_HDR),out.as_mut_ptr(),n);
  post(net.io,RXQ,ring,id);
  Some(n)
 }
}
/* Copies one frame into a TX buffer and transmits it, draining the used ring
   entry before returning. */
pub fn send_frame(frame:&[u8])->Result<(),i64>{
 if frame.len()+VNET_HDR>BUF_SZ{return Err(crate::errno::EMSGSIZE)}
 let mut net=NET.lock();
 if !net.ready{return Err(crate::errno::ENETDOWN)}
 let i=net.tx_next as usize;
 net.tx_next=((i+1)%TX_BUFS)as u8;
 unsafe{
  for b in 0..VNET_HDR{tx_buf(i).add(b).write_volatile(0)}
  core::ptr::copy_nonoverlapping(frame.as_ptr(),tx_buf(i).add(VNET_HDR),frame.len());
  desc_set(tx_ring(),i,virt_to_phys(tx_buf(i)as u64),(frame.len()+VNET_HDR)as u32,0);
  post(net.io,TXQ,tx_ring(),i);
  let mut spins=0u32;
  loop{
   let used=used_idx(tx_ring()).read_volatile();
   if used.wrapping_sub(net.tx_used)>=1{net.tx_used=used;return Ok(())}
   spins+=1;
   if spins>4_000_000{return Err(crate::errno::EIO)}
   core::hint::spin_loop();
  }
 }
}
