//! Allocation-free initramfs VFS with read/write support.
use crate::{arch::x86_64::{paging::PhysAccess,phys::Identity},errno,sync::SpinLock};
pub type Result<T>=core::result::Result<T,i64>;
pub const S_IFREG:u32=0o100000;pub const S_IFDIR:u32=0o040000;const MAX_NODES:usize=32;const NAME_MAX:usize=127;
#[derive(Clone,Copy)]pub struct Inode{pub ino:u64,pub mode:u32,pub size:u64,pub capacity:u64,pub data:*mut u8,pub name:[u8;NAME_MAX+1],pub name_len:usize}
unsafe impl Send for Inode{} unsafe impl Sync for Inode{}
const EMPTY:Inode=Inode{ino:0,mode:0,size:0,capacity:0,data:core::ptr::null_mut(),name:[0;NAME_MAX+1],name_len:0};
struct Mount{nodes:[Inode;MAX_NODES],len:usize,mounted:bool}
static ROOT:SpinLock<Mount>=SpinLock::new(Mount{nodes:[EMPTY;MAX_NODES],len:0,mounted:false});
fn u16le(b:&[u8],o:usize)->Result<u16>{let x=b.get(o..o+2).ok_or(errno::EINVAL)?;Ok(u16::from_le_bytes([x[0],x[1]]))}
fn u32le(b:&[u8],o:usize)->Result<u32>{let x=b.get(o..o+4).ok_or(errno::EINVAL)?;Ok(u32::from_le_bytes([x[0],x[1],x[2],x[3]]))}
fn align8(v:usize)->Result<usize>{v.checked_add(7).map(|x|x&!7).ok_or(errno::EINVAL)}
pub fn mount_initramfs(image:&'static [u8])->Result<usize>{if image.get(..8)!=Some(b"NSAR0001"){return Err(errno::EINVAL)}let count=u32le(image,8)? as usize;if count>MAX_NODES{return Err(errno::ENOMEM)}let mut m=ROOT.lock();m.len=0;let mut p=12usize;for i in 0..count{let nl=u16le(image,p)? as usize;let raw=u16le(image,p+2)? as u32;let size=u32le(image,p+4)? as usize;p+=8;let mode=if raw&0o170000!=0{raw}else{S_IFREG|raw&0o777};if nl==0||nl>NAME_MAX{return Err(errno::EINVAL)}let n=image.get(p..p+nl).ok_or(errno::EINVAL)?;if n[0]!=b'/'||n.contains(&0){return Err(errno::EINVAL)}let mut name=[0u8;NAME_MAX+1];name[..nl].copy_from_slice(n);p=align8(p+nl)?;let data=image.get(p..p+size).ok_or(errno::EINVAL)?;m.nodes[i]=Inode{ino:(i+2)as u64,mode,size:size as u64,capacity:size as u64,data:data.as_ptr()as*mut u8,name,name_len:nl};p=align8(p+size)?;}m.len=count;m.mounted=true;Ok(count)}
pub fn lookup(path:&str)->Result<Inode>{if !path.starts_with('/')||path.as_bytes().contains(&0){return Err(errno::ENOENT)}let m=ROOT.lock();if !m.mounted{return Err(errno::ENOENT)}m.nodes[..m.len].iter().copied().find(|n|&n.name[..n.name_len]==path.as_bytes()).ok_or(errno::ENOENT)}
pub fn read(inode:Inode,off:u64,dst:&mut[u8])->Result<usize>{if off>=inode.size{return Ok(0)}let n=core::cmp::min(dst.len(),(inode.size-off)as usize);let src=unsafe{core::slice::from_raw_parts(inode.data.add(off as usize),n)};dst[..n].copy_from_slice(src);Ok(n)}
pub fn write(inode:Inode,off:u64,src:&[u8])->Result<usize>{
 let mut m=ROOT.lock();let idx=m.nodes[..m.len].iter().position(|&n|n.ino==inode.ino).ok_or(errno::ENOENT)?;
 let end=off.checked_add(src.len() as u64).ok_or(errno::EINVAL)?;
 if end>m.nodes[idx].capacity{return Err(errno::ENOSPC)}
 unsafe{core::slice::from_raw_parts_mut(m.nodes[idx].data.add(off as usize),src.len()).copy_from_slice(src)}
 if end>m.nodes[idx].size{m.nodes[idx].size=end;}
 Ok(src.len())
}
pub fn create(path:&str,mode:u32)->Result<Inode>{
 let mut m=ROOT.lock();
 if !path.starts_with('/')||path.as_bytes().contains(&0){return Err(errno::EINVAL)}
 if m.len>=MAX_NODES{return Err(errno::EMFILE)}
 if m.nodes[..m.len].iter().any(|n|&n.name[..n.name_len]==path.as_bytes()){return Err(errno::EEXIST)}
 let name_len=path.len();
 if name_len>NAME_MAX{return Err(errno::ENAMETOOLONG)}
  let ino=(m.len+2)as u64;
  let mut name=[0u8;NAME_MAX+1];name[..name_len].copy_from_slice(path.as_bytes());
  let frame=match crate::memory::alloc_frame(){Some(f)=>f,None=>return Err(errno::ENOMEM)};
  unsafe{Identity.table(frame).fill(0)}
  let node=Inode{ino,mode:S_IFREG|mode&0o777,size:0,capacity:4096,data:frame as*mut u8,name,name_len};
  let idx=m.len; m.nodes[idx]=node;
  m.len+=1;
  Ok(node)
}
pub fn unlink(path:&str)->Result<()>{
 let mut m=ROOT.lock();
 let idx=m.nodes[..m.len].iter().position(|n|&n.name[..n.name_len]==path.as_bytes()).ok_or(errno::ENOENT)?;
 if m.nodes[idx].mode&S_IFDIR!=0{return Err(errno::EISDIR)}
 for i in idx..m.len-1{m.nodes[i]=m.nodes[i+1];}
 m.len-=1;
 Ok(())
}
pub fn mkdir(path:&str,mode:u32)->Result<Inode>{
 let mut m=ROOT.lock();
 if !path.starts_with('/')||path.as_bytes().contains(&0){return Err(errno::EINVAL)}
 if m.len>=MAX_NODES{return Err(errno::EMFILE)}
 if m.nodes[..m.len].iter().any(|n|&n.name[..n.name_len]==path.as_bytes()){return Err(errno::EEXIST)}
 let name_len=path.len();
 if name_len>NAME_MAX{return Err(errno::ENAMETOOLONG)}
 let ino=(m.len+2)as u64;
 let mut name=[0u8;NAME_MAX+1];name[..name_len].copy_from_slice(path.as_bytes());
 let node=Inode{ino,mode:S_IFDIR|mode&0o777,size:0,capacity:0,data:core::ptr::null_mut(),name,name_len};
 let idx=m.len; m.nodes[idx]=node;
 m.len+=1;
 Ok(node)
}
/* Lists the entries directly under path, calling cb(ino, mode, name) for each.
   "." and ".." are included when the directory exists. */
pub fn readdir(path:&str,mut cb:impl FnMut(u64,u32,&str))->Result<usize>{
 let m=ROOT.lock();
 if !path.starts_with('/')||path.as_bytes().contains(&0){return Err(errno::ENOENT)}
 if !m.mounted{return Err(errno::ENOENT)}
 let dir=path.trim_end_matches('/');
 let mut count=0usize;
 let prefix_len=if dir.is_empty(){0}else{dir.len()+1};
 for i in 0..m.len{
  let n=&m.nodes[i];
  let np=core::str::from_utf8(&n.name[..n.name_len]).map_err(|_|errno::EINVAL)?;
  let rest=&np[prefix_len.min(np.len())..];
  if dir.is_empty(){
   if np.matches('/').count()==1{cb(n.ino,n.mode,np.trim_start_matches('/'));count+=1}
  }else if !rest.is_empty()&&np.starts_with(dir)&&np.as_bytes().get(dir.len())==Some(&b'/'){
   if !rest.contains('/'){cb(n.ino,n.mode,rest);count+=1}
  }
 }
 Ok(count)
}
pub fn image(path:&str)->Result<&'static[u8]>{let n=lookup(path)?;Ok(unsafe{core::slice::from_raw_parts(n.data,n.size as usize)})}
