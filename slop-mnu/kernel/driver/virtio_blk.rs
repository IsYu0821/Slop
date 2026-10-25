//! virtio-block driver.
//!
//! The driver speaks both virtio interfaces: the legacy port I/O device
//! (version 0.9.5) and the modern memory mapped device (virtio 1.0), so a
//! plain `virtio-blk-pci` device model works without a compatibility switch.
use crate::arch::x86_64::phys::virt_to_phys;
use crate::arch::x86_64::port;
use crate::driver::virtio::{self,Transport};
use crate::sync::IrqSpinLock;
use core::sync::atomic::Ordering;

const VIRTIO_BLK_T_IN:u32=0;
const VIRTIO_BLK_T_OUT:u32=1;

struct Blk{ready:bool,transport:Transport,capacity:u64,qsize:usize}
static BLK:IrqSpinLock<Blk>=IrqSpinLock::new(Blk{ready:false,transport:Transport::legacy(0),capacity:0,qsize:0});

const MAX_Q:usize=256;
const RING_BYTES:usize=12288;
#[repr(C,align(4096))]struct Rings([u8;RING_BYTES]);
static mut RING:Rings=Rings([0;RING_BYTES]);
/* The offset of the used ring inside the ring buffer. The descriptor table
   and the available ring follow the split virtqueue layout, the used ring
   starts at the next page boundary. */
#[repr(align(16))]struct ReqBuf([u8;16]);
static mut REQ:ReqBuf=ReqBuf([0;16]);
#[repr(align(512))]struct StatusBuf([u8;1]);
static mut BLK_STATUS:StatusBuf=StatusBuf([0;1]);
#[repr(align(512))]struct DataBuf([u8;512]);
static mut DATA:DataBuf=DataBuf([0;512]);

fn ring()->*mut u8{unsafe{core::ptr::addr_of_mut!(RING.0)as*mut u8}}
/* The split virtqueue layout inside the ring buffer: the descriptor table
   comes first, the available ring follows it and the used ring starts at
   the next page boundary. */
fn avail_off(size:usize)->usize{16*size}
fn used_off(size:usize)->usize{((avail_off(size)+6+2*size)+4095)&!4095}
fn avail_idx(r:*mut u8,size:usize)->*mut u16{unsafe{r.add(avail_off(size)+2).cast()}}
fn avail_slot(r:*mut u8,size:usize,i:usize)->*mut u16{unsafe{r.add(avail_off(size)+4+2*(i%size)).cast()}}
fn used_idx(r:*mut u8,size:usize)->*mut u16{unsafe{r.add(used_off(size)+2).cast()}}
fn used_entry(r:*mut u8,size:usize,i:usize)->(u32,u32){unsafe{let p=r.add(used_off(size)+4+8*(i%size));((p as*const u32).read_volatile(),(p.add(4)as*const u32).read_volatile())}}
fn desc_set(r:*mut u8,size:usize,i:usize,addr:u64,len:u32,flags:u16,next:u16){
 unsafe{
  let p=r.add(16*(i%size));
  (p as*mut u64).write_volatile(addr);
  (p.add(8)as*mut u32).write_volatile(len);
  (p.add(12)as*mut u16).write_volatile(flags);
  (p.add(14)as*mut u16).write_volatile(next);
 }
}

/// Builds the queue description and installs it in the device.
fn setup_queue(t:&Transport,size:usize)->Result<(),&'static str>{
 let r=ring();
 let desc=r;
 let avail=unsafe{r.add(avail_off(size))};
 let used=unsafe{r.add(used_off(size))};
 let q=virtio::Queue{desc,avail,used,size};
 t.queue_setup(0,&q);
 Ok(())
}

