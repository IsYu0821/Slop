//! Commits a static ELF64 to a private bootstrap CR3 with rollback on failure.
use crate::{arch::x86_64::{paging::{self,PageTableRoot},phys::Identity},elf,linux_abi::stack,memory};
const PAGE:u64=4096;const MAX_COMMITTED:usize=256;const USER_STACK_PAGES:u64=8;
#[derive(Clone,Copy)]pub struct Loaded{pub entry:u64,pub rsp:u64,pub cr3:u64,pub bootstrap_cr3:u64,pub image_slot:usize,pub stack_slot:usize}
#[derive(Clone,Copy)]struct Owned{va:u64,pa:u64}const EMPTY:Owned=Owned{va:0,pa:0};
struct Transaction<'a>{root:&'a PageTableRoot,phys:&'a Identity,pages:[Owned;MAX_COMMITTED],len:usize,committed:bool}
impl<'a>Transaction<'a>{fn new(root:&'a PageTableRoot,phys:&'a Identity)->Self{Self{root,phys,pages:[EMPTY;MAX_COMMITTED],len:0,committed:false}}fn page(&mut self,va:u64,flags:u64)->Result<u64,&'static str>{if let Some(pa)=self.root.translate(self.phys,va){return Ok(pa&!(PAGE-1))}if self.len==MAX_COMMITTED{return Err("ELF transaction page limit")}let pa=memory::alloc_frame().ok_or("out of physical frames")?;unsafe{self.phys.bytes(pa,PAGE as usize).fill(0)};if self.root.map_page(self.phys,va,pa,flags).is_err(){let _=memory::free_frame(pa);return Err("map_page failed")}self.pages[self.len]=Owned{va,pa};self.len+=1;Ok(pa)}fn commit(mut self){self.committed=true;}}
impl Drop for Transaction<'_>{fn drop(&mut self){if self.committed{return}for p in self.pages[..self.len].iter().rev(){let _=self.root.unmap_page(self.phys,p.va);let _=memory::free_frame(p.pa);}}}
pub unsafe fn load_static(image:&[u8])->Result<Loaded,&'static str>{unsafe{load_static_args(image,&["/init"],&[],"/init",PageTableRoot::current().pml4)}}
pub unsafe fn load_static_args(image:&[u8],argv:&[&str],envp:&[&str],execfn:&str,bootstrap_cr3:u64)->Result<Loaded,&'static str>{
 let ep=crate::exec::plan(image)?;if ep.needs_interpreter{return Err("dynamic ELF not supported")}
 let parsed=elf::plan(image)?;let access=Identity;let firmware=PageTableRoot::current();
 let image_slot=((ep.entry>>39)&511)as usize;let stack_slot=((crate::exec::USER_STACK_TOP>>39)&511)as usize;
 if image_slot==0{return Err("ELF must not share bootstrap identity slot")}
 let mmap_slot=((crate::vm::MMAP_BASE>>39)&511)as usize;let root=firmware.clone_bootstrap(&access,&[image_slot,stack_slot,mmap_slot]).map_err(|_|"private CR3 allocation failed")?;
 let mut tx=Transaction::new(&root,&access);
 for seg in &parsed.segments[..parsed.segment_count]{
  if ((seg.vaddr>>39)&511)as usize!=image_slot{return Err("ELF crosses private PML4 slot")}
  let first=seg.vaddr&!(PAGE-1);let last=align(seg.vaddr.checked_add(seg.memory_size).ok_or("segment overflow")?)?;let mut va=first;
  while va<last{tx.page(va,flags(seg.flags))?;va+=PAGE;}
  for i in 0..seg.file_size{let pa=root.translate(&access,seg.vaddr+i).ok_or("ELF mapping disappeared")?;unsafe{access.bytes(pa,1)[0]=image[(seg.file_offset+i)as usize];}}
  for i in seg.file_size..seg.memory_size{let pa=root.translate(&access,seg.vaddr+i).ok_or("BSS mapping disappeared")?;unsafe{access.bytes(pa,1)[0]=0;}}
 }
 let stack_top=crate::exec::USER_STACK_TOP;let stack_base=stack_top-USER_STACK_PAGES*PAGE;
 for i in 0..USER_STACK_PAGES{let pa=tx.page(stack_base+i*PAGE,paging::USER|paging::WRITABLE|paging::NX)?;unsafe{access.bytes(pa,PAGE as usize).fill(0)};}
 let stack_pa=root.translate(&access,stack_top-PAGE).ok_or("stack mapping disappeared")?;
 let page=unsafe{access.bytes(stack_pa,PAGE as usize)};
 let built=stack::build(page,stack_top-PAGE,argv,envp,execfn,[0x4e;16],crate::exec::stack_info(&ep)).map_err(|_|"stack build failed")?;
 let loaded=Loaded{entry:ep.entry,rsp:built.rsp,cr3:root.pml4,bootstrap_cr3,image_slot,stack_slot};tx.commit();Ok(loaded)
}
fn flags(pf:u32)->u64{let mut f=paging::USER;if pf&elf::PF_W!=0{f|=paging::WRITABLE}if pf&elf::PF_X==0{f|=paging::NX}f}
fn align(v:u64)->Result<u64,&'static str>{v.checked_add(PAGE-1).map(|x|x&!(PAGE-1)).ok_or("alignment overflow")}
