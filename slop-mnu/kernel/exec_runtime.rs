//! Commits an ELF64 image (static or dynamic) to a private bootstrap CR3 with
//! rollback on failure. Dynamic images load their PT_INTERP interpreter first
//! and enter through its entry point with AT_BASE set.
use crate::{arch::x86_64::{paging::{self,PageTableRoot},phys::Identity},elf,linux_abi::stack,memory};
const PAGE:u64=4096;const MAX_COMMITTED:usize=512;const USER_STACK_PAGES:u64=8;
/* Load bias for the dynamic linker and for the PIE main image. Every user
   region sits alone in a page table slot: the linker in slot 128, the image
   in slot 129, the anonymous mappings in slot 130 and the program break in
   slot 131. Regions that share a slot also share the lower level indices,
   so a mapping in one of them would overwrite the page table entry of the
   other. */
const LD_BASE:u64=0x0000_4000_0000_0000;
const EXE_BASE:u64=0x0000_4080_0000_0000;
/* One committed PT_LOAD range, kept for /proc/<pid>/maps. */
#[derive(Clone,Copy,Default)]pub struct MapSeg{pub start:u64,pub end:u64,pub flags:u32}
pub const MAX_MAP_SEGS:usize=8;
#[derive(Clone,Copy)]pub struct Loaded{pub entry:u64,pub rsp:u64,pub cr3:u64,pub bootstrap_cr3:u64,pub image_slot:usize,pub stack_slot:usize,pub interp_slot:usize,
 pub cmdline:[u8;512],pub cmdline_len:usize,pub environ:[u8;512],pub environ_len:usize,
 pub segs:[MapSeg;MAX_MAP_SEGS],pub nsegs:usize,pub stack_base:u64,pub stack_top:u64}
fn pack_vec(out:&mut [u8],out_len:&mut usize,items:&[&str]){
 for s in items{let b=s.as_bytes();if *out_len+b.len()+1>out.len(){return}out[*out_len..*out_len+b.len()].copy_from_slice(b);*out_len+=b.len();out[*out_len]=0;*out_len+=1;}
}
#[derive(Clone,Copy)]struct Owned{va:u64,pa:u64}const EMPTY:Owned=Owned{va:0,pa:0};
struct Transaction<'a>{root:&'a PageTableRoot,phys:&'a Identity,pages:[Owned;MAX_COMMITTED],len:usize,committed:bool}
impl<'a>Transaction<'a>{fn new(root:&'a PageTableRoot,phys:&'a Identity)->Self{Self{root,phys,pages:[EMPTY;MAX_COMMITTED],len:0,committed:false}}fn page(&mut self,va:u64,flags:u64)->Result<u64,&'static str>{/* Always map a fresh frame: the address may still hold the old image's
           page, and the exec teardown releases those frames afterwards. */
 if self.len==MAX_COMMITTED{return Err("ELF transaction page limit")}let pa=memory::alloc_frame().ok_or("out of physical frames")?;unsafe{self.phys.bytes(pa,PAGE as usize).fill(0)};if self.root.map_page(self.phys,va,pa,flags).is_err(){let _=memory::free_frame(pa);return Err("map_page failed")}self.pages[self.len]=Owned{va,pa};self.len+=1;Ok(pa)}fn commit(mut self){self.committed=true;}}