fn do_request(typ:u32,sector:u64)->Result<(),i64>{
 let (t,size)={
  let blk=BLK.lock();
  if !blk.ready{return Err(crate::errno::ENODEV)}
  (blk.transport,blk.qsize)
 };
 let r=ring();
 unsafe{
  // The request header: type, reserved, sector.
  let hdr=core::ptr::addr_of_mut!(REQ.0)as*mut u8;
  (hdr as*mut u32).write_volatile(typ);
  (hdr.add(4)as*mut u32).write_volatile(0);
  (hdr.add(8)as*mut u64).write_volatile(sector);
  BLK_STATUS.0[0]=0xff;
  // The descriptor chain: header, data, status.
  desc_set(r,size,0,virt_to_phys(core::ptr::addr_of!(REQ.0)as u64),16,1,1);
  desc_set(r,size,1,virt_to_phys(core::ptr::addr_of!(DATA.0)as u64),512,if typ==VIRTIO_BLK_T_IN{3}else{1},2);
  desc_set(r,size,2,virt_to_phys(core::ptr::addr_of!(BLK_STATUS.0)as u64),1,2,0);
  // Queue one attempt and wait for its completion. The device side runs
  // while the vCPU is outside the guest, so halt regularly to hand the CPU
  // back instead of spinning a fixed count that can expire first under TCG.
  // Sector reads and writes are idempotent, so a bounded re-ring on a
  // missed completion is safe.
  let mut attempt=0;
  loop{
   let ai=avail_idx(r,size).read_volatile();
   avail_slot(r,size,ai as usize).write_volatile(0);
   core::sync::atomic::compiler_fence(Ordering::SeqCst);
   avail_idx(r,size).write_volatile(ai.wrapping_add(1));
   core::sync::atomic::compiler_fence(Ordering::SeqCst);
   t.notify(0);
   let start=used_idx(r,size).read_volatile();
   let mut used=start;
   let mut round=0u32;
   while used==start&&round<20000{
    used=used_idx(r,size).read_volatile();
    if used!=start{break}
    round+=1;
    for _ in 0..20000{core::hint::spin_loop();}
    if round%8==7{unsafe{core::arch::asm!("sti","hlt","cli",options(nomem,nostack))};}
   }
   if used!=start{break}
   attempt+=1;
   if attempt>=3{return Err(crate::errno::EIO)}
  }
  let _=used_entry(r,size,used_idx(r,size).read_volatile().wrapping_sub(1)as usize);
  let sb=unsafe{core::ptr::addr_of!(BLK_STATUS.0[0]).read_volatile()};
  if sb!=0{return Err(crate::errno::EIO)}
 }
 Ok(())
}

/// Brings up the block device and returns its capacity in sectors.
pub fn init()->Result<u64,&'static str>{
 let t=match virtio::probe(virtio::MODERN_BLK).or_else(||virtio::probe(virtio::LEGACY_BLK)){Some(t)=>t,None=>return Err("virtio-blk not found")};
 t.set_status(0);
 t.set_status(virtio::S_ACK);
 t.set_status(virtio::S_ACK|virtio::S_DRIVER);
 if t.modern{
  if !t.negotiate_modern(){return Err("virtio-blk feature negotiation failed")}
 }else{
  let _=t.read_features(0);
  t.set_features(0,0);
 }
 let size=t.queue_size(0,MAX_Q);
 if size<1{return Err("virtio-blk queue too small")}
 if size>MAX_Q{return Err("virtio-blk queue larger than the ring buffer")}
 setup_queue(&t,size)?;
 t.set_status(virtio::S_ACK|virtio::S_DRIVER|virtio::S_DRIVER_OK);
 let mut cap=t.device_config(0);
 if cap==0{
  // The legacy device reports the capacity in two words.
  unsafe{cap=port::inl(t.io+0x14)as u64|((port::inl(t.io+0x18)as u64)<<32);}
 }
 let mut blk=BLK.lock();
 blk.ready=true;blk.transport=t;blk.capacity=cap;blk.qsize=size;if cap==0{blk.capacity=1024;}
 Ok(blk.capacity)
}
pub fn ready()->bool{BLK.lock().ready}
pub fn capacity()->u64{BLK.lock().capacity}
pub fn read_sector(sector:u64,dst:&mut [u8])->Result<(),i64>{
 if dst.len()<512{return Err(crate::errno::EINVAL)}
 do_request(VIRTIO_BLK_T_IN,sector)?;
 unsafe{dst[..512].copy_from_slice(&DATA.0[..512]);}
 Ok(())
}
pub fn write_sector(sector:u64,src:&[u8])->Result<(),i64>{
 if src.len()<512{return Err(crate::errno::EINVAL)}
 unsafe{DATA.0[..512].copy_from_slice(&src[..512]);}
 match do_request(VIRTIO_BLK_T_OUT,sector){Ok(())=>Ok(()),Err(e)=>Err(e)}
}
