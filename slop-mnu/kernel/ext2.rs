//! Minimal read-only ext2 filesystem on the first virtio-blk device.
use crate::{errno, vfs};
const EXT2_MAGIC:u16=0xEF53;
pub fn ready()->bool{ crate::driver::virtio_blk::ready() }
fn read_bytes(off:u64,dst:&mut[u8])->Result<(),i64>{
 let cap=crate::driver::virtio_blk::capacity()*512;
 if off as usize+dst.len() > cap as usize {return Err(errno::EINVAL)}
 let mut done=0usize;
 while done<dst.len(){
  let sec=(off as usize+done)/512;
  let soff=(off as usize+done)%512;
  let n=core::cmp::min(dst.len()-done,512-soff);
  let mut buf=[0u8;512];
  // read sector
  let mut frame=[0u8;512];
  // virtio_blk read_sector needs 512 dst
  if crate::driver::virtio_blk::read_sector(sec as u64,&mut frame).is_err(){return Err(errno::EIO)}
  dst[done..done+n].copy_from_slice(&frame[soff..soff+n]);
  done+=n;
 }
 Ok(())
}
fn read_block(block:u64,blksize:usize,buf:&mut [u8])->Result<(),i64>{
 if buf.len()<blksize{return Err(errno::EINVAL)}
 let off=block*blksize as u64;
 read_bytes(off,&mut buf[..blksize])
}
#[repr(C)] struct Super{ s_inodes_count:u32, s_blocks_count:u32, _r0:u32, _r1:u32, _r2:u32, s_first_data_block:u32, s_log_block_size:u32, _r3:u32, s_blocks_per_group:u32, _r4:u32, s_inodes_per_group:u32, _r5:u32, _r6:u32, _r7:u16, _r8:u16, s_magic:u16, _r9:[u8;30], s_inode_size:u16, }
#[repr(C)] struct GroupDesc{ bg_block_bitmap:u32, bg_inode_bitmap:u32, bg_inode_table:u32, bg_free_blocks_count:u16, bg_free_inodes_count:u16, bg_used_dirs_count:u16, bg_pad:u16, _r:[u8;12], }
#[repr(C)] struct Inode{ i_mode:u16, i_uid:u16, i_size_lo:u32, i_atime:u32, i_ctime:u32, i_mtime:u32, i_dtime:u32, i_gid:u16, i_links_count:u16, i_blocks:u32, i_flags:u32, i_osd1:u32, i_block:[u32;15], i_generation:u32, i_file_acl:u32, i_dir_acl:u32, i_faddr:u32, i_osd2:[u8;12], }
pub fn superblock()->Result<Super,i64>{
 let mut buf=[0u8;1024];
 read_bytes(1024,&mut buf)?;
 let sb:Super=unsafe{core::ptr::read(buf.as_ptr() as *const Super)};
 if sb.s_magic != EXT2_MAGIC {return Err(errno::EINVAL)}
 Ok(sb)
}
fn block_size(sb:&Super)->usize{1024usize<<(sb.s_log_block_size as usize)}
fn groups_count(sb:&Super)->usize{ (sb.s_blocks_count as usize + sb.s_blocks_per_group as usize -1)/sb.s_blocks_per_group as usize }
fn group_desc_block(sb:&Super)->u64{ if block_size(sb)==1024 {2} else {1} }
fn read_group_desc(sb:&Super,g:usize)->Result<GroupDesc,i64>{
 let bs=block_size(sb);
 let mut buf=[0u8;4096];
 read_block(group_desc_block(sb),bs,&mut buf)?;
 let off=g*32;
 let gd:GroupDesc=unsafe{core::ptr::read(buf[off..].as_ptr() as *const GroupDesc)};
 Ok(gd)
}
fn read_inode(sb:&Super,inum:u32)->Result<Inode,i64>{
 let g=((inum-1) as usize)/sb.s_inodes_per_group as usize;
 let idx=(inum-1) as usize % sb.s_inodes_per_group as usize;
 let gd=read_group_desc(sb,g)?;
 let bs=block_size(sb);
 let inode_size=sb.s_inode_size as usize;
 let table_block=gd.bg_inode_table as u64;
 let byte_off=idx*inode_size;
 let block=table_block + (byte_off/bs) as u64;
 let off=byte_off%bs;
 let mut buf=[0u8;4096];
 read_block(block,bs,&mut buf)?;
 let inode:Inode=unsafe{core::ptr::read(buf[off..].as_ptr() as *const Inode)};
 Ok(inode)
}
fn read_file_block(sb:&Super,inode:&Inode,block_index:usize)->Result<u64,i64>{
 let bs=block_size(sb);
 if block_index < 12 { return Ok(inode.i_block[block_index] as u64); }
 // single indirect
 let mut idx=block_index-12;
 if idx < (bs/4) {
  let ib=inode.i_block[12] as u64;
  if ib==0 {return Ok(0)}
  let mut buf=[0u8;4096];
  read_block(ib,bs,&mut buf)?;
  let b=u32::from_le_bytes(buf[idx*4..idx*4+4].try_into().unwrap());
  return Ok(b as u64);
 }
 idx -= bs/4;
 if idx < (bs/4)*(bs/4) {
  let dib=inode.i_block[13] as u64;
  if dib==0 {return Ok(0)}
  let mut buf=[0u8;4096];
  read_block(dib,bs,&mut buf)?;
  let i1=(idx/(bs/4)) as usize;
  let ib=u32::from_le_bytes(buf[i1*4..i1*4+4].try_into().unwrap()) as u64;
  if ib==0 {return Ok(0)}
  let mut buf2=[0u8;4096];
  read_block(ib,bs,&mut buf2)?;
  let i2=idx%(bs/4);
  let b=u32::from_le_bytes(buf2[i2*4..i2*4+4].try_into().unwrap());
  return Ok(b as u64);
 }
 Err(errno::EINVAL)
}
pub fn lookup(path:&str)->Result<vfs::Inode,i64>{
 if !ready(){return Err(errno::ENODEV)}
 let sb=superblock()?;
 let mut inum=2u32;
 let mut inode=read_inode(&sb,inum)?;
 let trimmed=path.trim_start_matches('/').trim_end_matches('/');
 if trimmed.is_empty(){return Ok(make_vfs_inode("/mnt",inum,&inode,&sb))}
 for comp in trimmed.split('/') {
  if comp.is_empty() || comp=="." {continue}
  if comp==".." { // not fully supported
   inum=2; inode=read_inode(&sb,inum)?; continue
  }
  // read directory
  let size=inode.i_size_lo as usize;
  let mut off=0usize;
  let mut found=None;
   while off < size {
   let mut buf=[0u8;256];
   // read at least 8 bytes
   let blk=read_file_block(&sb,&inode,off/ (block_size(&sb)))?;
   if blk==0 {break}
   let bs=block_size(&sb);
   let mut block=[0u8;4096];
   read_block(blk,bs,&mut block)?;
   let boff=off%bs;
   let ino=u32::from_le_bytes(block[boff..boff+4].try_into().unwrap());
   let rec_len=u16::from_le_bytes(block[boff+4..boff+6].try_into().unwrap()) as usize;
   let name_len=block[boff+6] as usize;
   if rec_len==0 || boff+rec_len>bs {break}
   if ino!=0 && name_len==comp.len() && &block[boff+8..boff+8+name_len]==comp.as_bytes(){
    found=Some(ino); break;
   }
   off += rec_len;
  }
  let ino=found.ok_or(errno::ENOENT)?;
   inode=read_inode(&sb,ino)?;
   // skip re-read below
  inum=ino;
  inode=read_inode(&sb,inum)?;
  }
let n=make_vfs_inode(path,inum,&inode,&sb); Ok(n)
}
fn make_vfs_inode(path:&str,inum:u32,inode:&Inode,sb:&Super)->vfs::Inode{
 let mode=inode.i_mode as u32;
 let size=inode.i_size_lo as u64;
 let mut name=[0u8;127+1];
 let b=path.as_bytes(); let nl=b.len().min(127); name[..nl].copy_from_slice(&b[..nl]);
 vfs::Inode{ino: (inum as u64)+0x100000, mode: if mode & 0o170000==0 {vfs::S_IFREG| (mode&0o777)} else {mode}, size, capacity:size, data:core::ptr::null_mut(), rdev:inum as u64, pages:[0;8], npages:0, name, name_len:nl}
}
pub fn read(inode_rdev:u32,off:u64,dst:&mut[u8])->Result<usize,i64>{
 let sb=superblock()?;
 let inode=read_inode(&sb,inode_rdev)?;
 let size=inode.i_size_lo as usize;
 if off as usize >= size {return Ok(0)}
 let mut done=0usize;
 while done<dst.len() && off as usize+done < size {
  let poff=off as usize+done;
  let blk=read_file_block(&sb,&inode,poff/ (block_size(&sb) as usize))?;
  if blk==0 {break}
  let bs=block_size(&sb);
  let mut block=[0u8;4096];
  read_block(blk,bs,&mut block)?;
  let po=poff % bs;
  let n=core::cmp::min(dst.len()-done, core::cmp::min(bs-po, size-poff));
  if n==0{break}
  dst[done..done+n].copy_from_slice(&block[po..po+n]);
  done+=n;
 }
 Ok(done)
}
pub fn is_dir(path:&str)->bool{
 if path=="/mnt" {return true}
 if let Ok(inode)=lookup(path){ inode.mode & 0o040000 !=0 } else {false}
}
pub fn readdir(path:&str,mut cb:impl FnMut(u64,u32,&str))->Result<usize,i64>{
 // path is ext2 path without /mnt
 let inum=if path=="/" || path.is_empty() {2} else {
  // lookup parent? For simplicity only root listing supported
  2
 };
 let sb=superblock()?;
 let inode=read_inode(&sb,inum)?;
 let size=inode.i_size_lo as usize;
 let mut off=0usize; let mut count=0;
 let bs=block_size(&sb);
 while off < size {
  let blk=read_file_block(&sb,&inode,off/bs)?;
  if blk==0 {break}
  let mut block=[0u8;4096];
  read_block(blk,bs,&mut block)?;
  let boff=off%bs;
  let ino=u32::from_le_bytes(block[boff..boff+4].try_into().unwrap());
  let rec_len=u16::from_le_bytes(block[boff+4..boff+6].try_into().unwrap()) as usize;
  let name_len=block[boff+6] as usize;
  let ftype=block[boff+7];
  if rec_len==0 || boff+rec_len>bs {break}
  if ino!=0 && name_len>0 {
   let name=core::str::from_utf8(&block[boff+8..boff+8+name_len]).unwrap_or("");
   if name!="." && name!=".." {
    let mode=if ftype==2 {vfs::S_IFDIR} else {vfs::S_IFREG};
    cb((ino as u64)+0x100000, mode, name);
    count+=1;
   }
  }
  off += rec_len;
 }
 Ok(count)
}