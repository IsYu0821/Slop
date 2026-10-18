use crate::{boot::MemoryRegion,sync::SpinLock};
pub const PAGE_SIZE:u64=4096;pub const USABLE:u32=1;const MAX_RANGES:usize=128;const RECYCLED:usize=65_536;const MAX_FRAMES:usize=65_536;
#[derive(Clone,Copy)]struct Range{next:u64,end:u64}const EMPTY:Range=Range{next:0,end:0};
struct State{ranges:[Range;MAX_RANGES],len:usize,cursor:usize,total:u64,free:[u64;RECYCLED],free_len:usize}
static ALLOC:SpinLock<State>=SpinLock::new(State{ranges:[EMPTY;MAX_RANGES],len:0,cursor:0,total:0,free:[0;RECYCLED],free_len:0});
static REFCOUNTS:SpinLock<[u16;MAX_FRAMES]>=SpinLock::new([0;MAX_FRAMES]);

pub fn init(map:&[MemoryRegion])->Result<u64,&'static str>{let mut s=ALLOC.lock();s.len=0;s.cursor=0;s.free_len=0;let mut pages=0;for r in map.iter().filter(|r|r.kind==USABLE&&r.pages!=0){if s.len==MAX_RANGES{return Err("too many memory ranges")}let start=align_up(r.base,PAGE_SIZE).ok_or("memory range overflow")?;let raw_end=r.base.checked_add(r.pages.checked_mul(PAGE_SIZE).ok_or("memory range overflow")?).ok_or("memory range overflow")?;let end=raw_end&!(PAGE_SIZE-1);if start<end{let i=s.len;s.ranges[i]=Range{next:start,end};s.len+=1;pages+=(end-start)/PAGE_SIZE;}}s.total=pages;Ok(pages)}

pub fn alloc_frame()->Option<u64>{
 let pa={
  let mut s=ALLOC.lock();
  if s.free_len!=0{s.free_len-=1;Some(s.free[s.free_len])}
  else{
   let mut found=None;
   while s.cursor<s.len{
    let i=s.cursor;let r=s.ranges[i];
    if r.next<r.end{s.ranges[i].next+=PAGE_SIZE;found=Some(r.next);break}
    s.cursor+=1;
   }
   found
  }
 };
 let pa=pa?;
 let idx=(pa/PAGE_SIZE)as usize;
 if idx<MAX_FRAMES{REFCOUNTS.lock()[idx]=1;}
 Some(pa)
}

pub fn free_frame(pa:u64)->Result<(),i64>{if pa&(PAGE_SIZE-1)!=0{return Err(crate::errno::EINVAL)}let mut s=ALLOC.lock();if s.free_len==RECYCLED{return Err(crate::errno::ENOMEM)}if s.free[..s.free_len].contains(&pa){return Err(crate::errno::EINVAL)}let i=s.free_len;s.free[i]=pa;s.free_len+=1;Ok(())}

/* Total and free physical frames, for /proc/meminfo and sysinfo. */
pub fn frame_stats()->(u64,u64){let s=ALLOC.lock();let mut free=0u64;for r in &s.ranges[..s.len]{free=free.saturating_add((r.end-r.next)/PAGE_SIZE);}(s.total,free.saturating_add(s.free_len as u64))}

pub fn refcount(pa:u64)->u16{let idx=(pa/PAGE_SIZE)as usize;if idx>=MAX_FRAMES{return 0}REFCOUNTS.lock()[idx]}

pub fn refcount_inc(pa:u64){let idx=(pa/PAGE_SIZE)as usize;if idx<MAX_FRAMES{let mut r=REFCOUNTS.lock();if r[idx]>0{r[idx]=r[idx].saturating_add(1)}}}

pub fn refcount_dec(pa:u64)->bool{let idx=(pa/PAGE_SIZE)as usize;if idx>=MAX_FRAMES{return true}let mut r=REFCOUNTS.lock();if r[idx]==0{return true}r[idx]-=1;r[idx]==0}

pub fn release_frame(pa:u64)->Result<(),i64>{if pa==0{return Ok(())}if pa&(PAGE_SIZE-1)!=0{return Err(crate::errno::EINVAL)}if refcount_dec(pa){free_frame(pa)}else{Ok(())}}

pub fn refcount_self_test()->bool{
 let pa=match alloc_frame(){Some(p)=>p,None=>return false};
 if refcount(pa)!=1{return false}
 refcount_inc(pa);
 if refcount(pa)!=2{return false}
 if refcount_dec(pa){return false}
 if refcount(pa)!=1{return false}
 if !refcount_dec(pa){return false}
 if free_frame(pa).is_err(){return false}
 if alloc_frame().is_none(){return false}
 true
}

fn align_up(v:u64,a:u64)->Option<u64>{v.checked_add(a-1).map(|x|x&!(a-1))}