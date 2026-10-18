//! Allocation-free initramfs VFS with read/write support.
use crate::{arch::x86_64::{paging::PhysAccess,phys::Identity},errno,sync::SpinLock};
pub type Result<T>=core::result::Result<T,i64>;
pub const S_IFREG:u32=0o100000;pub const S_IFDIR:u32=0o040000;pub const S_IFBLK:u32=0o060000;const MAX_NODES:usize=32;const NAME_MAX:usize=127;
#[derive(Clone,Copy)]pub struct Inode{pub ino:u64,pub mode:u32,pub size:u64,pub capacity:u64,pub data:*mut u8,pub rdev:u64,pub pages:[u64;8],pub npages:u8,pub name:[u8;NAME_MAX+1],pub name_len:usize}
unsafe impl Send for Inode{} unsafe impl Sync for Inode{}
const EMPTY:Inode=Inode{ino:0,mode:0,size:0,capacity:0,data:core::ptr::null_mut(),rdev:0,pages:[0;8],npages:0,name:[0;NAME_MAX+1],name_len:0};
struct Mount{nodes:[Inode;MAX_NODES],len:usize,mounted:bool}
static ROOT:SpinLock<Mount>=SpinLock::new(Mount{nodes:[EMPTY;MAX_NODES],len:0,mounted:false});
fn u16le(b:&[u8],o:usize)->Result<u16>{let x=b.get(o..o+2).ok_or(errno::EINVAL)?;Ok(u16::from_le_bytes([x[0],x[1]]))}
fn u32le(b:&[u8],o:usize)->Result<u32>{let x=b.get(o..o+4).ok_or(errno::EINVAL)?;Ok(u32::from_le_bytes([x[0],x[1],x[2],x[3]]))}
fn align8(v:usize)->Result<usize>{v.checked_add(7).map(|x|x&!7).ok_or(errno::EINVAL)}
pub fn mount_initramfs(image:&'static [u8])->Result<usize>{if image.get(..8)!=Some(b"NSAR0001"){return Err(errno::EINVAL)}let count=u32le(image,8)? as usize;if count>MAX_NODES{return Err(errno::ENOMEM)}let mut m=ROOT.lock();m.len=0;let mut p=12usize;for i in 0..count{let nl=u16le(image,p)? as usize;let raw=u16le(image,p+2)? as u32;let size=u32le(image,p+4)? as usize;p+=8;let mode=if raw&0o170000!=0{raw}else{S_IFREG|raw&0o777};if nl==0||nl>NAME_MAX{return Err(errno::EINVAL)}let n=image.get(p..p+nl).ok_or(errno::EINVAL)?;if n[0]!=b'/'||n.contains(&0){return Err(errno::EINVAL)}let mut name=[0u8;NAME_MAX+1];name[..nl].copy_from_slice(n);p=align8(p+nl)?;let data=image.get(p..p+size).ok_or(errno::EINVAL)?;m.nodes[i]=Inode{ino:(i+2)as u64,mode,size:size as u64,capacity:size as u64,data:data.as_ptr()as*mut u8,rdev:0,pages:[data.as_ptr()as u64,0,0,0,0,0,0,0],npages:1,name,name_len:nl};p=align8(p+size)?;}m.len=count;m.mounted=true;Ok(count)}
pub fn read(inode:Inode,off:u64,dst:&mut[u8])->Result<usize>{
 if inode.rdev>0 && inode.mode&0o020000==0 && inode.mode&0o060000==0 && inode.npages==0 {
  return crate::ext2::read(inode.rdev as u32,off,dst)
 }
 if inode.npages==0 { if off>=inode.size{return Ok(0)} let n=core::cmp::min(dst.len(),(inode.size-off)as usize); let src=unsafe{core::slice::from_raw_parts(inode.data.add(off as usize),n)}; dst[..n].copy_from_slice(src); return Ok(n) }
 if off>=inode.size{return Ok(0)}
 let mut done=0usize;
 while done<dst.len() && off as usize+done < inode.size as usize {
  let poff=off as usize+done;
  let pi=poff/4096; let po=poff%4096;
  if pi>=inode.npages as usize{return Ok(done)}
  let frame=inode.pages[pi];
  let n=core::cmp::min(dst.len()-done, core::cmp::min(4096-po, inode.size as usize-poff));
  if n==0{break}
  let src=unsafe{core::slice::from_raw_parts((frame as *const u8).add(po),n)};
  dst[done..done+n].copy_from_slice(src);
  done+=n;
 }
 Ok(done)
}
/* Positional reads resolve the node by ino so a grown size is visible. */
pub fn write(inode:Inode,off:u64,src:&[u8])->Result<usize>{
 let mut m=ROOT.lock();let idx=m.nodes[..m.len].iter().position(|&n|n.ino==inode.ino).ok_or(errno::ENOENT)?;
 // ensure pages
 let mut node=&mut m.nodes[idx];
 if node.npages==0 {
  // initramfs static node: treat data as one page
  node.pages[0]=node.data as u64; node.npages=1; node.capacity=node.capacity.max(4096);
 }
 let end=off.checked_add(src.len() as u64).ok_or(errno::EINVAL)?;
 // allocate pages if needed
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
  let node=Inode{ino,mode:S_IFREG|mode&0o777,size:0,capacity:4096,data:frame as*mut u8,rdev:0,pages:[frame,0,0,0,0,0,0,0],npages:1,name,name_len};
  let idx=m.len; m.nodes[idx]=node;
  m.len+=1;
  crate::inotify::notify(path,crate::inotify::IN_CREATE);
  Ok(node)
}
pub fn unlink(path:&str)->Result<()>{
 let mut m=ROOT.lock();
 let idx=m.nodes[..m.len].iter().position(|n|&n.name[..n.name_len]==path.as_bytes()).ok_or(errno::ENOENT)?;
 if m.nodes[idx].mode&S_IFDIR!=0{return Err(errno::EISDIR)}
 for i in idx..m.len-1{m.nodes[i]=m.nodes[i+1];}
 m.len-=1;
 crate::inotify::notify(path,crate::inotify::IN_DELETE);
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
 let node=Inode{ino,mode:S_IFDIR|mode&0o777,size:0,capacity:0,data:core::ptr::null_mut(),rdev:0,pages:[0;8],npages:0,name,name_len};
 let idx=m.len; m.nodes[idx]=node;
 m.len+=1;
 crate::inotify::notify(path,crate::inotify::IN_CREATE|crate::inotify::IN_ISDIR);
 Ok(node)
}
/* Lists the entries directly under path, calling cb(ino, mode, name) for each.
   "." and ".." are included when the directory exists. */
pub fn readdir(path:&str,mut cb:impl FnMut(u64,u32,&str))->Result<usize>{
 if path=="/proc"{
  cb(0x9001,S_IFDIR,"self"); return Ok(1)
 }
 if path=="/proc/self"{
  cb(0x9002,S_IFDIR,"fd"); cb(0x9010,S_IFDIR,"ns"); cb(0x9011,S_IFLNK,"exe"); cb(0x9012,S_IFLNK,"root"); cb(0x9013,S_IFLNK,"cwd"); cb(0x9003,S_IFREG,"cmdline"); cb(0x9004,S_IFREG,"environ"); cb(0x9005,S_IFREG,"status"); cb(0x9006,S_IFREG,"stat"); cb(0x9007,S_IFREG,"maps"); cb(0x9008,S_IFREG,"cpuinfo"); cb(0x9009,S_IFREG,"meminfo"); cb(0x900a,S_IFREG,"uptime"); cb(0x900b,S_IFREG,"version"); cb(0x900c,S_IFREG,"loadavg"); return Ok(11)
 }
 if path=="/proc/self/fd"{
  cb(0x9100,S_IFLNK,"0"); cb(0x9101,S_IFLNK,"1"); cb(0x9102,S_IFLNK,"2"); return Ok(3)
 }
 if path=="/mnt" && crate::ext2::ready(){
  return crate::ext2::readdir("/",cb).map_err(|e|e)
 }
 if path.starts_with("/mnt/") && crate::ext2::ready(){
  let sub=&path[5..];
  return crate::ext2::readdir(sub,cb).map_err(|e|e)
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
 if path=="/"||path=="/proc"||path=="/proc/self"||path=="/proc/self/fd"||path=="/proc/self/ns"||path=="/sys"||path=="/sys/class"||path=="/sys/class/net"||path=="/sys/block"||path=="/sys/devices"||path=="/sys/kernel"||path=="/sys/fs"{return true}
 let m=ROOT.lock();
 if let Some(n)=m.nodes[..m.len].iter().find(|n|&n.name[..n.name_len]==path.as_bytes()){n.mode&S_IFDIR!=0}else{false}
}
pub fn rmdir(path:&str)->Result<()>{
 if path=="/"{return Err(errno::EINVAL)}
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
 let mut m=ROOT.lock();
 let idx=m.nodes[..m.len].iter().position(|n|&n.name[..n.name_len]==path.as_bytes()).ok_or(errno::ENOENT)?;
 let ty=m.nodes[idx].mode&0o170000;
 m.nodes[idx].mode=ty|(mode&0o7777);
 Ok(())
}
pub fn chmod_ino(ino:u64,mode:u32)->Result<()>{
 let mut m=ROOT.lock();
 let idx=m.nodes[..m.len].iter().position(|n|n.ino==ino).ok_or(errno::ENOENT)?;
 let ty=m.nodes[idx].mode&0o170000;
 m.nodes[idx].mode=ty|(mode&0o7777);
 Ok(())
}
pub fn truncate_path(path:&str,size:u64)->Result<()>{
 let mut m=ROOT.lock();
 let idx=m.nodes[..m.len].iter().position(|n|&n.name[..n.name_len]==path.as_bytes()).ok_or(errno::ENOENT)?;
 if m.nodes[idx].mode&S_IFDIR!=0{return Err(errno::EISDIR)}
 if m.nodes[idx].npages==0 { m.nodes[idx].pages[0]=m.nodes[idx].data as u64; m.nodes[idx].npages=1; m.nodes[idx].capacity=m.nodes[idx].capacity.max(4096); }
 if size>m.nodes[idx].capacity{return Err(errno::ENOSPC)}
 m.nodes[idx].size=size;
 Ok(())
}
pub fn truncate_ino(ino:u64,size:u64)->Result<()>{
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

fn is_proc_file(path:&str)->bool{
 matches!(path,"/proc/self/cmdline"|"/proc/self/environ"|"/proc/self/status"|"/proc/self/stat"|"/proc/self/maps"|"/proc/cpuinfo"|"/proc/meminfo"|"/proc/uptime"|"/proc/version"|"/proc/loadavg")
}
fn is_proc_link(path:&str)->bool{
 path=="/proc/self/exe"||path=="/proc/self/root"||path=="/proc/self/cwd"||path.starts_with("/proc/self/ns/")
}
fn is_proc_file_inode(ino:u64)->bool{ (ino>>12)==0x9 }
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
fn proc_fill(path:&str)->usize{
 unsafe{PROC_BUF=[0;4096]}
 let mut off=0usize;
 let buf=unsafe{&mut *(&raw mut PROC_BUF)};
 let pid=crate::task::current_pid();
 match path{
  "/proc/self/cmdline"=>{append(buf,&mut off,b"init\0");}
  "/proc/self/environ"=>{append(buf,&mut off,b"SLOP_MNU=1\0");}
  "/proc/self/status"=>{
   append(buf,&mut off,b"Name:\tinit\n");
   append(buf,&mut off,b"Pid:\t"); append_num(buf,&mut off,pid as u64); append(buf,&mut off,b"\n");
   append(buf,&mut off,b"PPid:\t"); append_num(buf,&mut off,crate::task::current_ppid() as u64); append(buf,&mut off,b"\n");
   append(buf,&mut off,b"State:\tR (running)\n");
   append(buf,&mut off,b"VmSize:\t0 kB\n");
  }
  "/proc/self/stat"=>{
   append_num(buf,&mut off,pid as u64); append(buf,&mut off,b" (init) R 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n");
  }
  "/proc/self/maps"=>{ append(buf,&mut off,b""); }
  "/proc/cpuinfo"=>{ append(buf,&mut off,b"processor\t: 0\nvendor_id\t: GenuineIntel\ncpu family\t: 6\nmodel name\t: Slop/MNU virtual CPU\n"); }
  "/proc/meminfo"=>{ append(buf,&mut off,b"MemTotal:       262144 kB\nMemFree:        262144 kB\n"); }
  "/proc/uptime"=>{ append(buf,&mut off,b"1.00 1.00\n"); }
  "/proc/version"=>{ append(buf,&mut off,b"Linux version 6.6.0-slop (slop@build) \n"); }
  "/proc/loadavg"=>{ append(buf,&mut off,b"0.00 0.00 0.00 1/1 1\n"); }
  _=>{}
 }
 off
}
fn proc_lookup(path:&str)->Result<Inode>{
 if path=="/proc"{
  let mut name=[0u8;NAME_MAX+1]; name[..5].copy_from_slice(b"/proc"); return Ok(Inode{ino:0x9000,mode:S_IFDIR|0o555,name,name_len:5,size:0,capacity:0,data:core::ptr::null_mut(),rdev:0,pages:[0;8],npages:0})
 }
 if path=="/proc/self"{
  let mut name=[0u8;NAME_MAX+1]; let b=path.as_bytes(); name[..b.len()].copy_from_slice(b); return Ok(Inode{ino:0x9001,mode:S_IFDIR|0o555,name,name_len:b.len(),size:0,capacity:0,data:core::ptr::null_mut(),rdev:0,pages:[0;8],npages:0})
 }
 if path=="/proc/self/fd"{
  let mut name=[0u8;NAME_MAX+1]; let b=path.as_bytes(); name[..b.len()].copy_from_slice(b); return Ok(Inode{ino:0x9002,mode:S_IFDIR|0o555,name,name_len:b.len(),size:0,capacity:0,data:core::ptr::null_mut(),rdev:0,pages:[0;8],npages:0})
 }
 if path=="/proc/self/ns"{
  let mut name=[0u8;NAME_MAX+1]; let b=path.as_bytes(); name[..b.len()].copy_from_slice(b); return Ok(Inode{ino:0x9003,mode:S_IFDIR|0o555,name,name_len:b.len(),size:0,capacity:0,data:core::ptr::null_mut(),rdev:0,pages:[0;8],npages:0})
 }
 if is_proc_file(path){ return proc_file_lookup(path) }
 if path=="/proc/self/exe"{
  let exe=crate::task::with_current_process(|pr|(pr.exe_path,pr.exe_len)).unwrap_or(([0;64],0));
  return proc_symlink(path,&exe.0[..exe.1])
 }
 if path=="/proc/self/root" || path=="/proc/self/cwd"{
  let (arr,len)=if path.ends_with("root"){crate::task::with_current_process(|pr|(pr.root,pr.root_len)).unwrap_or(([b'/' ;128],1))}else{crate::task::with_current_process(|pr|(pr.cwd,pr.cwd_len)).unwrap_or(([b'/' ;128],1))};
  return proc_symlink(path,&arr[..len])
 }
 if let Some(rest)=path.strip_prefix("/proc/self/ns/"){
  let (uts,ipc,mnt,pid,net,user,cgroup,time)=crate::task::with_current_process(|pr|(pr.ns_uts,pr.ns_ipc,pr.ns_mnt,pr.ns_pid,pr.ns_net,pr.ns_user,pr.ns_cgroup,pr.ns_time)).unwrap_or((1,1,1,1,1,1,1,1));
  let id=match rest{"uts"=>uts,"ipc"=>ipc,"mnt"=>mnt,"pid"=>pid,"net"=>net,"user"=>user,"cgroup"=>cgroup,"time"=>time,_=>return Err(errno::ENOENT)};
  let prefix: &[u8]=match rest{"uts"=>b"uts:[", "ipc"=>b"ipc:[", "mnt"=>b"mnt:[", "pid"=>b"pid:[", "net"=>b"net:[", "user"=>b"user:[", "cgroup"=>b"cgroup:[", "time"=>b"time:[", _=>b"ns:["};
  let mut buf=[0u8;32]; let mut off=0usize; buf[off..off+prefix.len()].copy_from_slice(prefix); off+=prefix.len();
  let mut tmp=[0u8;20]; let mut i=0; let mut v=id;
  if v==0{tmp[0]=b'0';i=1}else{while v>0{tmp[i]=b'0'+(v%10)as u8;v/=10;i+=1}for j in 0..i/2{tmp.swap(j,i-1-j)}}
  buf[off..off+i].copy_from_slice(&tmp[..i]); off+=i; buf[off]=b']'; off+=1;
  return proc_symlink(path,&buf[..off])
 }
 Err(errno::ENOENT)
}

pub fn symlink(target:&str,path:&str)->Result<Inode>{
 if !path.starts_with('/')||path.as_bytes().contains(&0){return Err(errno::EINVAL)}
 let mut m=ROOT.lock();
 if m.len>=MAX_NODES{return Err(errno::EMFILE)}
 if m.nodes[..m.len].iter().any(|n|&n.name[..n.name_len]==path.as_bytes()){return Err(errno::EEXIST)}
 let tb=target.as_bytes();
 let frame=match crate::memory::alloc_frame(){Some(f)=>f,None=>return Err(errno::ENOMEM)};
 unsafe{crate::arch::x86_64::phys::Identity.table(frame).fill(0)}
 if tb.len()>4095{return Err(errno::ENAMETOOLONG)}
 unsafe{core::slice::from_raw_parts_mut(frame as*mut u8,tb.len()).copy_from_slice(tb)}
 let ino=(m.len+2)as u64 + 0x1000;
 let mut name=[0u8;NAME_MAX+1]; let nl=path.len(); name[..nl].copy_from_slice(path.as_bytes());
 let node=Inode{ino,mode:S_IFLNK|0o777,size:tb.len() as u64,capacity:tb.len() as u64,data:frame as*mut u8,rdev:0,pages:[frame,0,0,0,0,0,0,0],npages:1,name,name_len:nl};
 let idx=m.len; m.nodes[idx]=node; m.len+=1;
 Ok(node)
}
pub fn link(old:&str,new:&str)->Result<Inode>{
 if !old.starts_with('/')||!new.starts_with('/')||new.as_bytes().contains(&0){return Err(errno::EINVAL)}
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

pub fn lookup(path:&str)->Result<Inode>{
 if !path.starts_with('/')||path.as_bytes().contains(&0){return Err(errno::ENOENT)}
 if path=="/"{let mut name=[0u8;NAME_MAX+1];name[0]=b'/';return Ok(Inode{ino:1,mode:S_IFDIR|0o755,size:0,capacity:0,data:core::ptr::null_mut(),rdev:0,pages:[0;8],npages:0,name,name_len:1})}
 if path=="/proc"||path=="/proc/self"||path=="/proc/self/fd"||path=="/proc/self/ns"||is_proc_file(path)||is_proc_link(path){
  return proc_lookup(path)
 }
 if path=="/sys"||path=="/sys/class"||path=="/sys/class/net"||path=="/sys/block"||path=="/sys/devices"||path=="/sys/kernel"||path=="/sys/fs"||is_sys_file(path){
  return sys_lookup(path)
 }
 if path=="/mnt"{
  let mut name=[0u8;NAME_MAX+1]; name[..4].copy_from_slice(b"/mnt"); return Ok(Inode{ino:0xb000,mode:S_IFDIR|0o755,size:0,capacity:0,data:core::ptr::null_mut(),rdev:0,pages:[0;8],npages:0,name,name_len:4})
 }
 if path.starts_with("/mnt/") && crate::ext2::ready(){
  let n=crate::ext2::lookup(&path[5..]);
  return n
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
 let m=ROOT.lock();
 let node=m.nodes[..m.len].iter().find(|n|n.ino==inode.ino).ok_or(errno::ENOENT)?;
 if node.npages==0 { if off>=node.size{return Ok(0)} let n=core::cmp::min(dst.len(),(node.size-off)as usize); let src=unsafe{core::slice::from_raw_parts(node.data.add(off as usize),n)}; dst[..n].copy_from_slice(src); return Ok(n) }
 if off>=node.size{return Ok(0)}
 let mut done=0usize;
 while done<dst.len() && off as usize+done < node.size as usize {
  let poff=off as usize+done;
  let pi=poff/4096; let po=poff%4096;
  if pi>=node.npages as usize{return Ok(done)}
  let frame=node.pages[pi];
  let n=core::cmp::min(dst.len()-done, core::cmp::min(4096-po, node.size as usize-poff));
  if n==0{break}
  let src=unsafe{core::slice::from_raw_parts((frame as *const u8).add(po),n)};
  dst[done..done+n].copy_from_slice(src);
  done+=n;
 }
 Ok(done)
}
pub fn write_off(inode:Inode,off:u64,src:&[u8])->Result<usize>{
 if is_proc_file_inode(inode.ino)||is_sys_file_inode(inode.ino){return Err(errno::EROFS)}
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
 Ok(src.len())
}

pub fn mknod(path:&str,mode:u32,rdev:u64)->Result<Inode>{
 if !path.starts_with('/')||path.as_bytes().contains(&0){return Err(errno::EINVAL)}
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

fn proc_file_lookup(path:&str)->Result<Inode>{
 if is_proc_file(path){
  let size=proc_fill(path);
  let mut name=[0u8;NAME_MAX+1]; let b=path.as_bytes(); name[..b.len()].copy_from_slice(b);
  let ino=0x90000 + (path.len() as u64);
  return Ok(Inode{ino,mode:S_IFREG|0o444,name,name_len:b.len(),size:size as u64,capacity:4096,data:unsafe{(&raw mut PROC_BUF).cast::<u8>()},rdev:0,pages:[0;8],npages:0})
 }
 Err(errno::ENOENT)
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