impl Drop for Transaction<'_>{fn drop(&mut self){if self.committed{return}for p in self.pages[..self.len].iter().rev(){let _=self.root.unmap_page(self.phys,p.va);let _=memory::free_frame(p.pa);}}}
pub unsafe fn load_static(image:&[u8])->Result<Loaded,&'static str>{unsafe{load_static_args(image,&["/init"],&[],"/init",PageTableRoot::current().pml4)}}
pub unsafe fn load_static_args(image:&[u8],argv:&[&str],envp:&[&str],execfn:&str,bootstrap_cr3:u64)->Result<Loaded,&'static str>{
 let parsed=elf::plan(image)?;let access=Identity;let firmware=PageTableRoot::current();
 /* A PT_INTERP segment means dynamic linking: load the interpreter from the
    initramfs, then the main image above it, and enter via the interpreter. */
 let interp_bytes=if parsed.has_interpreter{
  let path=elf::interp_path(image,&parsed)?;
  Some(crate::vfs::image(path).map_err(|_|"interpreter not in initramfs")?)
 }else{None};
 let interp=match interp_bytes{Some(b)=>Some(elf::plan(b).map_err(|_|"bad interpreter ELF")?),None=>None};
 let is_pie=parsed.header.kind==elf::ET_DYN;
 let exe_base=if is_pie{EXE_BASE}else{0};
 let entry=match interp.as_ref(){Some(ip)=>LD_BASE+ip.header.entry,None=>exe_base+parsed.header.entry};
 /* The interpreter and the main image live in page table slots of their own,
    so a mapping in one of them never overwrites the page table entry of the
    other. A static ET_EXEC keeps its absolute entry-derived slot. */
 let image_slot=if is_pie{((exe_base>>39)&511)as usize}else{((parsed.header.entry>>39)&511)as usize};
 let interp_slot=((LD_BASE>>39)&511)as usize;
 let stack_slot=((crate::exec::USER_STACK_TOP>>39)&511)as usize;
 let mmap_slot=((crate::vm::MMAP_BASE>>39)&511)as usize;
 let brk_slot=((crate::process::BRK_BASE>>39)&511)as usize;
 let mut private=[image_slot,stack_slot,mmap_slot,interp_slot,brk_slot,crate::vdso::VDSO_SLOT,crate::vdso::VVAR_SLOT];
 let root=firmware.clone_bootstrap(&access,&private).map_err(|_|"private CR3 allocation failed")?;
 let mut tx=Transaction::new(&root,&access);
 if let Some(ip)=interp.as_ref(){
  let bytes=interp_bytes.ok_or("interpreter image lost")?;
  for seg in &ip.segments[..ip.segment_count]{
   if ((LD_BASE+seg.vaddr>>39)&511)as usize!=interp_slot{return Err("interpreter crosses private PML4 slot")}
   map_segment(&mut tx,&root,&access,bytes,LD_BASE,seg)?;
  }
 }
 for seg in &parsed.segments[..parsed.segment_count]{
  if ((exe_base+seg.vaddr>>39)&511)as usize!=image_slot{return Err("ELF crosses private PML4 slot")}
  map_segment(&mut tx,&root,&access,image,exe_base,seg)?;
 }
 let stack_top=crate::exec::USER_STACK_TOP;let stack_base=stack_top-USER_STACK_PAGES*PAGE;
 for i in 0..USER_STACK_PAGES{let pa=tx.page(stack_base+i*PAGE,paging::USER|paging::WRITABLE|paging::NX)?;unsafe{access.bytes(pa,PAGE as usize).fill(0)};}
 let stack_pa=root.translate(&access,stack_top-PAGE).ok_or("stack mapping disappeared")?;
 let page=unsafe{access.bytes(stack_pa,PAGE as usize)};
 /* AT_PHDR and AT_ENTRY describe the main image; AT_BASE is the interpreter. */
 let phoff=parsed.header.phoff;
 let phdr_vaddr=parsed.segments[..parsed.segment_count].iter().find_map(|s|{
  if phoff>=s.file_offset&&phoff<s.file_offset+s.file_size{Some(exe_base+s.vaddr+(phoff-s.file_offset))}else{None}
 }).ok_or("program headers not mapped")?;
 /* The vector system call entry point and the shared page that holds the
    time base are mapped into every process, and the auxiliary vector points
    at the entry point, so the C library finds the clock entry points. */
 crate::vdso::map_into(&root,&access).map_err(|_|"the vector system call entry point could not be mapped")?;
 let info=stack::ImageInfo{phdr:phdr_vaddr,phent:parsed.header.phentsize as u64,phnum:parsed.header.phnum as u64,entry:exe_base+parsed.header.entry,base:if interp.is_some(){LD_BASE}else{0},vdso:crate::vdso::VDSO_BASE};
  let built=stack::build(page,stack_top-PAGE,argv,envp,execfn,[0x4e;16],info).map_err(|_|"stack build failed")?;
  let mut segs=[MapSeg::default();MAX_MAP_SEGS];let mut nsegs=0usize;
  if let Some(ip)=interp.as_ref(){for seg in &ip.segments[..ip.segment_count]{if nsegs==MAX_MAP_SEGS{break}segs[nsegs]=MapSeg{start:LD_BASE+seg.vaddr&!(PAGE-1),end:align(LD_BASE+seg.vaddr+seg.memory_size).map_err(|_|"segment overflow")?,flags:seg.flags};nsegs+=1;}}
  for seg in &parsed.segments[..parsed.segment_count]{if nsegs==MAX_MAP_SEGS{break}segs[nsegs]=MapSeg{start:exe_base+seg.vaddr&!(PAGE-1),end:align(exe_base+seg.vaddr+seg.memory_size).map_err(|_|"segment overflow")?,flags:seg.flags};nsegs+=1;}
  let mut cmdline=[0u8;512];let mut cmdline_len=0usize;pack_vec(&mut cmdline,&mut cmdline_len,argv);
  let mut environ=[0u8;512];let mut environ_len=0usize;pack_vec(&mut environ,&mut environ_len,envp);
  let loaded=Loaded{entry,rsp:built.rsp,cr3:root.pml4,bootstrap_cr3,image_slot,stack_slot,interp_slot,cmdline,cmdline_len,environ,environ_len,segs,nsegs,stack_base,stack_top};tx.commit();Ok(loaded)
}
/* Maps one PT_LOAD at base+seg.vaddr, copying file bytes and zeroing the rest. */
fn map_segment(tx:&mut Transaction,root:&PageTableRoot,access:&Identity,image:&[u8],base:u64,seg:&elf::LoadSegment)->Result<(),&'static str>{
 let first=base+seg.vaddr&!(PAGE-1);
 let last=align(base+seg.vaddr.checked_add(seg.memory_size).ok_or("segment overflow")?)?;
 let mut va=first;
 while va<last{tx.page(va,flags(seg.flags))?;va+=PAGE;}
 for i in 0..seg.file_size{let pa=root.translate(access,base+seg.vaddr+i).ok_or("ELF mapping disappeared")?;unsafe{access.bytes(pa,1)[0]=image[(seg.file_offset+i)as usize];}}
 for i in seg.file_size..seg.memory_size{let pa=root.translate(access,base+seg.vaddr+i).ok_or("BSS mapping disappeared")?;unsafe{access.bytes(pa,1)[0]=0;}}
 Ok(())
}
fn flags(pf:u32)->u64{let mut f=paging::USER;if pf&elf::PF_W!=0{f|=paging::WRITABLE}if pf&elf::PF_X==0{f|=paging::NX}f}
fn align(v:u64)->Result<u64,&'static str>{v.checked_add(PAGE-1).map(|x|x&!(PAGE-1)).ok_or("alignment overflow")}
