//! Allocation-free initramfs VFS with read/write support.
use crate::{arch::x86_64::{paging::PhysAccess,phys::Identity},errno,sync::SpinLock,vm::VmFlags};
pub type Result<T>=core::result::Result<T,i64>;
pub const S_IFREG:u32=0o100000;pub const S_IFDIR:u32=0o040000;pub const S_IFBLK:u32=0o060000;pub const MAX_NODES:usize=64;const NAME_MAX:usize=127;
#[derive(Clone,Copy)]pub struct Inode{pub ino:u64,pub mode:u32,pub size:u64,pub capacity:u64,pub data:*mut u8,pub rdev:u64,pub pages:[u64;8],pub npages:u8,pub name:[u8;NAME_MAX+1],pub name_len:usize}
unsafe impl Send for Inode{} unsafe impl Sync for Inode{}
const EMPTY:Inode=Inode{ino:0,mode:0,size:0,capacity:0,data:core::ptr::null_mut(),rdev:0,pages:[0;8],npages:0,name:[0;NAME_MAX+1],name_len:0};
struct Mount{nodes:[Inode;MAX_NODES],len:usize,mounted:bool}
static ROOT:SpinLock<Mount>=SpinLock::new(Mount{nodes:[EMPTY;MAX_NODES],len:0,mounted:false});
const MAX_MOUNTS:usize=16;
const TMPFS_INO_TAG:u64=0x1000_0000_0000_0000;
#[derive(Clone,Copy,PartialEq,Eq)]enum MountKind{Tmpfs,Ext2}
#[derive(Clone,Copy)]struct MountEntry{used:bool,ns:u64,kind:MountKind,fs_id:usize,target:[u8;128],tlen:usize}
const EMPTY_MOUNT:MountEntry=MountEntry{used:false,ns:0,kind:MountKind::Tmpfs,fs_id:0,target:[0;128],tlen:0};
#[derive(Clone,Copy)]struct TmpfsStore{used:bool,len:usize,nodes:[Inode;MAX_NODES],linked:[bool;MAX_NODES]}
const EMPTY_TMPFS:TmpfsStore=TmpfsStore{used:false,len:0,nodes:[EMPTY;MAX_NODES],linked:[false;MAX_NODES]};
struct MountRegistry{entries:[MountEntry;MAX_MOUNTS],tmpfs:[TmpfsStore;MAX_MOUNTS]}
static MOUNT_REGISTRY:SpinLock<MountRegistry>=SpinLock::new(MountRegistry{entries:[EMPTY_MOUNT;MAX_MOUNTS],tmpfs:[EMPTY_TMPFS;MAX_MOUNTS]});
#[derive(Clone,Copy)]struct PathRoute{kind:Option<MountKind>,fs_id:usize,target:[u8;128],tlen:usize,relative:[u8;128],rlen:usize}

fn current_mount_ns()->u64{crate::task::with_current_process(|p|p.ns_mnt).unwrap_or(1)}
fn path_has_prefix(path:&str,prefix:&str)->bool{path==prefix||(path.starts_with(prefix)&&(prefix=="/"||path.as_bytes().get(prefix.len())==Some(&b'/')))}
fn route_path(path:&str,ns:u64)->PathRoute{
 let mut route=PathRoute{kind:None,fs_id:0,target:[0;128],tlen:0,relative:[0;128],rlen:0};
 let mut best:Option<MountEntry>=None;
 {
  let registry=MOUNT_REGISTRY.lock();
  for entry in registry.entries.iter().copied(){
   if !entry.used||entry.ns!=ns{continue}
   let target=core::str::from_utf8(&entry.target[..entry.tlen]).unwrap_or("");
   if path_has_prefix(path,target)&&best.map(|old|old.tlen<entry.tlen).unwrap_or(true){best=Some(entry);}
  }
 }
 if let Some(entry)=best{
  route.kind=Some(entry.kind);route.fs_id=entry.fs_id;route.tlen=entry.tlen;route.target=entry.target;
  let suffix=&path[entry.tlen..];let relative=if suffix.is_empty(){"/"}else{suffix};
  route.rlen=relative.len().min(127);route.relative[..route.rlen].copy_from_slice(&relative.as_bytes()[..route.rlen]);
  return route
 }
 /* Keep the legacy virtio-ext2 mount visible unless an explicit mount shadows it. */
 if crate::ext2::ready()&&path_has_prefix(path,"/mnt"){
  route.kind=Some(MountKind::Ext2);route.target[..4].copy_from_slice(b"/mnt");route.tlen=4;
  let suffix=&path[4..];let relative=if suffix.is_empty(){"/"}else{suffix};
  route.rlen=relative.len().min(127);route.relative[..route.rlen].copy_from_slice(&relative.as_bytes()[..route.rlen]);
  return route
 }
 route.rlen=path.len().min(127);route.relative[..route.rlen].copy_from_slice(&path.as_bytes()[..route.rlen]);
 route
}

