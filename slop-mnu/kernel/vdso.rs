//! The vector system call entry point.
//!
//! Linux gives every program a page of code that it can call without
//! entering the kernel, and the auxiliary vector points at it. The C
//! library looks the entry points up in the dynamic symbol table of that
//! page and uses them for the clock, so a program that reads the clock does
//! not pay for a system call, and the time it spends is charged to the
//! program instead of the kernel.
//!
//! The page carries the entry points __vdso_clock_gettime,
//! __vdso_gettimeofday, __vdso_time and __vdso_getcpu, the same names Linux
//! uses, and the C library looks them up by name.
//!
//! The page needs the current time, and it cannot ask the kernel for it
//! without defeating its purpose. So the kernel keeps a shared page up to
//! date: the rate of the time stamp counter, the value of the counter at
//! the last update, and the monotonic and the wall clock at that moment.
//! The entry point reads the page under a sequence lock and adds the time
//! that passed since the last update, measured with the counter.

use crate::arch::x86_64::phys::{phys_to_virt,Identity};
use crate::arch::x86_64::paging::{self,PageTableRoot};
use core::sync::atomic::{AtomicU64,Ordering};

/// The address of the entry point in every process. It sits alone in a
/// page table slot, so a mapping of the program can never overwrite it.
pub const VDSO_BASE:u64=0x0000_6000_0000_0000;
pub const VDSO_SLOT:usize=((VDSO_BASE>>39)&511)as usize;
/// The address of the shared page that holds the time base. It sits alone
/// in a page table slot of its own, beside the one of the entry point.
pub const VVAR_BASE:u64=0x0000_6100_0000_0000;
pub const VVAR_SLOT:usize=((VVAR_BASE>>39)&511)as usize;
/// The path of the entry point in the initramfs.
pub const VDSO_PATH:&str="/lib/vdso.so";
const PAGE:u64=4096;

/// The shared page. The kernel writes it, the entry point reads it.
#[repr(C,align(4096))]
pub struct Vvar{
 /// The number of the update. A reader that sees an odd number waits.
 pub seq:u64,
 /// The nanoseconds per counter cycle, with 32 fractional bits.
 pub scale:u64,
 /// The value of the counter at the last update.
 pub tsc_base:u64,
 /// The monotonic nanoseconds at the last update.
 pub mono_base:u64,
 /// The wall clock nanoseconds at the last update.
 pub real_base:u64,
}

static VVAR_FRAME:AtomicU64=AtomicU64::new(0);
static VVAR_READY:AtomicU64=AtomicU64::new(0);

/// Reserves the shared page and the physical frame behind it.
pub fn init(){
 if VVAR_READY.load(Ordering::Relaxed)!=0{return}
 let frame=match crate::memory::alloc_frame(){Some(f)=>f,None=>{crate::println!("vdso: the shared page could not be reserved");return}};
 unsafe{Identity.bytes(frame,PAGE as usize).fill(0)};
 VVAR_FRAME.store(frame,Ordering::Relaxed);
 VVAR_READY.store(1,Ordering::Relaxed);
 update();
}

/// Writes the time base into the shared page. The page is written under a
/// sequence lock: a reader that sees the number change reads the page
/// again.
pub fn update(){
 let frame=VVAR_FRAME.load(Ordering::Relaxed);
 if frame==0{return}
 let v=unsafe{&mut *(phys_to_virt(frame) as *mut Vvar)};
 let seq=v.seq;
 v.seq=seq+1;
 core::sync::atomic::compiler_fence(Ordering::SeqCst);
 v.tsc_base=crate::arch::x86_64::tsc::rdtsc();
 v.mono_base=crate::time::monotonic_ns();
 v.real_base=crate::time::realtime_ns();
 core::sync::atomic::compiler_fence(Ordering::SeqCst);
 v.seq=seq+2;
}

/// Records the rate of the time stamp counter in the shared page. The
/// entry point needs it to turn the counter into nanoseconds.
pub fn set_scale(){
 let frame=VVAR_FRAME.load(Ordering::Relaxed);
 if frame==0{return}
 let hz=crate::time::tsc_hz();
 if hz==0{return}
 let v=unsafe{&mut *(phys_to_virt(frame) as *mut Vvar)};
 /* The nanoseconds per cycle with 32 fractional bits, so the entry point
    can multiply and shift instead of dividing. */
 v.scale=(1_000_000_000u64<<32)/hz;
}

/// Takes the entry point and the shared page out of an address space. The
/// frame behind the shared page belongs to the kernel, so it is only
/// counted down and not released.
pub fn unmap_from(root:&PageTableRoot,access:&Identity){
 let frame=VVAR_FRAME.load(Ordering::Relaxed);
 if frame!=0{
  if let Ok(pa)=root.unmap_page(access,VVAR_BASE){if pa==frame{let _=crate::memory::release_frame(pa);}}
 }
 let _=root.destroy_private_slot(access,VDSO_SLOT);
 let _=root.destroy_private_slot(access,VVAR_SLOT);
}

/// Maps the entry point and the shared page into an address space. The
/// shared page is the same physical frame in every process, so the kernel
/// can write it without knowing which process runs.
pub fn map_into(root:&PageTableRoot,access:&Identity)->Result<(),i64>{
 let frame=VVAR_FRAME.load(Ordering::Relaxed);
 if frame==0{return Err(crate::errno::ENODEV)}
 /* The shared page is read only for the program, like the page Linux
    keeps the time in. */
 if let Err(e)=root.map_page(access,VVAR_BASE,frame,paging::USER|paging::PRESENT){crate::println!("vdso: the shared page could not be mapped errno={}",e);return Err(e)}
 /* The frame is counted, so the last process that goes away does not take
    the page away from the kernel. */
 crate::memory::refcount_inc(frame);
 let bytes=match crate::vfs::image(VDSO_PATH){Ok(v)=>v,Err(e)=>{crate::println!("vdso: the entry point is not in the initramfs errno={}",e);return Err(e)}};
 let plan=match crate::elf::plan(&bytes){Ok(p)=>p,Err(e)=>{crate::println!("vdso: the entry point is not a usable image: {}",e);return Err(crate::errno::ENOEXEC)}};

 /* Every page of the file is mapped at the address of its offset, and the
    pages that hold code are executable. The file offset and the address of
    a loadable segment are the same, so the C library finds the entry
    points where the kernel put them. */
 let pages=(bytes.len() as u64+PAGE-1)/PAGE;
 for p in 0..pages{
  let va=VDSO_BASE+p*PAGE;
  let exec=plan.segments[..plan.segment_count].iter().any(|s|{
   s.vaddr<(p+1)*PAGE&&s.vaddr+s.memory_size>p*PAGE&&s.flags&crate::elf::PF_X!=0
  });
  let pa=match crate::memory::alloc_frame(){Some(f)=>f,None=>return Err(crate::errno::ENOMEM)};
  unsafe{
   let page=access.bytes(pa,PAGE as usize);
   page.fill(0);
   let from=(p*PAGE) as usize;
   let to=core::cmp::min(from+PAGE as usize,bytes.len());
   if to>from{page[..to-from].copy_from_slice(&bytes[from..to]);}
  }
  let flags=paging::USER|paging::PRESENT|if exec{0}else{paging::NX};
  if let Err(e)=root.map_page(access,va,pa,flags){let _=crate::memory::free_frame(pa);return Err(e)}
 }
 Ok(())
}
