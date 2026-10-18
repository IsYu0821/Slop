use crate::{boot::MemoryRegion,sync::SpinLock};
pub const PAGE_SIZE:u64=4096;pub const USABLE:u32=1;const MAX_RANGES:usize=128;const RECYCLED:usize=65_536;
#[derive(Clone,Copy)]struct Range{next:u64,end:u64}const EMPTY:Range=Range{next:0,end:0};
struct State{ranges:[Range;MAX_RANGES],len:usize,cursor:usize,free:[u64;RECYCLED],free_len:usize}
static ALLOC:SpinLock<State>=SpinLock::new(State{ranges:[EMPTY;MAX_RANGES],len:0,cursor:0,free:[0;RECYCLED],free_len:0});
pub fn init(map:&[MemoryRegion])->Result<u64,&'static str>{let mut s=ALLOC.lock();s.len=0;s.cursor=0;s.free_len=0;let mut pages=0;for r in map.iter().filter(|r|r.kind==USABLE&&r.pages!=0){if s.len==MAX_RANGES{return Err("too many memory ranges")}let start=align_up(r.base,PAGE_SIZE).ok_or("memory range overflow")?;let raw_end=r.base.checked_add(r.pages.checked_mul(PAGE_SIZE).ok_or("memory range overflow")?).ok_or("memory range overflow")?;let end=raw_end&!(PAGE_SIZE-1);if start<end{let i=s.len;s.ranges[i]=Range{next:start,end};s.len+=1;pages+=(end-start)/PAGE_SIZE;}}Ok(pages)}
pub fn alloc_frame()->Option<u64>{let mut s=ALLOC.lock();if s.free_len!=0{s.free_len-=1;return Some(s.free[s.free_len])}while s.cursor<s.len{let i=s.cursor;let r=s.ranges[i];if r.next<r.end{s.ranges[i].next+=PAGE_SIZE;return Some(r.next)}s.cursor+=1}None}
pub fn free_frame(pa:u64)->Result<(),i64>{if pa&PAGE_SIZE-1!=0{return Err(crate::errno::EINVAL)}let mut s=ALLOC.lock();if s.free_len==RECYCLED{return Err(crate::errno::ENOMEM)}if s.free[..s.free_len].contains(&pa){return Err(crate::errno::EINVAL)}let i=s.free_len;s.free[i]=pa;s.free_len+=1;Ok(())}
fn align_up(v:u64,a:u64)->Option<u64>{v.checked_add(a-1).map(|x|x&!(a-1))}