pub fn mount_filesystem(source:&str,target:&str,fstype:&str,flags:u64,ns:u64)->Result<()>{
 let _=source;let _=flags;
 if !target.starts_with('/')||target=="/"||target.as_bytes().contains(&0){return Err(errno::EINVAL)}
 if target.len()>127{return Err(errno::ENAMETOOLONG)}
 let kind=match fstype{"tmpfs"=>MountKind::Tmpfs,"ext2"=>MountKind::Ext2,_=>return Err(errno::ENODEV)};
 let point=lookup_in_namespace(target,ns)?;
 if point.mode&0o170000!=S_IFDIR{return Err(errno::ENOTDIR)}
 if kind==MountKind::Ext2&&!crate::ext2::ready(){return Err(errno::ENODEV)}
 let mut registry=MOUNT_REGISTRY.lock();
 if registry.entries.iter().any(|e|e.used&&e.ns==ns&&core::str::from_utf8(&e.target[..e.tlen]).ok()==Some(target)){return Err(errno::EBUSY)}
 let entry_slot=registry.entries.iter().position(|e|!e.used).ok_or(errno::ENOMEM)?;
 let fs_id=if kind==MountKind::Tmpfs{
  let id=registry.tmpfs.iter().position(|s|!s.used).ok_or(errno::ENOSPC)?;
  registry.tmpfs[id]=TmpfsStore{used:true,..EMPTY_TMPFS};id
 }else{0};
 let mut name=[0u8;128];name[..target.len()].copy_from_slice(target.as_bytes());
 registry.entries[entry_slot]=MountEntry{used:true,ns,kind,fs_id,target:name,tlen:target.len()};
 Ok(())
}
pub fn unmount_filesystem(target:&str,flags:u64,ns:u64)->Result<()>{
  let _=flags;let mut registry=MOUNT_REGISTRY.lock();
  let index=registry.entries.iter().position(|e|e.used&&e.ns==ns&&core::str::from_utf8(&e.target[..e.tlen]).ok()==Some(target)).ok_or(errno::EINVAL)?;
  if registry.entries.iter().any(|e|{if !e.used||e.ns!=ns||e.tlen<=target.len(){return false}let path=core::str::from_utf8(&e.target[..e.tlen]).unwrap_or("");path_has_prefix(path,target)}){return Err(errno::EBUSY)}
  registry.entries[index].used=false;Ok(())
}
pub fn mount_self_test()->bool{
 const NS_A:u64=0xffff_ffff_ffff_ff01;const NS_B:u64=0xffff_ffff_ffff_ff02;
 const TARGET:&str="/mnt";const PATH:&str="/mnt/.slop-tmpfs-check";
 if mount_filesystem("tmpfs",TARGET,"tmpfs",0,NS_A).is_err(){return false}
 let route=route_path(PATH,NS_A);let relative=match core::str::from_utf8(&route.relative[..route.rlen]){Ok(v)=>v,Err(_)=>return false};
 let result=(||->Result<bool>{
  let inode=tmpfs_add_node(route.fs_id,relative,PATH,S_IFREG|0o600,0,None)?;
  write_off(inode,0,b"mount")?;
  let mut bytes=[0u8;5];if read_off(inode,0,&mut bytes)?!=5||&bytes!=b"mount"{return Ok(false)}
  if lookup_in_namespace(PATH,NS_B).is_ok(){return Ok(false)}
  clone_mount_namespace(NS_A,NS_B)?;
  let inherited=lookup_in_namespace(PATH,NS_B)?;
  if inherited.size!=5{return Ok(false)}
  tmpfs_unlink(route.fs_id,relative,PATH)?;
  if lookup_in_namespace(PATH,NS_B).is_ok(){return Ok(false)}
  unmount_filesystem(TARGET,0,NS_A)?;
  if lookup_in_namespace(TARGET,NS_B).is_err(){return Ok(false)}
  unmount_filesystem(TARGET,0,NS_B)?;
  Ok(true)
 })();
 if result.is_err()||result==Ok(false){let _=tmpfs_unlink(route.fs_id,relative,PATH);let _=unmount_filesystem(TARGET,0,NS_A);let _=unmount_filesystem(TARGET,0,NS_B);return false}
 result.unwrap_or(false)
}
pub fn clone_mount_namespace(source_ns:u64,destination_ns:u64)->Result<()>{
 let mut registry=MOUNT_REGISTRY.lock();
 let mut source=[EMPTY_MOUNT;MAX_MOUNTS];let mut count=0usize;
 for entry in registry.entries.iter().copied(){if entry.used&&entry.ns==source_ns{source[count]=entry;count+=1;}}
 let free=registry.entries.iter().filter(|e|!e.used).count();
 if free<count{return Err(errno::ENOMEM)}
 let mut next=0usize;
 for i in 0..MAX_MOUNTS{if registry.entries[i].used{continue}while next<count{let mut entry=source[next];next+=1;entry.ns=destination_ns;registry.entries[i]=entry;break}if next>=count{break}}
 Ok(())
}
fn tmpfs_ino(fs_id:usize,sequence:usize)->u64{TMPFS_INO_TAG|(((fs_id+1)as u64)<<32)|sequence as u64}
fn tmpfs_fs_id(ino:u64)->Option<usize>{if ino&0xf000_0000_0000_0000!=TMPFS_INO_TAG{return None}let id=((ino>>32)&0xff)as usize;if id==0{None}else{Some(id-1)}}
fn visible_inode(mut inode:Inode,path:&str)->Result<Inode>{if path.len()>NAME_MAX{return Err(errno::ENAMETOOLONG)}inode.name=[0;NAME_MAX+1];inode.name[..path.len()].copy_from_slice(path.as_bytes());inode.name_len=path.len();Ok(inode)}
fn tmpfs_root(fs_id:usize,path:&str)->Result<Inode>{
 let mut name=[0u8;NAME_MAX+1];if path.len()>NAME_MAX{return Err(errno::ENAMETOOLONG)}name[..path.len()].copy_from_slice(path.as_bytes());
 Ok(Inode{ino:tmpfs_ino(fs_id,0),mode:S_IFDIR|0o755,size:0,capacity:0,data:core::ptr::null_mut(),rdev:0,pages:[0;8],npages:0,name,name_len:path.len()})
}
fn tmpfs_lookup(fs_id:usize,relative:&str,visible:&str)->Result<Inode>{
 if relative=="/"{return tmpfs_root(fs_id,visible)}
 let registry=MOUNT_REGISTRY.lock();let store=registry.tmpfs.get(fs_id).filter(|s|s.used).ok_or(errno::ENOENT)?;
 let node=store.nodes[..store.len].iter().enumerate().find(|(i,n)|store.linked[*i]&&n.ino!=0&&&n.name[..n.name_len]==relative.as_bytes()).map(|(_,n)|*n).ok_or(errno::ENOENT)?;
 visible_inode(node,visible)
}
fn tmpfs_parent_exists(store:&TmpfsStore,path:&str)->bool{
 let slash=path.rfind('/').unwrap_or(0);let parent=if slash==0{"/"}else{&path[..slash]};
 if parent=="/"{return true}
 store.nodes[..store.len].iter().enumerate().any(|(i,n)|store.linked[i]&&n.ino!=0&&&n.name[..n.name_len]==parent.as_bytes()&&n.mode&0o170000==S_IFDIR)
}
fn tmpfs_add_node(fs_id:usize,relative:&str,visible:&str,mode:u32,rdev:u64,data:Option<&[u8]>)->Result<Inode>{
 if !relative.starts_with('/')||relative=="/"||relative.as_bytes().contains(&0){return Err(errno::EINVAL)}
 if relative.len()>NAME_MAX{return Err(errno::ENAMETOOLONG)}
 if let Some(bytes)=data{if bytes.len()>4095{return Err(errno::ENAMETOOLONG)}}
 let mut registry=MOUNT_REGISTRY.lock();let store=registry.tmpfs.get_mut(fs_id).filter(|s|s.used).ok_or(errno::ENOENT)?;
 if store.nodes[..store.len].iter().enumerate().any(|(i,n)|store.linked[i]&&n.ino!=0&&&n.name[..n.name_len]==relative.as_bytes()){return Err(errno::EEXIST)}
 if !tmpfs_parent_exists(store,relative){return Err(errno::ENOENT)}
 if store.len==MAX_NODES{return Err(errno::ENOSPC)}
 let mut pages=[0u64;8];let mut npages=0u8;let mut capacity=0u64;let mut pointer=core::ptr::null_mut();let mut size=0u64;
 if mode&0o170000==S_IFREG||mode&0o170000==S_IFLNK{
  let frame=crate::memory::alloc_frame().ok_or(errno::ENOMEM)?;
  unsafe{Identity.table(frame).fill(0)}
  if let Some(bytes)=data{unsafe{core::slice::from_raw_parts_mut(frame as*mut u8,bytes.len()).copy_from_slice(bytes)}size=bytes.len()as u64;}
  pages[0]=frame;npages=1;capacity=4096;pointer=frame as*mut u8;
 }
 let idx=store.len;let mut name=[0u8;NAME_MAX+1];name[..relative.len()].copy_from_slice(relative.as_bytes());
 let node=Inode{ino:tmpfs_ino(fs_id,idx+1),mode,size,capacity,data:pointer,rdev,pages,npages,name,name_len:relative.len()};
 store.nodes[idx]=node;store.linked[idx]=true;store.len+=1;
 drop(registry);crate::inotify::notify(visible,crate::inotify::IN_CREATE|if mode&0o170000==S_IFDIR{crate::inotify::IN_ISDIR}else{0});
 visible_inode(node,visible)
}
fn tmpfs_readdir(fs_id:usize,relative:&str,mut cb:impl FnMut(u64,u32,&str))->Result<usize>{
 let mut entries:[(u64,u32,[u8;NAME_MAX+1],usize);MAX_NODES]=[(0,0,[0;NAME_MAX+1],0);MAX_NODES];let mut count=0usize;
 {
  let registry=MOUNT_REGISTRY.lock();let store=registry.tmpfs.get(fs_id).filter(|s|s.used).ok_or(errno::ENOENT)?;
  let dir=if relative.is_empty(){"/"}else{relative.trim_end_matches('/')};
  for i in 0..store.len{let node=store.nodes[i];if !store.linked[i]||node.ino==0{continue}
   let name=core::str::from_utf8(&node.name[..node.name_len]).map_err(|_|errno::EINVAL)?;
   let child=if dir=="/"{name.strip_prefix('/').unwrap_or("")}else{name.strip_prefix(dir).and_then(|r|r.strip_prefix('/')).unwrap_or("")};
   if child.is_empty()||child.contains('/')||count==MAX_NODES{continue}
   let mut b=[0u8;NAME_MAX+1];b[..child.len()].copy_from_slice(child.as_bytes());entries[count]=(node.ino,node.mode,b,child.len());count+=1;
  }
 }
 for(ino,mode,name,len)in entries[..count].iter(){let n=core::str::from_utf8(&name[..*len]).map_err(|_|errno::EINVAL)?;cb(*ino,*mode,n)}
 Ok(count)
}
fn tmpfs_unlink(fs_id:usize,relative:&str,visible:&str)->Result<()>{
 let mut registry=MOUNT_REGISTRY.lock();let store=registry.tmpfs.get_mut(fs_id).filter(|s|s.used).ok_or(errno::ENOENT)?;
 let i=store.nodes[..store.len].iter().enumerate().position(|(i,n)|store.linked[i]&&n.ino!=0&&&n.name[..n.name_len]==relative.as_bytes()).ok_or(errno::ENOENT)?;
 if store.nodes[i].mode&0o170000==S_IFDIR{return Err(errno::EISDIR)}store.linked[i]=false;drop(registry);crate::inotify::notify(visible,crate::inotify::IN_DELETE);Ok(())
}
fn tmpfs_rmdir(fs_id:usize,relative:&str,visible:&str)->Result<()>{
 if relative=="/"{return Err(errno::EBUSY)}
 let mut registry=MOUNT_REGISTRY.lock();let store=registry.tmpfs.get_mut(fs_id).filter(|s|s.used).ok_or(errno::ENOENT)?;
 let i=store.nodes[..store.len].iter().enumerate().position(|(i,n)|store.linked[i]&&n.ino!=0&&&n.name[..n.name_len]==relative.as_bytes()).ok_or(errno::ENOENT)?;
 if store.nodes[i].mode&0o170000!=S_IFDIR{return Err(errno::ENOTDIR)}
 let child=store.nodes[..store.len].iter().enumerate().any(|(j,n)|store.linked[j]&&n.ino!=0&&n.name_len>relative.len()&&n.name[..relative.len()]==*relative.as_bytes()&&n.name[relative.len()]==b'/');
 if child{return Err(errno::ENOTEMPTY)}store.linked[i]=false;drop(registry);crate::inotify::notify(visible,crate::inotify::IN_DELETE|crate::inotify::IN_ISDIR);Ok(())
}
fn tmpfs_write_inode(node:&mut Inode,off:u64,src:&[u8])->Result<usize>{
 let end=off.checked_add(src.len()as u64).ok_or(errno::EINVAL)?;let needed=((end as usize+4095)/4096).max(1);
 while(node.npages as usize)<needed&&(node.npages as usize)<8{let frame=crate::memory::alloc_frame().ok_or(errno::ENOMEM)?;unsafe{Identity.table(frame).fill(0)}node.pages[node.npages as usize]=frame;node.npages+=1;node.capacity=node.npages as u64*4096;}
 if end>node.capacity{return Err(errno::ENOSPC)}
 let mut done=0usize;while done<src.len(){let poff=off as usize+done;let pi=poff/4096;let po=poff%4096;if pi>=node.npages as usize{return Err(errno::ENOSPC)}let frame=node.pages[pi];let n=core::cmp::min(src.len()-done,4096-po);unsafe{core::slice::from_raw_parts_mut((frame as*mut u8).add(po),n).copy_from_slice(&src[done..done+n])};done+=n;}
 if end>node.size{node.size=end}Ok(src.len())
}
fn u16le(b:&[u8],o:usize)->Result<u16>{let x=b.get(o..o+2).ok_or(errno::EINVAL)?;Ok(u16::from_le_bytes([x[0],x[1]]))}
fn u32le(b:&[u8],o:usize)->Result<u32>{let x=b.get(o..o+4).ok_or(errno::EINVAL)?;Ok(u32::from_le_bytes([x[0],x[1],x[2],x[3]]))}
fn align8(v:usize)->Result<usize>{v.checked_add(7).map(|x|x&!7).ok_or(errno::EINVAL)}
pub fn mount_initramfs(image:&'static [u8])->Result<usize>{if image.get(..8)!=Some(b"NSAR0001"){return Err(errno::EINVAL)}let count=u32le(image,8)? as usize;if count>MAX_NODES{return Err(errno::ENOMEM)}let mut m=ROOT.lock();m.len=0;let mut p=12usize;for i in 0..count{let nl=u16le(image,p)? as usize;let raw=u16le(image,p+2)? as u32;let size=u32le(image,p+4)? as usize;p+=8;let mode=if raw&0o170000!=0{raw}else{S_IFREG|raw&0o777};if nl==0||nl>NAME_MAX{return Err(errno::EINVAL)}let n=image.get(p..p+nl).ok_or(errno::EINVAL)?;if n[0]!=b'/'||n.contains(&0){return Err(errno::EINVAL)}let mut name=[0u8;NAME_MAX+1];name[..nl].copy_from_slice(n);p=align8(p+nl)?;let data=image.get(p..p+size).ok_or(errno::EINVAL)?;m.nodes[i]=Inode{ino:(i+2)as u64,mode,size:size as u64,capacity:size as u64,data:data.as_ptr()as*mut u8,rdev:0,pages:[data.as_ptr()as u64,0,0,0,0,0,0,0],npages:1,name,name_len:nl};p=align8(p+size)?;}m.len=count;m.mounted=true;Ok(count)}
pub fn read(inode:Inode,off:u64,dst:&mut[u8])->Result<usize>{
  read_off(inode,off,dst)
}

fn read_inode_data(inode:&Inode,off:u64,dst:&mut[u8])->Result<usize>{
 if off>=inode.size{return Ok(0)}
 if inode.npages==0{let n=core::cmp::min(dst.len(),(inode.size-off)as usize);if n>0{if inode.data.is_null(){return Err(errno::EIO)}let src=unsafe{core::slice::from_raw_parts(inode.data.add(off as usize),n)};dst[..n].copy_from_slice(src)}return Ok(n)}
 let mut done=0usize;
 while done<dst.len()&&off as usize+done<inode.size as usize{let poff=off as usize+done;let pi=poff/4096;let po=poff%4096;if pi>=inode.npages as usize{return Ok(done)}let n=core::cmp::min(dst.len()-done,core::cmp::min(4096-po,inode.size as usize-poff));if n==0{break}let src=unsafe{core::slice::from_raw_parts((inode.pages[pi]as*const u8).add(po),n)};dst[done..done+n].copy_from_slice(src);done+=n;}
 Ok(done)
}
/* Positional reads resolve the node by ino so a grown size is visible. */
pub fn write(inode:Inode,off:u64,src:&[u8])->Result<usize>{
  write_off(inode,off,src)
}
pub fn create(path:&str,mode:u32)->Result<Inode>{
  let route=route_path(path,current_mount_ns());
  match route.kind{Some(MountKind::Tmpfs)=>{let rel=core::str::from_utf8(&route.relative[..route.rlen]).map_err(|_|errno::EINVAL)?;return tmpfs_add_node(route.fs_id,rel,path,S_IFREG|mode&0o777,0,None)},Some(MountKind::Ext2)=>return Err(errno::EROFS),None=>{}}
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
  let node=Inode{ino,mode:S_IFREG|mode&0o777,size:0,capacity:4096,data:frame as*mut u8,rdev:0,pages:[frame,0,0,0,0,0,0,0],npages:1,name,name_len};
  let idx=m.len; m.nodes[idx]=node;
  m.len+=1;
  crate::inotify::notify(path,crate::inotify::IN_CREATE);
  Ok(node)
}
pub fn unlink(path:&str)->Result<()>{
  let route=route_path(path,current_mount_ns());
  match route.kind{Some(MountKind::Tmpfs)=>{let rel=core::str::from_utf8(&route.relative[..route.rlen]).map_err(|_|errno::EINVAL)?;return tmpfs_unlink(route.fs_id,rel,path)},Some(MountKind::Ext2)=>return Err(errno::EROFS),None=>{}}
  let mut m=ROOT.lock();
 let idx=m.nodes[..m.len].iter().position(|n|&n.name[..n.name_len]==path.as_bytes()).ok_or(errno::ENOENT)?;
 if m.nodes[idx].mode&S_IFDIR!=0{return Err(errno::EISDIR)}
 for i in idx..m.len-1{m.nodes[i]=m.nodes[i+1];}
 m.len-=1;
 crate::inotify::notify(path,crate::inotify::IN_DELETE);
 Ok(())
}
pub fn mkdir(path:&str,mode:u32)->Result<Inode>{
  let route=route_path(path,current_mount_ns());
  match route.kind{Some(MountKind::Tmpfs)=>{let rel=core::str::from_utf8(&route.relative[..route.rlen]).map_err(|_|errno::EINVAL)?;return tmpfs_add_node(route.fs_id,rel,path,S_IFDIR|mode&0o777,0,None)},Some(MountKind::Ext2)=>return Err(errno::EROFS),None=>{}}
  let mut m=ROOT.lock();
 if !path.starts_with('/')||path.as_bytes().contains(&0){return Err(errno::EINVAL)}
 if m.len>=MAX_NODES{return Err(errno::EMFILE)}
 if m.nodes[..m.len].iter().any(|n|&n.name[..n.name_len]==path.as_bytes()){return Err(errno::EEXIST)}
 let name_len=path.len();
 if name_len>NAME_MAX{return Err(errno::ENAMETOOLONG)}
 let ino=(m.len+2)as u64;
 let mut name=[0u8;NAME_MAX+1];name[..name_len].copy_from_slice(path.as_bytes());
 let node=Inode{ino,mode:S_IFDIR|mode&0o777,size:0,capacity:0,data:core::ptr::null_mut(),rdev:0,pages:[0;8],npages:0,name,name_len};
 let idx=m.len; m.nodes[idx]=node;
 m.len+=1;
 crate::inotify::notify(path,crate::inotify::IN_CREATE|crate::inotify::IN_ISDIR);
 Ok(node)
}
/* Lists the entries directly under path, calling cb(ino, mode, name) for each.
   "." and ".." are included when the directory exists. */
pub fn readdir(path:&str,mut cb:impl FnMut(u64,u32,&str))->Result<usize>{
  let route=route_path(path,current_mount_ns());
  match route.kind{
   Some(MountKind::Tmpfs)=>{let rel=core::str::from_utf8(&route.relative[..route.rlen]).map_err(|_|errno::EINVAL)?;return tmpfs_readdir(route.fs_id,rel,cb)},
   Some(MountKind::Ext2)=>{let rel=core::str::from_utf8(&route.relative[..route.rlen]).map_err(|_|errno::EINVAL)?;return crate::ext2::readdir(rel,cb)},
   None=>{}
  }
  if path=="/proc"{
  let mut count=1usize;
  cb(0x9001,S_IFDIR,"self");
  crate::task::live_pids(|pid|{let mut b=[0u8;12];let name=num_str(pid,&mut b);cb(0x9400+pid as u64,S_IFDIR,name);count+=1;});
  for name in ["uptime","meminfo","version","cpuinfo","loadavg","mounts"]{cb(0x90000+name.len()as u64,S_IFREG,name);count+=1;}
  return Ok(count)
 }
 if path=="/proc/self"{
  cb(0x9002,S_IFDIR,"fd"); cb(0x9010,S_IFDIR,"ns"); cb(0x9011,S_IFLNK,"exe"); cb(0x9012,S_IFLNK,"root"); cb(0x9013,S_IFLNK,"cwd"); cb(0x9003,S_IFREG,"cmdline"); cb(0x9004,S_IFREG,"environ"); cb(0x9005,S_IFREG,"status"); cb(0x9006,S_IFREG,"stat"); cb(0x900e,S_IFREG,"statm"); cb(0x9007,S_IFREG,"maps"); return Ok(11)
 }
 if path=="/proc/self/fd"{
  let mut count=0usize;
  crate::task::with_current_process(|p|{p.fds.open_fds(|fd|{let mut b=[0u8;12];let name=num_str(fd as u32,&mut b);cb(0x9100+fd as u64,S_IFLNK,name);count+=1;});});
  return Ok(count)
 }
 if let Some((subject,rest))=parse_proc(path){
  if rest.is_empty()&&subject!=ProcSubject::SelfProc{
   if !proc_exists(subject){return Err(errno::ENOENT)}
   cb(0x9003,S_IFREG,"cmdline"); cb(0x9004,S_IFREG,"environ"); cb(0x9005,S_IFREG,"status"); cb(0x9006,S_IFREG,"stat"); cb(0x900e,S_IFREG,"statm"); cb(0x9007,S_IFREG,"maps"); cb(0x9011,S_IFLNK,"exe"); cb(0x9012,S_IFLNK,"root"); cb(0x9013,S_IFLNK,"cwd"); return Ok(9)
  }
 }
 if path=="/proc/self/ns"{
  cb(0x9200,S_IFLNK,"uts"); cb(0x9201,S_IFLNK,"ipc"); cb(0x9202,S_IFLNK,"mnt"); cb(0x9203,S_IFLNK,"pid"); cb(0x9204,S_IFLNK,"net"); cb(0x9205,S_IFLNK,"user"); cb(0x9206,S_IFLNK,"cgroup"); cb(0x9207,S_IFLNK,"time"); return Ok(8)
 }
 if path=="/sys"{cb(0xa000,S_IFDIR,"class");cb(0xa001,S_IFDIR,"devices");cb(0xa002,S_IFDIR,"kernel");cb(0xa003,S_IFDIR,"fs");cb(0xa004,S_IFDIR,"block");return Ok(5)}
 if path=="/sys/class"{cb(0xa010,S_IFDIR,"net");cb(0xa011,S_IFDIR,"tty");cb(0xa012,S_IFDIR,"block");return Ok(3)}
 if path=="/sys/class/net"{cb(0xa030,S_IFDIR,"eth0");return Ok(1)}
 if path=="/sys/block"{cb(0xa040,S_IFDIR,"vda");return Ok(1)}
 if path=="/sys/class/net/eth0"{cb(0xa031,S_IFREG,"address");cb(0xa032,S_IFREG,"operstate");return Ok(2)}
 if path=="/sys/block/vda"{cb(0xa041,S_IFREG,"size");cb(0xa042,S_IFREG,"dev");return Ok(2)}
 if path=="/sys/class/net"{cb(0xa030,S_IFDIR,"eth0");return Ok(1)}
 if path=="/sys/block"{cb(0xa040,S_IFDIR,"vda");return Ok(1)}
 if path=="/sys/class"{cb(0xa010,S_IFDIR,"net");return Ok(1)}
 if path=="/sys/kernel"{cb(0xa020,S_IFREG,"hostname");cb(0xa021,S_IFREG,"osrelease");return Ok(2)}
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

pub fn is_dir(path:&str)->bool{
  lookup(path).map(|n|n.mode&0o170000==S_IFDIR).unwrap_or(false)
}
pub fn rmdir(path:&str)->Result<()>{
  if path=="/"{return Err(errno::EINVAL)}
  let route=route_path(path,current_mount_ns());
  match route.kind{Some(MountKind::Tmpfs)=>{let rel=core::str::from_utf8(&route.relative[..route.rlen]).map_err(|_|errno::EINVAL)?;return tmpfs_rmdir(route.fs_id,rel,path)},Some(MountKind::Ext2)=>return Err(errno::EROFS),None=>{}}
  let mut m=ROOT.lock();
 let idx=m.nodes[..m.len].iter().position(|n|&n.name[..n.name_len]==path.as_bytes()).ok_or(errno::ENOENT)?;
 if m.nodes[idx].mode&S_IFDIR==0{return Err(errno::ENOTDIR)}
 let prefix=path.trim_end_matches('/');
 let has_child=m.nodes[..m.len].iter().any(|n|{
  let np=&n.name[..n.name_len];
  np.starts_with(prefix.as_bytes()) && np.len()>prefix.len() && np[prefix.len()]==b'/'
 });
 if has_child{return Err(errno::ENOTEMPTY)}
 for i in idx..m.len-1{m.nodes[i]=m.nodes[i+1];}
 m.len-=1;
 Ok(())
}
pub fn rename(old:&str,new:&str)->Result<()>{
  if old=="/"||new=="/"{return Err(errno::EINVAL)}
  let ns=current_mount_ns();let old_route=route_path(old,ns);let new_route=route_path(new,ns);
  if old_route.kind!=new_route.kind||old_route.fs_id!=new_route.fs_id{return Err(errno::EXDEV)}
  if let Some(kind)=old_route.kind{match kind{
   MountKind::Ext2=>return Err(errno::EROFS),
   MountKind::Tmpfs=>{
    let old_rel=core::str::from_utf8(&old_route.relative[..old_route.rlen]).map_err(|_|errno::EINVAL)?;
    let new_rel=core::str::from_utf8(&new_route.relative[..new_route.rlen]).map_err(|_|errno::EINVAL)?;
    let mut registry=MOUNT_REGISTRY.lock();let store=registry.tmpfs.get_mut(old_route.fs_id).filter(|s|s.used).ok_or(errno::ENOENT)?;
    let idx=store.nodes[..store.len].iter().enumerate().position(|(i,n)|store.linked[i]&&n.ino!=0&&&n.name[..n.name_len]==old_rel.as_bytes()).ok_or(errno::ENOENT)?;
    if store.nodes[..store.len].iter().enumerate().any(|(i,n)|store.linked[i]&&n.ino!=0&&&n.name[..n.name_len]==new_rel.as_bytes()){return Err(errno::EEXIST)}
    if !tmpfs_parent_exists(store,new_rel){return Err(errno::ENOENT)}
    let old_bytes=old_rel.as_bytes();let new_bytes=new_rel.as_bytes();
    for i in 0..store.len{if !store.linked[i]{continue}let name=&store.nodes[i].name[..store.nodes[i].name_len];let matches=i==idx||name.starts_with(old_bytes)&&name.len()>old_bytes.len()&&name[old_bytes.len()]==b'/';if !matches{continue}
     let suffix=if i==idx{&[][..]}else{&name[old_bytes.len()..]};let total=new_bytes.len()+suffix.len();if total>NAME_MAX{return Err(errno::ENAMETOOLONG)}
     let mut renamed=[0u8;NAME_MAX+1];renamed[..new_bytes.len()].copy_from_slice(new_bytes);renamed[new_bytes.len()..total].copy_from_slice(suffix);store.nodes[i].name=renamed;store.nodes[i].name_len=total;
    }
    drop(registry);crate::inotify::notify(old,crate::inotify::IN_MOVED_FROM);crate::inotify::notify(new,crate::inotify::IN_MOVED_TO);return Ok(())
   }
  }}
  let mut m=ROOT.lock();
 let idx=m.nodes[..m.len].iter().position(|n|&n.name[..n.name_len]==old.as_bytes()).ok_or(errno::ENOENT)?;
 if m.nodes[..m.len].iter().any(|n|&n.name[..n.name_len]==new.as_bytes()){return Err(errno::EEXIST)}
 let nl=new.len();
 if nl>NAME_MAX{return Err(errno::ENAMETOOLONG)}
 let mut name=[0u8;NAME_MAX+1];name[..nl].copy_from_slice(new.as_bytes());
 m.nodes[idx].name=name;m.nodes[idx].name_len=nl;
 let oldb=old.as_bytes();let newb=new.as_bytes();
 for i in 0..m.len{
  if i==idx{continue}
  let nm=&m.nodes[i].name[..m.nodes[i].name_len];
  if nm.starts_with(oldb) && nm.len()>oldb.len() && nm[oldb.len()]==b'/'{
   let rest=&nm[oldb.len()..];
   let mut buf=[0u8;NAME_MAX+1];let total=newb.len()+rest.len();
   if total>NAME_MAX{continue}
   buf[..newb.len()].copy_from_slice(newb);buf[newb.len()..total].copy_from_slice(rest);
   m.nodes[i].name=buf;m.nodes[i].name_len=total;
  }
 }
 crate::inotify::notify(old,crate::inotify::IN_MOVED_FROM);
 crate::inotify::notify(new,crate::inotify::IN_MOVED_TO);
 Ok(())
}
pub fn chmod_path(path:&str,mode:u32)->Result<()>{
  let route=route_path(path,current_mount_ns());
  match route.kind{Some(MountKind::Tmpfs)=>{let rel=core::str::from_utf8(&route.relative[..route.rlen]).map_err(|_|errno::EINVAL)?;let mut registry=MOUNT_REGISTRY.lock();let store=registry.tmpfs.get_mut(route.fs_id).filter(|s|s.used).ok_or(errno::ENOENT)?;let node=store.nodes[..store.len].iter_mut().enumerate().find(|(i,n)|store.linked[*i]&&n.ino!=0&&&n.name[..n.name_len]==rel.as_bytes()).map(|(_,n)|n).ok_or(errno::ENOENT)?;let ty=node.mode&0o170000;node.mode=ty|(mode&0o7777);return Ok(())},Some(MountKind::Ext2)=>return Err(errno::EROFS),None=>{}}
  let mut m=ROOT.lock();
 let idx=m.nodes[..m.len].iter().position(|n|&n.name[..n.name_len]==path.as_bytes()).ok_or(errno::ENOENT)?;
 let ty=m.nodes[idx].mode&0o170000;
 m.nodes[idx].mode=ty|(mode&0o7777);
 Ok(())
}
pub fn chmod_ino(ino:u64,mode:u32)->Result<()>{
  if let Some(fs_id)=tmpfs_fs_id(ino){let mut registry=MOUNT_REGISTRY.lock();let store=registry.tmpfs.get_mut(fs_id).filter(|s|s.used).ok_or(errno::ENOENT)?;let node=store.nodes[..store.len].iter_mut().find(|n|n.ino==ino).ok_or(errno::ENOENT)?;let ty=node.mode&0o170000;node.mode=ty|(mode&0o7777);return Ok(())}
  let mut m=ROOT.lock();
 let idx=m.nodes[..m.len].iter().position(|n|n.ino==ino).ok_or(errno::ENOENT)?;
 let ty=m.nodes[idx].mode&0o170000;
 m.nodes[idx].mode=ty|(mode&0o7777);
 Ok(())
}
pub fn truncate_path(path:&str,size:u64)->Result<()>{
  let route=route_path(path,current_mount_ns());
  match route.kind{Some(MountKind::Tmpfs)=>{let rel=core::str::from_utf8(&route.relative[..route.rlen]).map_err(|_|errno::EINVAL)?;let mut registry=MOUNT_REGISTRY.lock();let store=registry.tmpfs.get_mut(route.fs_id).filter(|s|s.used).ok_or(errno::ENOENT)?;let node=store.nodes[..store.len].iter_mut().enumerate().find(|(i,n)|store.linked[*i]&&n.ino!=0&&&n.name[..n.name_len]==rel.as_bytes()).map(|(_,n)|n).ok_or(errno::ENOENT)?;if node.mode&0o170000==S_IFDIR{return Err(errno::EISDIR)}if size>node.capacity{return Err(errno::ENOSPC)}node.size=size;return Ok(())},Some(MountKind::Ext2)=>return Err(errno::EROFS),None=>{}}
  let mut m=ROOT.lock();
 let idx=m.nodes[..m.len].iter().position(|n|&n.name[..n.name_len]==path.as_bytes()).ok_or(errno::ENOENT)?;
 if m.nodes[idx].mode&S_IFDIR!=0{return Err(errno::EISDIR)}
 if m.nodes[idx].npages==0 { m.nodes[idx].pages[0]=m.nodes[idx].data as u64; m.nodes[idx].npages=1; m.nodes[idx].capacity=m.nodes[idx].capacity.max(4096); }
 if size>m.nodes[idx].capacity{return Err(errno::ENOSPC)}
 m.nodes[idx].size=size;
 Ok(())
}
pub fn truncate_ino(ino:u64,size:u64)->Result<()>{
  if let Some(fs_id)=tmpfs_fs_id(ino){let mut registry=MOUNT_REGISTRY.lock();let store=registry.tmpfs.get_mut(fs_id).filter(|s|s.used).ok_or(errno::ENOENT)?;let node=store.nodes[..store.len].iter_mut().find(|n|n.ino==ino).ok_or(errno::ENOENT)?;if node.mode&0o170000==S_IFDIR{return Err(errno::EISDIR)}if size>node.capacity{return Err(errno::ENOSPC)}node.size=size;return Ok(())}
  let mut m=ROOT.lock();
 let idx=m.nodes[..m.len].iter().position(|n|n.ino==ino).ok_or(errno::ENOENT)?;
 if m.nodes[idx].mode&S_IFDIR!=0{return Err(errno::EISDIR)}
 if m.nodes[idx].npages==0 { m.nodes[idx].pages[0]=m.nodes[idx].data as u64; m.nodes[idx].npages=1; m.nodes[idx].capacity=m.nodes[idx].capacity.max(4096); }
 if size>m.nodes[idx].capacity{return Err(errno::ENOSPC)}
 m.nodes[idx].size=size;
 Ok(())
}

pub const S_IFLNK:u32=0o120000;

static mut PROC_BUF:[u8;4096]=[0;4096];

#[derive(Clone,Copy,PartialEq)]enum ProcSubject{SelfProc,Pid(u32)}
fn parse_proc(path:&str)->Option<(ProcSubject,&str)>{
 if let Some(rest)=path.strip_prefix("/proc/self"){if rest.is_empty()||rest.starts_with('/'){return Some((ProcSubject::SelfProc,rest))}}
 if let Some(rest)=path.strip_prefix("/proc/"){
  let digits=rest.split('/').next().unwrap_or("");
  if !digits.is_empty()&&digits.bytes().all(|b|b.is_ascii_digit()){
   if let Ok(pid)=digits.parse::<u32>(){return Some((ProcSubject::Pid(pid),&rest[digits.len()..]))}
  }
 }
 None
}
fn proc_with<R>(subject:ProcSubject,f:impl FnOnce(&crate::process::Process)->R)->Option<R>{
 match subject{ProcSubject::SelfProc=>crate::task::with_current_process(|p|f(p)),ProcSubject::Pid(pid)=>crate::task::with_process_pid(pid,f)}
}
fn proc_exists(subject:ProcSubject)->bool{proc_with(subject,|_|()).is_some()}
fn subject_pid(subject:ProcSubject)->u32{match subject{ProcSubject::SelfProc=>crate::task::current_pid(),ProcSubject::Pid(pid)=>pid}}
fn proc_file_name(name:&str)->bool{matches!(name,"cmdline"|"environ"|"status"|"stat"|"statm"|"maps")}
fn proc_sys_name(name:&str)->bool{matches!(name,"cpuinfo"|"meminfo"|"uptime"|"version"|"loadavg"|"mounts")}
fn num_str<'a>(v:u32,buf:&'a mut[u8;12])->&'a str{let mut i=buf.len();let mut x=v;loop{i-=1;buf[i]=b'0'+(x%10)as u8;x/=10;if x==0{break}}core::str::from_utf8(&buf[i..]).unwrap_or("0")}
fn proc_dir_inode(path:&str,ino:u64)->Inode{let mut name=[0u8;NAME_MAX+1];let b=path.as_bytes();let n=core::cmp::min(b.len(),NAME_MAX);name[..n].copy_from_slice(&b[..n]);Inode{ino,mode:S_IFDIR|0o555,name,name_len:n,size:0,capacity:0,data:core::ptr::null_mut(),rdev:0,pages:[0;8],npages:0}}
fn put_bytes(out:&mut[u8],off:&mut usize,s:&[u8]){let n=core::cmp::min(s.len(),out.len().saturating_sub(*off));out[*off..*off+n].copy_from_slice(&s[..n]);*off+=n;}
fn comm_of(p:&crate::process::Process,out:&mut[u8;64])->usize{let path=&p.exe_path[..p.exe_len];let base=path.iter().rposition(|&b|b==b'/').map(|i|&path[i+1..]).unwrap_or(path);let n=core::cmp::min(base.len(),15);out[..n].copy_from_slice(&base[..n]);n}
fn fd_link_target(fd:usize)->Option<([u8;128],usize)>{
 let id=crate::task::with_current_process(|p|p.fds.get(fd))??;
 let mut out=[0u8;128];let mut off=0usize;
 match crate::fd::kind(id)?{
  crate::fd::OpenKind::Console=>put_bytes(&mut out,&mut off,b"/dev/console"),
  crate::fd::OpenKind::Inode(ino)=>put_bytes(&mut out,&mut off,&ino.name[..ino.name_len]),
  crate::fd::OpenKind::PipeRead(x)|crate::fd::OpenKind::PipeWrite(x)=>{put_bytes(&mut out,&mut off,b"pipe:[");append_num(&mut out,&mut off,x as u64);put_bytes(&mut out,&mut off,b"]")}
  crate::fd::OpenKind::Socket(x)=>{put_bytes(&mut out,&mut off,b"socket:[");append_num(&mut out,&mut off,x as u64);put_bytes(&mut out,&mut off,b"]")}
  crate::fd::OpenKind::EventFd(_,_)=>put_bytes(&mut out,&mut off,b"anon_inode:[eventfd]"),
  crate::fd::OpenKind::Epoll=>put_bytes(&mut out,&mut off,b"anon_inode:[eventpoll]"),
  crate::fd::OpenKind::TimerFd(_,_)=>put_bytes(&mut out,&mut off,b"anon_inode:[timerfd]"),
  crate::fd::OpenKind::CharDevice(_)=>put_bytes(&mut out,&mut off,b"/dev/console"),
  crate::fd::OpenKind::BlockDevice(_)=>put_bytes(&mut out,&mut off,b"/dev/vda"),
  crate::fd::OpenKind::Inotify=>put_bytes(&mut out,&mut off,b"anon_inode:inotify"),
  crate::fd::OpenKind::SignalFd(_)=>put_bytes(&mut out,&mut off,b"anon_inode:[signalfd]"),
  crate::fd::OpenKind::Netlink=>put_bytes(&mut out,&mut off,b"socket:[netlink]"),
  crate::fd::OpenKind::IoUring=>put_bytes(&mut out,&mut off,b"anon_inode:[io_uring]"),
  crate::fd::OpenKind::PidFd(p)=>{put_bytes(&mut out,&mut off,b"anon_inode:[pidfd:");append_num(&mut out,&mut off,p as u64);put_bytes(&mut out,&mut off,b"]")}
 }
 Some((out,off))
}
fn ns_link(path:&str,rest:&str)->Result<Inode>{
 let (uts,ipc,mnt,pid,net,user,cgroup,time)=crate::task::with_current_process(|pr|(pr.ns_uts,pr.ns_ipc,pr.ns_mnt,pr.ns_pid,pr.ns_net,pr.ns_user,pr.ns_cgroup,pr.ns_time)).unwrap_or((1,1,1,1,1,1,1,1));
 let id=match rest{"uts"=>uts,"ipc"=>ipc,"mnt"=>mnt,"pid"=>pid,"net"=>net,"user"=>user,"cgroup"=>cgroup,"time"=>time,_=>return Err(errno::ENOENT)};
 let prefix: &[u8]=match rest{"uts"=>b"uts:[", "ipc"=>b"ipc:[", "mnt"=>b"mnt:[", "pid"=>b"pid:[", "net"=>b"net:[", "user"=>b"user:[", "cgroup"=>b"cgroup:[", "time"=>b"time:[", _=>b"ns:["};
 let mut buf=[0u8;32]; let mut off=0usize; buf[off..off+prefix.len()].copy_from_slice(prefix); off+=prefix.len();
 let mut tmp=[0u8;20]; let mut i=0; let mut v=id;
 if v==0{tmp[0]=b'0';i=1}else{while v>0{tmp[i]=b'0'+(v%10)as u8;v/=10;i+=1}for j in 0..i/2{tmp.swap(j,i-1-j)}}
 buf[off..off+i].copy_from_slice(&tmp[..i]); off+=i; buf[off]=b']'; off+=1;
 proc_symlink(path,&buf[..off])
}
fn is_proc_file_inode(ino:u64)->bool{ (ino>>12)==0x90 }
fn is_sys_file_inode(ino:u64)->bool{ (ino>>12)==0xa }

fn append(buf:&mut [u8], off:&mut usize, data:&[u8]){
 let n=core::cmp::min(data.len(), buf.len().saturating_sub(*off));
 buf[*off..*off+n].copy_from_slice(&data[..n]);
 *off += n;
}
fn append_num(buf:&mut [u8], off:&mut usize, mut v:u64){
 let mut tmp=[0u8;20]; let mut i=0;
 if v==0{tmp[0]=b'0';i=1}
 else{while v>0{tmp[i]=b'0'+(v%10)as u8;v/=10;i+=1}}
 for j in 0..i/2{tmp.swap(j,i-1-j)}
 append(buf,off,&tmp[..i]);
}
fn append_hex(buf:&mut[u8],off:&mut usize,mut v:u64){
 let mut tmp=[0u8;16];let mut i=0;
 if v==0{tmp[0]=b'0';i=1}else{while v>0{let d=(v%16)as u8;tmp[i]=if d<10{b'0'+d}else{b'a'+d-10};v/=16;i+=1}}
 for j in 0..i/2{tmp.swap(j,i-1-j)}
 append(buf,off,&tmp[..i]);
}
fn seg_perms(flags:u32,out:&mut[u8;4]){out[0]=if flags&crate::elf::PF_R!=0{b'r'}else{b'-'};out[1]=if flags&crate::elf::PF_W!=0{b'w'}else{b'-'};out[2]=if flags&crate::elf::PF_X!=0{b'x'}else{b'-'};out[3]=b'p';}
fn vma_perms(flags:VmFlags,out:&mut[u8;4]){out[0]=if flags.contains(VmFlags::READ){b'r'}else{b'-'};out[1]=if flags.contains(VmFlags::WRITE){b'w'}else{b'-'};out[2]=if flags.contains(VmFlags::EXEC){b'x'}else{b'-'};out[3]=b'p';}
fn write_map_line(buf:&mut[u8],off:&mut usize,start:u64,end:u64,perm:&[u8;4],offset:u64,ino:u64,path:&[u8]){
 append_hex(buf,off,start);append(buf,off,b"-");append_hex(buf,off,end);append(buf,off,b" ");append(buf,off,perm);append(buf,off,b" ");append_hex(buf,off,offset);append(buf,off,b" 00:00 ");append_num(buf,off,ino);append(buf,off,b" ");append(buf,off,path);append(buf,off,b"\n");
}
fn proc_fill(subject:ProcSubject,file:&str)->usize{
 unsafe{PROC_BUF=[0;4096]}
 let mut off=0usize;
 let buf=unsafe{&mut *(&raw mut PROC_BUF)};
 match file{
  "cmdline"=>{proc_with(subject,|p|append(buf,&mut off,&p.cmdline[..p.cmdline_len]));}
  "environ"=>{proc_with(subject,|p|append(buf,&mut off,&p.environ[..p.environ_len]));}
  "status"=>{
   let pid=subject_pid(subject);
   let (comm,n)=proc_with(subject,|p|{let mut b=[0u8;64];let n=comm_of(p,&mut b);(b,n)}).unwrap_or(([0u8;64],0));
   let (uid,euid,suid,fsuid)=proc_with(subject,|p|(p.uid,p.euid,p.suid,p.fsuid)).unwrap_or((0,0,0,0));
   let (gid,egid,sgid,fsgid)=proc_with(subject,|p|(p.gid,p.egid,p.sgid,p.fsgid)).unwrap_or((0,0,0,0));
   let (vsize,rss)=proc_with(subject,|p|(p.vma_bytes(),p.resident_pages())).unwrap_or((0,0));
   append(buf,&mut off,b"Name:\t");append(buf,&mut off,&comm[..n]);append(buf,&mut off,b"\nState:\tR (running)\nTgid:\t");append_num(buf,&mut off,pid as u64);append(buf,&mut off,b"\nPid:\t");append_num(buf,&mut off,pid as u64);append(buf,&mut off,b"\nPPid:\t");append_num(buf,&mut off,crate::task::ppid_of(pid) as u64);append(buf,&mut off,b"\n");
   for (label,vals) in [(b"Uid:\t".as_slice(),[uid,euid,suid,fsuid]),(b"Gid:\t".as_slice(),[gid,egid,sgid,fsgid])]{append(buf,&mut off,label);for v in vals{append_num(buf,&mut off,v as u64);append(buf,&mut off,b"\t")}if off>0{buf[off-1]=b'\n'}}
   append(buf,&mut off,b"Threads:\t");append_num(buf,&mut off,crate::task::thread_count(pid) as u64);append(buf,&mut off,b"\nVmSize:\t");append_num(buf,&mut off,(vsize/1024) as u64);append(buf,&mut off,b" kB\nVmRSS:\t");append_num(buf,&mut off,(rss*4) as u64);append(buf,&mut off,b" kB\n");
  }
  "stat"=>{
   let pid=subject_pid(subject);
   let (comm,n)=proc_with(subject,|p|{let mut b=[0u8;64];let n=comm_of(p,&mut b);(b,n)}).unwrap_or(([0u8;64],0));
   let (vsize,rss)=proc_with(subject,|p|(p.vma_bytes(),p.resident_pages())).unwrap_or((0,0));
   append_num(buf,&mut off,pid as u64);append(buf,&mut off,b" (");append(buf,&mut off,&comm[..n]);append(buf,&mut off,b") R ");append_num(buf,&mut off,crate::task::ppid_of(pid) as u64);append(buf,&mut off,b" ");append_num(buf,&mut off,pid as u64);append(buf,&mut off,b" ");append_num(buf,&mut off,pid as u64);append(buf,&mut off,b" 0 -1 0 0 0 0 0 0 0 0 20 0 ");append_num(buf,&mut off,crate::task::thread_count(pid) as u64);append(buf,&mut off,b" 0 0 ");append_num(buf,&mut off,vsize);append(buf,&mut off,b" ");append_num(buf,&mut off,rss);append(buf,&mut off,b" 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n");
  }
  "statm"=>{let (vsize,rss)=proc_with(subject,|p|(p.vma_bytes(),p.resident_pages())).unwrap_or((0,0));append_num(buf,&mut off,vsize/4096);append(buf,&mut off,b" ");append_num(buf,&mut off,rss);append(buf,&mut off,b" 0 0 0 0 0\n");}
  "maps"=>{
   proc_with(subject,|p|{
    for s in &p.segs[..p.nsegs]{let mut perm=[0u8;4];seg_perms(s.flags,&mut perm);write_map_line(buf,&mut off,s.start,s.end,&perm,0,0,b"");}
    for i in 0..p.vm.resource_stats().areas{
     if let Some(a)=p.vm.area(i){
      let mut perm=[0u8;4];vma_perms(a.flags,&mut perm);
      let mut nbuf=[0u8;128];let mut nlen=0usize;
      let (offset,ino):(u64,u64)=match a.kind{
       crate::vm::VmKind::File{inode,offset}=>{nlen=core::cmp::min(inode.name_len,nbuf.len());nbuf[..nlen].copy_from_slice(&inode.name[..nlen]);(offset,inode.ino)}
       crate::vm::VmKind::Brk=>{nbuf[..6].copy_from_slice(b"[heap]");nlen=6;(0,0)}
       _=>(0,0),
      };
      write_map_line(buf,&mut off,a.start as u64,a.end as u64,&perm,offset,ino,&nbuf[..nlen]);
     }
    }
    if p.stack_top>p.stack_base{let perm=[b'r',b'w',b'-',b'p'];write_map_line(buf,&mut off,p.stack_base,p.stack_top,&perm,0,0,b"[stack]");}
   });
  }
  "cpuinfo"=>{append(buf,&mut off,b"processor\t: 0\nvendor_id\t: GenuineIntel\ncpu family\t: 6\nmodel name\t: Slop/MNU virtual CPU\n");}
  "meminfo"=>{let (total,free)=crate::memory::frame_stats();append(buf,&mut off,b"MemTotal:       ");append_num(buf,&mut off,total*4);append(buf,&mut off,b" kB\nMemFree:        ");append_num(buf,&mut off,free*4);append(buf,&mut off,b" kB\nMemAvailable:   ");append_num(buf,&mut off,free*4);append(buf,&mut off,b" kB\nBuffers:        0 kB\nCached:         0 kB\nSwapTotal:      0 kB\nSwapFree:       0 kB\n");}
  "uptime"=>{let ns=crate::time::monotonic_ns();let sec=ns/1_000_000_000;let cs=(ns%1_000_000_000)/10_000_000;append_num(buf,&mut off,sec);append(buf,&mut off,if cs<10{b".0"}else{b"."});append_num(buf,&mut off,cs);append(buf,&mut off,b" ");append_num(buf,&mut off,sec);append(buf,&mut off,if cs<10{b".0"}else{b"."});append_num(buf,&mut off,cs);append(buf,&mut off,b"\n");}
  "version"=>{append(buf,&mut off,b"Linux version 6.6.0-slop (slop@build) (rustc) #1 SMP\n");}
  "loadavg"=>{let mut count=0u64;crate::task::live_pids(|_|count+=1);append(buf,&mut off,b"0.00 0.00 0.00 ");append_num(buf,&mut off,count.max(1));append(buf,&mut off,b"/");append_num(buf,&mut off,count.max(1));append(buf,&mut off,b" ");append_num(buf,&mut off,crate::task::last_pid() as u64);append(buf,&mut off,b"\n");}
  "mounts"=>{
   append(buf,&mut off,b"rootfs / rootfs rw 0 0\n");
   let ns=current_mount_ns();let reg=MOUNT_REGISTRY.lock();
   for e in &reg.entries{if !e.used||e.ns!=ns{continue}let (src,fs)=match e.kind{MountKind::Ext2=>(b"vda".as_slice(),b"ext2".as_slice()),_=>(b"tmpfs".as_slice(),b"tmpfs".as_slice())};append(buf,&mut off,src);append(buf,&mut off,b" ");append(buf,&mut off,&e.target[..e.tlen]);append(buf,&mut off,b" ");append(buf,&mut off,fs);append(buf,&mut off,b" rw 0 0\n");}
  }
  _=>{}
 }
 off
}
fn proc_lookup(path:&str)->Result<Inode>{
 if path=="/proc"{return Ok(proc_dir_inode(path,0x9000))}
 if let Some(file)=path.strip_prefix("/proc/"){
  if !file.contains('/')&&proc_sys_name(file){return proc_file_inode(path,ProcSubject::SelfProc,file)}
 }
 if let Some((subject,rest))=parse_proc(path){
  if rest.is_empty(){if subject!=ProcSubject::SelfProc&&!proc_exists(subject){return Err(errno::ENOENT)}return Ok(proc_dir_inode(path,0x9001))}
  if !proc_exists(subject){return Err(errno::ENOENT)}
  if subject==ProcSubject::SelfProc{
   if rest=="/fd"{return Ok(proc_dir_inode(path,0x9002))}
   if rest=="/ns"{return Ok(proc_dir_inode(path,0x9003))}
   if let Some(ns)=rest.strip_prefix("/ns/"){return ns_link(path,ns)}
   if let Some(fd)=rest.strip_prefix("/fd/"){
    if let Ok(fd)=fd.parse::<usize>(){if let Some((t,tn))=fd_link_target(fd){return proc_symlink(path,&t[..tn])}}
    return Err(errno::ENOENT)
   }
  }
  if rest=="/exe"{let exe=proc_with(subject,|p|(p.exe_path,p.exe_len)).unwrap_or(([0;64],0));if exe.1==0{return Err(errno::ENOENT)}return proc_symlink(path,&exe.0[..exe.1])}
  if rest=="/root"||rest=="/cwd"{
   let (arr,len)=if rest=="/root"{proc_with(subject,|p|(p.root,p.root_len)).unwrap_or(([b'/' ;128],1))}else{proc_with(subject,|p|(p.cwd,p.cwd_len)).unwrap_or(([b'/' ;128],1))};
   return proc_symlink(path,&arr[..len])
  }
  if let Some(file)=rest.strip_prefix('/'){if proc_file_name(file){return proc_file_inode(path,subject,file)}}
 }
 Err(errno::ENOENT)
}

pub fn symlink(target:&str,path:&str)->Result<Inode>{
  if !path.starts_with('/')||path.as_bytes().contains(&0){return Err(errno::EINVAL)}
  if target.len()>4095{return Err(errno::ENAMETOOLONG)}
  let route=route_path(path,current_mount_ns());
  match route.kind{Some(MountKind::Tmpfs)=>{let rel=core::str::from_utf8(&route.relative[..route.rlen]).map_err(|_|errno::EINVAL)?;return tmpfs_add_node(route.fs_id,rel,path,S_IFLNK|0o777,0,Some(target.as_bytes()))},Some(MountKind::Ext2)=>return Err(errno::EROFS),None=>{}}
  let mut m=ROOT.lock();
 if m.len>=MAX_NODES{return Err(errno::EMFILE)}
 if m.nodes[..m.len].iter().any(|n|&n.name[..n.name_len]==path.as_bytes()){return Err(errno::EEXIST)}
 let tb=target.as_bytes();
 let frame=match crate::memory::alloc_frame(){Some(f)=>f,None=>return Err(errno::ENOMEM)};
 unsafe{crate::arch::x86_64::phys::Identity.table(frame).fill(0)}
  unsafe{core::slice::from_raw_parts_mut(frame as*mut u8,tb.len()).copy_from_slice(tb)}
 let ino=(m.len+2)as u64 + 0x1000;
 let mut name=[0u8;NAME_MAX+1]; let nl=path.len(); name[..nl].copy_from_slice(path.as_bytes());
 let node=Inode{ino,mode:S_IFLNK|0o777,size:tb.len() as u64,capacity:tb.len() as u64,data:frame as*mut u8,rdev:0,pages:[frame,0,0,0,0,0,0,0],npages:1,name,name_len:nl};
 let idx=m.len; m.nodes[idx]=node; m.len+=1;
 Ok(node)
}
pub fn link(old:&str,new:&str)->Result<Inode>{
  if !old.starts_with('/')||!new.starts_with('/')||new.as_bytes().contains(&0){return Err(errno::EINVAL)}
  let ns=current_mount_ns();let old_route=route_path(old,ns);let new_route=route_path(new,ns);
  if old_route.kind!=new_route.kind||old_route.fs_id!=new_route.fs_id{return Err(errno::EXDEV)}
  match old_route.kind{Some(MountKind::Tmpfs)=>return Err(errno::EOPNOTSUPP),Some(MountKind::Ext2)=>return Err(errno::EROFS),None=>{}}
  let mut m=ROOT.lock();
 let oldnode=m.nodes[..m.len].iter().copied().find(|n|&n.name[..n.name_len]==old.as_bytes()).ok_or(errno::ENOENT)?;
 if m.len>=MAX_NODES{return Err(errno::EMFILE)}
 if m.nodes[..m.len].iter().any(|n|&n.name[..n.name_len]==new.as_bytes()){return Err(errno::EEXIST)}
 let mut name=[0u8;NAME_MAX+1]; let nl=new.len(); name[..nl].copy_from_slice(new.as_bytes());
 let mut node=oldnode; node.name=name; node.name_len=nl; node.ino=(m.len+2)as u64+0x2000; node.rdev=oldnode.rdev; node.pages=oldnode.pages; node.npages=oldnode.npages;
 let idx=m.len; m.nodes[idx]=node; m.len+=1;
 Ok(node)
}

pub fn resolve(cwd:&[u8],input:&str,out:&mut [u8;128])->Result<usize>{
 if input.as_bytes().contains(&0){return Err(errno::EINVAL)}
 let mut combined=[0u8;256];let mut clen=0usize;
 if !input.starts_with('/'){
  let bl=core::cmp::min(cwd.len(),120);
  combined[..bl].copy_from_slice(&cwd[..bl]);clen=bl;
  if clen>0 && combined[clen-1]!=b'/'{combined[clen]=b'/';clen+=1;}
 }
 let il=core::cmp::min(input.len(),120);
 combined[clen..clen+il].copy_from_slice(&input.as_bytes()[..il]);clen+=il;
 let mut starts=[0usize;32];let mut ends=[0usize;32];let mut ncomp=0usize;
 let mut i=0usize;let mut inseg=false;let mut st=0usize;
 while i<=clen{
  let c=if i<clen{combined[i]}else{b'/'};
  if c==b'/'{
   if inseg && ncomp<32{starts[ncomp]=st;ends[ncomp]=i;ncomp+=1;inseg=false;}
  } else if !inseg{st=i;inseg=true;}
  i+=1;
 }
 let mut res=[(0usize,0usize);32];let mut rdepth=0usize;
 for ci in 0..ncomp{
  let seg=&combined[starts[ci]..ends[ci]];
  if seg==b"."{continue;}
  if seg==b".." {if rdepth>0{rdepth-=1;}continue;}
  if rdepth<32{res[rdepth]=(starts[ci],ends[ci]);rdepth+=1;}
 }
 out[0]=b'/';let mut len=1usize;
 for ci in 0..rdepth{
  if len>1{out[len]=b'/';len+=1;}
  let (a,b)=res[ci];let slen=b-a;
  if len+slen>=127{return Err(errno::ENAMETOOLONG)}
  out[len..len+slen].copy_from_slice(&combined[a..b]);len+=slen;
 }
 if len==0{out[0]=b'/';len=1;}
 Ok(len)
}
pub fn resolve_nofollow(cwd:&[u8],input:&str,out:&mut [u8;128])->Result<usize>{
 resolve_follow(cwd,input,false,out)
}
pub fn resolve_follow(cwd:&[u8],input:&str,follow_last:bool,out:&mut [u8;128])->Result<usize>{
 resolve_inner(cwd,input,follow_last,out,0)
}
fn resolve_inner(cwd:&[u8],input:&str,follow_last:bool,out:&mut [u8;128],depth:usize)->Result<usize>{
 if depth>40{return Err(errno::ELOOP)}
 if input.as_bytes().contains(&0){return Err(errno::EINVAL)}
 let mut outlen= if input.starts_with('/'){
  out[0]=b'/';1usize
 } else {
  let mut l=0usize;
  while l<cwd.len() && l<127 {out[l]=cwd[l];l+=1;}
  if l==0{out[0]=b'/';l=1;}
  if l>1 && out[l-1]!=b'/' && l<127 {out[l]=b'/';l+=1;}
  if l==0 {out[0]=b'/';l=1;}
  l
 };
 let bytes=input.as_bytes();
 let mut comps=[(0usize,0usize);32];
 let mut ncomp=0usize;
 let mut i=0usize; let mut start=0usize; let mut inseg=false;
 while i<=bytes.len(){
  let c=if i<bytes.len(){bytes[i]}else{b'/'};
  if c==b'/'{
   if inseg && ncomp<32 {comps[ncomp]=(start,i);ncomp+=1;inseg=false;}
  } else if !inseg {start=i;inseg=true;}
  i+=1;
 }
 for ci in 0..ncomp{
  let (a,b)=comps[ci];
  let comp=&bytes[a..b];
  if comp==b"." {continue;}
  if comp==b".." {
   if outlen>1{
    if out[outlen-1]==b'/' {outlen-=1;}
    while outlen>1 && out[outlen-1]!=b'/' {outlen-=1;}
    if outlen==0 {out[0]=b'/';outlen=1;}
   }
   continue;
  }
  if outlen>1 && out[outlen-1]!=b'/' && outlen<127 {out[outlen]=b'/';outlen+=1;}
  if outlen==1 && out[0]==b'/' {
   if outlen+comp.len()>127 {return Err(errno::ENAMETOOLONG)}
   out[1..1+comp.len()].copy_from_slice(comp); outlen=1+comp.len();
  } else {
   if outlen>0 && out[outlen-1]!=b'/' && outlen<127 {out[outlen]=b'/';outlen+=1;}
   if outlen+comp.len()>127 {return Err(errno::ENAMETOOLONG)}
   out[outlen..outlen+comp.len()].copy_from_slice(comp); outlen+=comp.len();
  }
  let is_last=ci==ncomp-1;
  let should_follow=if is_last {follow_last} else {true};
  if should_follow{
   let pathstr=core::str::from_utf8(&out[..outlen]).map_err(|_|errno::EINVAL)?;
   if let Ok(inode)=lookup(pathstr){
    if inode.mode & 0o170000 == S_IFLNK {
     let tlen=inode.size as usize;
     if tlen>=128 {return Err(errno::ENAMETOOLONG)}
     let mut target=[0u8;128];
     if tlen>0 {target[..tlen].copy_from_slice(unsafe{core::slice::from_raw_parts(inode.data,tlen)});}
     let tstr=core::str::from_utf8(&target[..tlen]).map_err(|_|errno::EINVAL)?;
     if outlen>1{
      if out[outlen-1]==b'/' {outlen-=1;}
      while outlen>1 && out[outlen-1]!=b'/' {outlen-=1;}
      if outlen==0 {out[0]=b'/';outlen=1;}
     }
     if outlen==0 {out[0]=b'/';outlen=1;}
     let parent_len=outlen;
     let mut tmp=[0u8;128];
     let newlen=resolve_inner(&out[..parent_len],tstr,true,&mut tmp,depth+1)?;
     out[..newlen].copy_from_slice(&tmp[..newlen]);
     outlen=newlen;
    }
   }
  }
 }
 if outlen==0 {out[0]=b'/';outlen=1;}
 Ok(outlen)
}

pub fn lookup(path:&str)->Result<Inode>{lookup_in_namespace(path,current_mount_ns())}
pub fn lookup_in_namespace(path:&str,ns:u64)->Result<Inode>{
  if !path.starts_with('/')||path.as_bytes().contains(&0){return Err(errno::ENOENT)}
  let route=route_path(path,ns);
  match route.kind{
   Some(MountKind::Tmpfs)=>{let rel=core::str::from_utf8(&route.relative[..route.rlen]).map_err(|_|errno::EINVAL)?;return tmpfs_lookup(route.fs_id,rel,path)},
   Some(MountKind::Ext2)=>{let rel=core::str::from_utf8(&route.relative[..route.rlen]).map_err(|_|errno::EINVAL)?;return visible_inode(crate::ext2::lookup(rel)?,path)},
   None=>{}
  }
  if path=="/"{let mut name=[0u8;NAME_MAX+1];name[0]=b'/';return Ok(Inode{ino:1,mode:S_IFDIR|0o755,size:0,capacity:0,data:core::ptr::null_mut(),rdev:0,pages:[0;8],npages:0,name,name_len:1})}
 if path=="/proc"||path.starts_with("/proc/"){
  return proc_lookup(path)
 }
 if path=="/sys"||path=="/sys/class"||path=="/sys/class/net"||path=="/sys/block"||path=="/sys/devices"||path=="/sys/kernel"||path=="/sys/fs"||is_sys_file(path){
  return sys_lookup(path)
 }
 if path=="/mnt"{
  let mut name=[0u8;NAME_MAX+1]; name[..4].copy_from_slice(b"/mnt"); return Ok(Inode{ino:0xb000,mode:S_IFDIR|0o755,size:0,capacity:0,data:core::ptr::null_mut(),rdev:0,pages:[0;8],npages:0,name,name_len:4})
 }
  let m=ROOT.lock();
 if !m.mounted{return Err(errno::ENOENT)}
 m.nodes[..m.len].iter().copied().find(|n|&n.name[..n.name_len]==path.as_bytes()).ok_or(errno::ENOENT)
}
pub fn read_off(inode:Inode,off:u64,dst:&mut[u8])->Result<usize>{
 if is_proc_file_inode(inode.ino)||is_sys_file_inode(inode.ino){ if off>=inode.size{return Ok(0)}; let n=core::cmp::min(dst.len(),(inode.size-off)as usize); dst[..n].copy_from_slice(unsafe{core::slice::from_raw_parts(inode.data.add(off as usize),n)}); return Ok(n) }
 if inode.rdev>0 && inode.mode&0o020000==0 && inode.mode&0o060000==0 && inode.npages==0 {
  return crate::ext2::read(inode.rdev as u32,off,dst)
 }
 if let Some(fs_id)=tmpfs_fs_id(inode.ino){let registry=MOUNT_REGISTRY.lock();let store=registry.tmpfs.get(fs_id).filter(|s|s.used).ok_or(errno::ENOENT)?;let node=store.nodes[..store.len].iter().find(|n|n.ino==inode.ino).ok_or(errno::ENOENT)?;return read_inode_data(node,off,dst)}
 let m=ROOT.lock();
 let node=m.nodes[..m.len].iter().find(|n|n.ino==inode.ino).ok_or(errno::ENOENT)?;
 read_inode_data(node,off,dst)
}
pub fn write_off(inode:Inode,off:u64,src:&[u8])->Result<usize>{
 if is_proc_file_inode(inode.ino)||is_sys_file_inode(inode.ino){return Err(errno::EROFS)}
 if let Some(fs_id)=tmpfs_fs_id(inode.ino){let mut registry=MOUNT_REGISTRY.lock();let store=registry.tmpfs.get_mut(fs_id).filter(|s|s.used).ok_or(errno::ENOENT)?;let node=store.nodes[..store.len].iter_mut().find(|n|n.ino==inode.ino).ok_or(errno::ENOENT)?;let wrote=tmpfs_write_inode(node,off,src)?;drop(registry);if wrote>0{if let Ok(path)=core::str::from_utf8(&inode.name[..inode.name_len]){crate::inotify::notify(path,crate::inotify::IN_MODIFY)}}return Ok(wrote)}
 let mut m=ROOT.lock();
 let idx=m.nodes[..m.len].iter().position(|&n|n.ino==inode.ino).ok_or(errno::ENOENT)?;
 let node=&mut m.nodes[idx];
 if node.npages==0 { node.pages[0]=node.data as u64; node.npages=1; node.capacity=node.capacity.max(4096); }
 let end=off.checked_add(src.len() as u64).ok_or(errno::EINVAL)?;
 let needed=((end as usize+4095)/4096).max(1);
 while (node.npages as usize) < needed && (node.npages as usize) < 8 {
  let frame=match crate::memory::alloc_frame(){Some(f)=>f,None=>return Err(errno::ENOMEM)};
  unsafe{crate::arch::x86_64::phys::Identity.table(frame).fill(0)}
  node.pages[node.npages as usize]=frame;
  node.npages+=1;
  node.capacity=(node.npages as u64)*4096;
 }
 if end>node.capacity{return Err(errno::ENOSPC)}
 let mut done=0usize;
 while done<src.len(){
  let poff=off as usize+done;
  let pi=poff/4096; let po=poff%4096;
  if pi>=node.npages as usize{return Err(errno::ENOSPC)}
  let frame=node.pages[pi];
  let n=core::cmp::min(src.len()-done,4096-po);
  unsafe{core::slice::from_raw_parts_mut((frame as *mut u8).add(po),n).copy_from_slice(&src[done..done+n])};
  done+=n;
 }
  if end>node.size{node.size=end;}
  let mut path=[0u8;128];let path_len=node.name_len;path[..path_len].copy_from_slice(&node.name[..path_len]);drop(m);
  if !src.is_empty(){if let Ok(path)=core::str::from_utf8(&path[..path_len]){crate::inotify::notify(path,crate::inotify::IN_MODIFY)}}
  Ok(src.len())
}

pub fn mknod(path:&str,mode:u32,rdev:u64)->Result<Inode>{
  if !path.starts_with('/')||path.as_bytes().contains(&0){return Err(errno::EINVAL)}
  let route=route_path(path,current_mount_ns());
  match route.kind{Some(MountKind::Tmpfs)=>{let rel=core::str::from_utf8(&route.relative[..route.rlen]).map_err(|_|errno::EINVAL)?;return tmpfs_add_node(route.fs_id,rel,path,mode,rdev,None)},Some(MountKind::Ext2)=>return Err(errno::EROFS),None=>{}}
  let mut m=ROOT.lock();
 if m.len>=MAX_NODES{return Err(errno::EMFILE)}
 if m.nodes[..m.len].iter().any(|n|&n.name[..n.name_len]==path.as_bytes()){return Err(errno::EEXIST)}
 let nl=path.len();
 if nl>NAME_MAX{return Err(errno::ENAMETOOLONG)}
 let ino=(m.len+2)as u64+0x3000;
 let mut name=[0u8;NAME_MAX+1];name[..nl].copy_from_slice(path.as_bytes());
 let node=Inode{ino,mode,size:0,capacity:0,data:core::ptr::null_mut(),rdev,pages:[0;8],npages:0,name,name_len:nl};
 let idx=m.len; m.nodes[idx]=node; m.len+=1;
 Ok(node)
}

fn is_sys_file(path:&str)->bool{
 matches!(path,"/sys/kernel/hostname"|"/sys/kernel/osrelease"|"/sys/class/net/eth0/address"|"/sys/class/net/eth0/operstate"|"/sys/block/vda/size"|"/sys/block/vda/dev")
}
fn sys_fill(path:&str)->usize{
 unsafe{PROC_BUF=[0;4096]}
 let mut off=0usize;
 let buf=unsafe{&mut *(&raw mut PROC_BUF)};
 match path{
  "/sys/kernel/hostname"=>{append(buf,&mut off,b"slop\n");},
  "/sys/kernel/osrelease"=>{append(buf,&mut off,b"6.6.0-slop\n");},
  "/sys/class/net/eth0/address"=>{append(buf,&mut off,b"52:54:00:12:34:56\n");},
  "/sys/class/net/eth0/operstate"=>{append(buf,&mut off,b"up\n");},
  "/sys/block/vda/size"=>{append(buf,&mut off,b"2097152\n");},
  "/sys/block/vda/dev"=>{append(buf,&mut off,b"254:0\n");},
  _=>{},
 }
 off
}
fn sys_lookup(path:&str)->Result<Inode>{
 if path=="/sys"||path=="/sys/class"||path=="/sys/class/net"||path=="/sys/block"||path=="/sys/devices"||path=="/sys/kernel"||path=="/sys/fs"{
  let mut name=[0u8;NAME_MAX+1]; let b=path.as_bytes(); name[..b.len()].copy_from_slice(b);
  return Ok(Inode{ino:0xa000+path.len() as u64,mode:S_IFDIR|0o555,name,name_len:b.len(),size:0,capacity:0,data:core::ptr::null_mut(),rdev:0,pages:[0;8],npages:0})
 }
 if path=="/sys/class/net/eth0"||path=="/sys/block/vda"{
  let mut name=[0u8;NAME_MAX+1]; let b=path.as_bytes(); name[..b.len()].copy_from_slice(b);
  return Ok(Inode{ino:0xa200+path.len() as u64,mode:S_IFDIR|0o555,name,name_len:b.len(),size:0,capacity:0,data:core::ptr::null_mut(),rdev:0,pages:[0;8],npages:0})
 }
 if is_sys_file(path){
  let size=sys_fill(path);
  let mut name=[0u8;NAME_MAX+1]; let b=path.as_bytes(); name[..b.len()].copy_from_slice(b);
  let ino=0xa100+path.len() as u64;
  return Ok(Inode{ino,mode:S_IFREG|0o444,name,name_len:b.len(),size:size as u64,capacity:4096,data:unsafe{(&raw mut PROC_BUF).cast::<u8>()},rdev:0,pages:[0;8],npages:0})
 }
 Err(errno::ENOENT)
}

fn proc_file_inode(path:&str,subject:ProcSubject,file:&str)->Result<Inode>{
 let size=proc_fill(subject,file);
 let mut name=[0u8;NAME_MAX+1]; let b=path.as_bytes(); let n=core::cmp::min(b.len(),NAME_MAX); name[..n].copy_from_slice(&b[..n]);
 let ino=0x90000 + ((path.len() as u64)&0xfff);
 Ok(Inode{ino,mode:S_IFREG|0o444,name,name_len:n,size:size as u64,capacity:4096,data:unsafe{(&raw mut PROC_BUF).cast::<u8>()},rdev:0,pages:[0;8],npages:0})
}

fn proc_symlink(path:&str,target:&[u8])->Result<Inode>{
 let frame=match crate::memory::alloc_frame(){Some(f)=>f,None=>return Err(errno::ENOMEM)};
 unsafe{crate::arch::x86_64::phys::Identity.table(frame).fill(0)}
 if target.len()>4095{return Err(errno::ENAMETOOLONG)}
 unsafe{core::slice::from_raw_parts_mut(frame as*mut u8,target.len()).copy_from_slice(target)}
 let mut name=[0u8;NAME_MAX+1]; let b=path.as_bytes(); name[..b.len()].copy_from_slice(b);
 let ino=0x93000 + path.len() as u64;
 Ok(Inode{ino,mode:S_IFLNK|0o777,name,name_len:b.len(),size:target.len() as u64,capacity:target.len() as u64,data:frame as*mut u8,rdev:0,pages:[frame,0,0,0,0,0,0,0],npages:1})
}
