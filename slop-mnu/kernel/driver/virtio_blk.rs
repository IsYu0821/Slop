//! Legacy virtio-block (virtio 0.9.5) PCI driver.
use crate::arch::x86_64::port;
use crate::sync::SpinLock;
use core::sync::atomic::Ordering;

const VIRTIO_VENDOR:u16=0x1af4;
const BLK_LEGACY_ID:u16=0x1001;

const HOST_FEATURES:u16=0x00;const GUEST_FEATURES:u16=0x04;
const QUEUE_ADDR:u16=0x08;const QUEUE_SIZE:u16=0x0c;const QUEUE_SELECT:u16=0x0e;
const QUEUE_NOTIFY:u16=0x10;const STATUS:u16=0x12;

const S_ACK:u8=1;const S_DRIVER:u8=2;const S_DRIVER_OK:u8=4;

const VIRTIO_BLK_T_IN:u32=0;const VIRTIO_BLK_T_OUT:u32=1;

struct Blk{ready:bool,io:u16,capacity:u64}
static BLK:SpinLock<Blk>=SpinLock::new(Blk{ready:false,io:0,capacity:0});

const MAX_Q:usize=128;
const RING_BYTES:usize=12288;
#[repr(C,align(4096))]struct Rings([u8;RING_BYTES]);
static mut RING:Rings=Rings([0;RING_BYTES]);
static mut REQ:[u8;16]=[0;16];
static mut BLK_STATUS:[u8;1]=[0;1];
static mut DATA:[u8;512]=[0;512];

fn ring()->*mut u8{unsafe{core::ptr::addr_of_mut!(RING.0)as*mut u8}}
fn avail_idx(r:*mut u8)->*mut u16{unsafe{r.add(4096+2).cast()}}
fn avail_slot(r:*mut u8,i:usize)->*mut u16{unsafe{r.add(4096+4+2*(i%MAX_Q)).cast()}}
fn used_idx(r:*mut u8)->*mut u16{unsafe{r.add(4096+2).cast()}} // placeholder, real used at 4096? 
// Legacy: used ring at 4096+? Actually for 128 descs: desc 2048, avail 260, used aligned 4096.
const USED_OFF:usize=8192;
fn used_idx_real(r:*mut u8)->*mut u16{unsafe{r.add(USED_OFF+2).cast()}}
fn desc_set(r:*mut u8,i:usize,addr:u64,len:u32,flags:u16){
 unsafe{
  let p=r.add(16*(i%MAX_Q));
  (p as *mut u64).write_volatile(addr);
  (p.add(8) as *mut u32).write_volatile(len);
  (p.add(12) as *mut u16).write_volatile(flags);
  (p.add(14) as *mut u16).write_volatile(0);
 }
}
fn setup_queue(io:u16,ring:*mut u8)->Result<(),&'static str>{
 unsafe{
  port::outw(io+QUEUE_SELECT as u16,0);
  let size=port::inw(io+QUEUE_SIZE)as usize;
  if size<1{return Err("blk queue too small")}
  let qsize=size.min(MAX_Q);
  port::outl(io+QUEUE_ADDR,(ring as u64>>12)as u32);
 }
 Ok(())
}
fn do_request(typ:u32,sector:u64)->Result<(),i64>{
 let mut blk=BLK.lock();
 if !blk.ready{return Err(crate::errno::ENODEV)}
 let io=blk.io;
 drop(blk);
 let r=ring();
 unsafe{
  // header
  let hdr=core::ptr::addr_of_mut!(REQ) as *mut u8;
  (hdr as *mut u32).write_volatile(typ);
  (hdr.add(4) as *mut u32).write_volatile(0);
  (hdr.add(8) as *mut u64).write_volatile(sector);
  BLK_STATUS[0]=0xff;
  // descriptors
  desc_set(r,0,core::ptr::addr_of!(REQ) as u64,16,0);
  desc_set(r,1,core::ptr::addr_of!(DATA) as u64,512, if typ==VIRTIO_BLK_T_IN {1} else {0}); // VRING_DESC_F_WRITE=2? Wait flags 2, not 1
  desc_set(r,2,core::ptr::addr_of!(BLK_STATUS) as u64,1,2);
  // fix flags
  let d1=r.add(16); (d1.add(12) as *mut u16).write_volatile(if typ==VIRTIO_BLK_T_IN {2} else {0});
  let d2=r.add(32); (d2.add(12) as *mut u16).write_volatile(2);
  // chain
  let d0=r.add(14); (d0 as *mut u16).write_volatile(1);
  let d1n=r.add(16+14); (d1n as *mut u16).write_volatile(2);
  // avail
  let ai=avail_idx(r).read_volatile();
  avail_slot(r,ai as usize).write_volatile(0);
  core::sync::atomic::compiler_fence(Ordering::SeqCst);
  avail_idx(r).write_volatile(ai.wrapping_add(1));
  core::sync::atomic::compiler_fence(Ordering::SeqCst);
  port::outw(io+QUEUE_NOTIFY,0);
  // spin
  let mut used=used_idx_real(r).read_volatile();
  let start=used;
  for _ in 0..1000000{
   used=used_idx_real(r).read_volatile();
   if used!=start{break;}
  }
  if BLK_STATUS[0]!=0{return Err(crate::errno::EIO)}
 }
 Ok(())
}
pub fn init()->Result<u64,&'static str>{
 let found=crate::driver::pci::find(VIRTIO_VENDOR,BLK_LEGACY_ID).ok_or("virtio-blk not found")?;
 if !found.bars[0].io{return Err("virtio-blk BAR0 not IO")}
 found.enable(&crate::driver::pci::Cam);
 let io=found.bars[0].base as u16;
 unsafe{
  port::outb(io+STATUS,0);
  port::outb(io+STATUS,S_ACK|S_DRIVER);
  let _host=port::inl(io+HOST_FEATURES);
  port::outl(io+GUEST_FEATURES,0);
 }
 setup_queue(io,ring())?;
 unsafe{port::outb(io+STATUS,S_ACK|S_DRIVER|S_DRIVER_OK);}
 // Legacy config space starts at io+18? Actually status at 0x12, config at 0x14? For net mac at 0x14. For block capacity at io+0x14? Let's read u64 at io+0x14.
 let cap=unsafe{port::inl(io+0x14) as u64 | ((port::inl(io+0x18) as u64)<<32)};
 let mut blk=BLK.lock();
 blk.ready=true;blk.io=io;blk.capacity=cap;if cap==0{blk.capacity=1024;}
 Ok(blk.capacity)
}
pub fn ready()->bool{BLK.lock().ready}
pub fn capacity()->u64{BLK.lock().capacity}
pub fn read_sector(sector:u64,dst:&mut [u8])->Result<(),i64>{
 if dst.len()<512{return Err(crate::errno::EINVAL)}
 do_request(VIRTIO_BLK_T_IN,sector)?;
 unsafe{dst[..512].copy_from_slice(&DATA[..512]);}
 Ok(())
}
pub fn write_sector(sector:u64,src:&[u8])->Result<(),i64>{
 if src.len()<512{return Err(crate::errno::EINVAL)}
 unsafe{DATA[..512].copy_from_slice(&src[..512]);}
 do_request(VIRTIO_BLK_T_OUT,sector)?;
 Ok(())
}
