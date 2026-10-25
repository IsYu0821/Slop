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
fn write_bytes(off:u64,src:&[u8])->Result<(),i64>{
 let cap=crate::driver::virtio_blk::capacity()*512;
 if off as usize+src.len() > cap as usize {return Err(errno::EINVAL)}
 let mut done=0usize;
 while done<src.len(){
  let sec=(off as usize+done)/512;
  let soff=(off as usize+done)%512;
  let n=core::cmp::min(src.len()-done,512-soff);
  let mut frame=[0u8;512];
  crate::driver::virtio_blk::read_sector(sec as u64,&mut frame)?;
  frame[soff..soff+n].copy_from_slice(&src[done..done+n]);
  crate::driver::virtio_blk::write_sector(sec as u64,&frame).map_err(|_|errno::EIO)?;
  done+=n;
 }
 Ok(())
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
 vfs::Inode{ino: (inum as u64)+0x100000, mode: if mode & 0o170000==0 {vfs::S_IFREG| (mode&0o777)} else {mode}, size, capacity:size, indirect:0, dindirect:0, data:core::ptr::null_mut(), rdev:inum as u64, pages:[0;8], npages:0, name, name_len:nl}
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
  let bs=block_size(&sb);
  let po=poff % bs;
  let n=core::cmp::min(dst.len()-done, core::cmp::min(bs-po, size-poff));
  if n==0{break}
  if blk==0 {
   /* Sparse holes read back as zeros. */
   dst[done..done+n].fill(0);
   done+=n;
   continue;
  }
  let mut block=[0u8;4096];
  read_block(blk,bs,&mut block)?;
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
  // resolve the subdirectory instead of always listing the root
 let t=path.trim_start_matches('/').trim_end_matches('/');
 if t.is_empty(){2}else{lookup(t)?.rdev as u32}
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

/* ---------- writable support: block/inode allocation ---------- */

fn write_block(block:u64,blksize:usize,buf:&[u8])->Result<(),i64>{
 if buf.len()<blksize{return Err(errno::EINVAL)}
 let mut off=(block as usize)*blksize;
 let mut done=0usize;
 while done<blksize{
  let mut sec=[0u8;512];
  let n=core::cmp::min(512,blksize-done);
  sec[..n].copy_from_slice(&buf[done..done+n]);
  for s in 0..(n+511)/512{
   let mut full=[0u8;512];
   let nn=core::cmp::min(512,n-s*512);
   if nn==0{break}
   if n<512{
    /* Read-modify-write the tail sector so neighboring bytes survive. */
    crate::driver::virtio_blk::read_sector((off/512) as u64,&mut full)?;
   }
   full[..nn].copy_from_slice(&sec[s*512..s*512+nn]);
   crate::driver::virtio_blk::write_sector((off/512) as u64,&full)?;
   off+=512;
  }
  done+=n;
 }
 Ok(())
}
/* Patches little-endian fields of the 1024-byte superblock sector. */
fn patch_super(f:impl FnOnce(&mut [u8;1024]))->Result<(),i64>{
 let mut buf=[0u8;1024];
 read_bytes(1024,&mut buf)?;
 f(&mut buf);
 write_bytes(1024,&buf)
}
fn group_start(sb:&Super,g:usize)->u64{(g as u64)*sb.s_blocks_per_group as u64}
/* Bitmap position of an absolute block number. Block 0 (the boot sector of
   a 1 KiB-block filesystem) lives outside every bitmap: bit N of group G
   tracks block G*per_group + N + (G == 0 ? s_first_data_block : 0), so the
   native tools, the guest and fsck agree on which block a bit means. */
fn block_bit(sb:&Super,g:usize,abs:u64)->Option<usize>{
 let adjust=if g==0{sb.s_first_data_block as u64}else{0};
 let bit=abs.checked_sub(group_start(sb,g)+adjust)?;
 if bit as usize>=sb.s_blocks_per_group as usize{return None}
 Some(bit as usize)
}
/* Allocates one zeroed data block. Scans group bitmaps top-down so fresh
   files land away from the low metadata area: mke2fs leaves the resize
   inode's top indirect block unmarked in the bitmap, and native Linux
   likewise allocates high (goal heuristics), so bottom-up scanning would
   trample a reservation that e2fsck still validates structurally. */
fn alloc_block(sb:&Super)->Result<u64,i64>{
 let ng=groups_count(sb);
 let bs=block_size(sb);
 for g in (0..ng).rev(){
  let gd=read_group_desc(sb,g)?;
  if gd.bg_free_blocks_count==0{continue}
  let mut bmp=[0u8;4096];
  read_block(gd.bg_block_bitmap as u64,bs,&mut bmp)?;
  let bits=core::cmp::min(sb.s_blocks_per_group as usize,bs*8);
  let mut bit=bits;
  while bit>0{
   bit-=1;
   if bmp[bit/8]&(1<<(bit%8))!=0{continue}
   let abs=group_start(sb,g)+(if g==0{sb.s_first_data_block as u64}else{0})+bit as u64;
   if abs>=sb.s_blocks_count as u64||abs==0{continue}
   bmp[bit/8]|=1<<(bit%8);
   crate::println!("ext2alloc blk={} bit={}",group_start(sb,g)+(if g==0{sb.s_first_data_block as u64}else{0})+bit as u64,bit);
   write_block(gd.bg_block_bitmap as u64,bs,&bmp)?;
   /* Zero the fresh block so holes and new tails read back zeros. */
   let zero=[0u8;4096];
   write_block(abs,bs,&zero)?;
   /* Write back the free-block counters. */
   let mut gbuf=[0u8;4096];
   read_block(group_desc_block(sb),bs,&mut gbuf)?;
   let eoff=g*32;
   let free=u16::from_le_bytes(gbuf[eoff+12..eoff+14].try_into().unwrap());
   gbuf[eoff+12..eoff+14].copy_from_slice(&free.saturating_sub(1).to_le_bytes());
   write_block(group_desc_block(sb),bs,&gbuf)?;
   patch_super(|s|{
    let f=u32::from_le_bytes(s[12..16].try_into().unwrap());
    s[12..16].copy_from_slice(&f.saturating_sub(1).to_le_bytes());
    let t=crate::time::realtime_ns()/1_000_000_000;
    s[48..52].copy_from_slice(&(t as u32).to_le_bytes());
   })?;
   return Ok(abs);
  }
 }
 Err(errno::ENOSPC)
}
fn free_block(sb:&Super,abs:u64)->Result<(),i64>{
 let bs=block_size(sb);
 let g=(abs/sb.s_blocks_per_group as u64) as usize;
 if g>=groups_count(sb){return Err(errno::EINVAL)}
 let gd=read_group_desc(sb,g)?;
 let bit=match block_bit(sb,g,abs){Some(b)=>b,None=>return Err(errno::EINVAL)};
 crate::println!("ext2free blk={} bit={}",abs,bit);
 let mut bmp=[0u8;4096];
 read_block(gd.bg_block_bitmap as u64,bs,&mut bmp)?;
 bmp[bit/8]&=!(1<<(bit%8));
 write_block(gd.bg_block_bitmap as u64,bs,&bmp)?;
 let mut gbuf=[0u8;4096];
 read_block(group_desc_block(sb),bs,&mut gbuf)?;
 let eoff=g*32;
 let free=u16::from_le_bytes(gbuf[eoff+12..eoff+14].try_into().unwrap());
 gbuf[eoff+12..eoff+14].copy_from_slice(&free.saturating_add(1).to_le_bytes());
 write_block(group_desc_block(sb),bs,&gbuf)?;
 patch_super(|s|{
  let f=u32::from_le_bytes(s[12..16].try_into().unwrap());
  s[12..16].copy_from_slice(&f.saturating_add(1).to_le_bytes());
 })?;
 Ok(())
}
fn alloc_inode(sb:&Super,is_dir:bool)->Result<u32,i64>{
 let ng=groups_count(sb);
 for g in 0..ng{
  let gd=read_group_desc(sb,g)?;
  if gd.bg_free_inodes_count==0{continue}
  let mut bmp=[0u8;4096];
  read_block(gd.bg_inode_bitmap as u64,block_size(sb),&mut bmp)?;
  let bits=core::cmp::min(sb.s_inodes_per_group as usize,block_size(sb)*8);
  for bit in 0..bits{
   if bmp[bit/8]&(1<<(bit%8))!=0{continue}
   bmp[bit/8]|=1<<(bit%8);
   write_block(gd.bg_inode_bitmap as u64,block_size(sb),&bmp)?;
   let mut gbuf=[0u8;4096];
   read_block(group_desc_block(sb),block_size(sb),&mut gbuf)?;
   let eoff=g*32;
   let free=u16::from_le_bytes(gbuf[eoff+14..eoff+16].try_into().unwrap());
   gbuf[eoff+14..eoff+16].copy_from_slice(&free.saturating_sub(1).to_le_bytes());
   if is_dir{
    let used=u16::from_le_bytes(gbuf[eoff+16..eoff+18].try_into().unwrap());
    gbuf[eoff+16..eoff+18].copy_from_slice(&used.saturating_add(1).to_le_bytes());
   }
   write_block(group_desc_block(sb),block_size(sb),&gbuf)?;
   patch_super(|s|{
    let f=u32::from_le_bytes(s[16..20].try_into().unwrap());
    s[16..20].copy_from_slice(&f.saturating_sub(1).to_le_bytes());
   })?;
   return Ok((g as u32)*sb.s_inodes_per_group+bit as u32+1);
  }
 }
 Err(errno::ENOSPC)
}
/* Frees an inode number: clears the bitmap bit, refunds the counts, and
   drops the link count with a deletion timestamp (native Linux leaves the
   mode behind on deleted inodes). */
fn free_inode(sb:&Super,inum:u32,is_dir:bool)->Result<(),i64>{
 let t=(crate::time::realtime_ns()/1_000_000_000) as u32;
 patch_inode_dtime(inum,t)?;
 patch_inode(inum,None,None,Some(0),None,false,None)?;
 let g=((inum-1) as usize)/sb.s_inodes_per_group as usize;
 let idx=(inum-1) as usize % sb.s_inodes_per_group as usize;
 let gd=read_group_desc(sb,g)?;
 let bs=block_size(sb);
 let mut bmp=[0u8;4096];
 read_block(gd.bg_inode_bitmap as u64,bs,&mut bmp)?;
 bmp[idx/8]&=!(1<<(idx%8));
 write_block(gd.bg_inode_bitmap as u64,bs,&bmp)?;
 let mut gbuf=[0u8;4096];
 read_block(group_desc_block(sb),bs,&mut gbuf)?;
 let eoff=g*32;
 let free=u16::from_le_bytes(gbuf[eoff+14..eoff+16].try_into().unwrap());
 gbuf[eoff+14..eoff+16].copy_from_slice(&free.saturating_add(1).to_le_bytes());
 if is_dir{
  let used=u16::from_le_bytes(gbuf[eoff+16..eoff+18].try_into().unwrap());
  gbuf[eoff+16..eoff+18].copy_from_slice(&used.saturating_sub(1).to_le_bytes());
 }
 write_block(group_desc_block(sb),bs,&gbuf)?;
 patch_super(|s|{
  let f=u32::from_le_bytes(s[16..20].try_into().unwrap());
  s[16..20].copy_from_slice(&f.saturating_add(1).to_le_bytes());
 })?;
 Ok(())
}
/* Reads one raw inode-table entry into a caller buffer. */
fn read_inode_raw(sb:&Super,inum:u32,entry:&mut [u8])->Result<(),i64>{
 let g=((inum-1) as usize)/sb.s_inodes_per_group as usize;
 let idx=(inum-1) as usize % sb.s_inodes_per_group as usize;
 let gd=read_group_desc(sb,g)?;
 let bs=block_size(sb);
 let isize=sb.s_inode_size as usize;
 if entry.len()<isize{return Err(errno::EINVAL)}
 let byte_off=idx*isize;
 let block=gd.bg_inode_table as u64+(byte_off/bs) as u64;
 let off=byte_off%bs;
 let mut buf=[0u8;4096];
 read_block(block,bs,&mut buf)?;
 entry[..isize].copy_from_slice(&buf[off..off+isize]);
 Ok(())
}
fn write_inode_raw(sb:&Super,inum:u32,entry:&[u8])->Result<(),i64>{
 let g=((inum-1) as usize)/sb.s_inodes_per_group as usize;
 let idx=(inum-1) as usize % sb.s_inodes_per_group as usize;
 let gd=read_group_desc(sb,g)?;
 let bs=block_size(sb);
 let isize=sb.s_inode_size as usize;
 if entry.len()<isize{return Err(errno::EINVAL)}
 let byte_off=idx*isize;
 let block=gd.bg_inode_table as u64+(byte_off/bs) as u64;
 let off=byte_off%bs;
 let mut buf=[0u8;4096];
 read_block(block,bs,&mut buf)?;
 buf[off..off+isize].copy_from_slice(&entry[..isize]);
 write_block(block,bs,&buf)
}
/* Applies little-endian field patches to an inode-table entry. Offsets
   follow the on-disk layout (mode@0, size@4, mtime@16, links@26,
   blocks@28, block[]@40); the tail of a larger entry is preserved. */
fn patch_inode(inum:u32,mode:Option<u16>,size:Option<u32>,links:Option<u16>,blocks:Option<u32>,stamp:bool,blk:Option<([u32;15])>)->Result<(),i64>{
 let sb=superblock()?;
 let isize=sb.s_inode_size as usize;
 let mut e=[0u8;512];
 if isize>e.len(){return Err(errno::EINVAL)}
 read_inode_raw(&sb,inum,&mut e)?;
 if let Some(m)=mode{e[0..2].copy_from_slice(&m.to_le_bytes());}
 if let Some(s)=size{e[4..8].copy_from_slice(&s.to_le_bytes());}
 if stamp{let t=(crate::time::realtime_ns()/1_000_000_000) as u32;e[12..16].copy_from_slice(&t.to_le_bytes());e[16..20].copy_from_slice(&t.to_le_bytes());}
 if let Some(l)=links{e[26..28].copy_from_slice(&l.to_le_bytes());}
 if let Some(b)=blocks{e[28..32].copy_from_slice(&b.to_le_bytes());}
 if let Some(arr)=blk{for i in 0..15{e[40+i*4..44+i*4].copy_from_slice(&arr[i].to_le_bytes());}}
 write_inode_raw(&sb,inum,&e[..isize])
}

/* patch_inode plus an explicit deletion timestamp (dtime lives at offset
   20 and has no slot in the common helper). */
fn patch_inode_dtime(inum:u32,dtime:u32)->Result<(),i64>{
 let sb=superblock()?;
 let isize=sb.s_inode_size as usize;
 let mut e=[0u8;512];
 if isize>e.len(){return Err(errno::EINVAL)}
 read_inode_raw(&sb,inum,&mut e)?;
 e[20..24].copy_from_slice(&dtime.to_le_bytes());
 write_inode_raw(&sb,inum,&e[..isize])
}
fn zero_inode(inum:u32)->Result<(),i64>{
 let sb=superblock()?;
 let isize=sb.s_inode_size as usize;
 let z=[0u8;512];
 if isize>z.len(){return Err(errno::EINVAL)}
 write_inode_raw(&sb,inum,&z[..isize])
}
/* Returns the disk block for file block `idx`, allocating data and indirect
   blocks when asked. `blks` is the caller's working copy of i_block[] and is
   updated in place; the caller writes it back with patch_inode. Every fresh
   block (data or indirect) bumps `new_blocks` for i_blocks accounting. */
fn file_block(sb:&Super,blks:&mut [u32;15],idx:usize,alloc:bool,new_blocks:&mut u32)->Result<u64,i64>{
 let bs=block_size(sb);
 if idx<12{
  if blks[idx]!=0{return Ok(blks[idx] as u64)}
  if !alloc{return Ok(0)}
  let nb=alloc_block(sb)?;
  *new_blocks+=1;
  blks[idx]=nb as u32;
  return Ok(nb);
 }
 let per=bs/4;
 let mut idx2=idx-12;
 if idx2<per{
  if blks[12]==0{
   if !alloc{return Ok(0)}
   let ib=alloc_block(sb)?;
   *new_blocks+=1;
   blks[12]=ib as u32;
   let z=[0u8;4096];
   write_block(ib,bs,&z)?;
  }
  let ib=blks[12] as u64;
  let mut buf=[0u8;4096];
  read_block(ib,bs,&mut buf)?;
  let cur=u32::from_le_bytes(buf[idx2*4..idx2*4+4].try_into().unwrap()) as u64;
  if cur!=0{return Ok(cur)}
  if !alloc{return Ok(0)}
  let nb=alloc_block(sb)?;
  *new_blocks+=1;
  buf[idx2*4..idx2*4+4].copy_from_slice(&(nb as u32).to_le_bytes());
  write_block(ib,bs,&buf)?;
  return Ok(nb);
 }
 idx2-=per;
 let per2=per*per;
 if idx2>=per2{return Err(errno::EFBIG)}
 if blks[13]==0{
  if !alloc{return Ok(0)}
  let dib=alloc_block(sb)?;
  *new_blocks+=1;
  blks[13]=dib as u32;
  let z=[0u8;4096];
  write_block(dib,bs,&z)?;
 }
 let dib=blks[13] as u64;
 let i1=idx2/per;
 let mut buf=[0u8;4096];
 read_block(dib,bs,&mut buf)?;
 let mut ib=u32::from_le_bytes(buf[i1*4..i1*4+4].try_into().unwrap()) as u64;
 if ib==0{
  if !alloc{return Ok(0)}
  ib=alloc_block(sb)?;
  *new_blocks+=1;
  buf[i1*4..i1*4+4].copy_from_slice(&(ib as u32).to_le_bytes());
  write_block(dib,bs,&buf)?;
  let z=[0u8;4096];
  write_block(ib,bs,&z)?;
 }
 let mut buf2=[0u8;4096];
 read_block(ib,bs,&mut buf2)?;
 let i2=idx2%per;
 let cur=u32::from_le_bytes(buf2[i2*4..i2*4+4].try_into().unwrap()) as u64;
 if cur!=0{return Ok(cur)}
 if !alloc{return Ok(0)}
 let nb=alloc_block(sb)?;
 *new_blocks+=1;
 buf2[i2*4..i2*4+4].copy_from_slice(&(nb as u32).to_le_bytes());
 write_block(ib,bs,&buf2)?;
 Ok(nb)
}fn entry_len(name_len:usize)->usize{(8+name_len+3)&!3}
/* Inserts (inum, name) into a directory, allocating a block if needed.
   Reuses slack inside an entry (shrinking it to its true size) or a fully
   deleted entry, and reports EEXIST on a live name clash. */
fn dir_insert(dir_inum:u32,inum:u32,name:&[u8],ftype:u8)->Result<(),i64>{
 if name.is_empty()||name.len()>255||name.contains(&b'/'){return Err(errno::EINVAL)}
 let sb=superblock()?;
 let bs=block_size(&sb);
 let need=entry_len(name.len());
 let dir=read_inode(&sb,dir_inum)?;
 if dir.i_mode&0o170000!=0o040000{return Err(errno::ENOTDIR)}
 let mut blks=dir.i_block;
 let size=dir.i_size_lo as usize;
 let nblocks=(size+bs-1)/bs;
 for bi in 0..nblocks{
  let mut newb=0u32;
  let blk=file_block(&sb,&mut blks,bi,true,&mut newb)?;
  if blk==0{return Err(errno::ENOSPC)}
  if newb>0{
   let cur=inode_sector_count(dir_inum)?;
   patch_inode(dir_inum,None,None,None,Some(cur+newb*(bs/512) as u32),true,Some(blks))?;
  }
  let mut block=[0u8;4096];
  read_block(blk,bs,&mut block)?;
  let mut off=0usize;
  while off+8<=bs{
   let ino=u32::from_le_bytes(block[off..off+4].try_into().unwrap());
   let rec=u16::from_le_bytes(block[off+4..off+6].try_into().unwrap()) as usize;
   let nl=block[off+6] as usize;
   if rec==0||off+rec>bs{break}
   if ino!=0&&nl==name.len()&&&block[off+8..off+8+nl]==name{return Err(errno::EEXIST)}
   let actual=if ino==0{8}else{entry_len(nl)};
   if rec>=actual&&rec-actual>=need{
    let no=if ino==0{off}else{off+actual};
    let nrec=if ino==0{rec}else{rec-actual};
    if ino!=0{
     block[off+4..off+6].copy_from_slice(&(actual as u16).to_le_bytes());
    }
    block[no..no+4].copy_from_slice(&inum.to_le_bytes());
    block[no+4..no+6].copy_from_slice(&(nrec as u16).to_le_bytes());
    block[no+6]=name.len() as u8;
    block[no+7]=ftype;
    block[no+8..no+8+name.len()].copy_from_slice(name);
    write_block(blk,bs,&block)?;
    let cur=inode_sector_count(dir_inum)?;
    patch_inode(dir_inum,None,None,None,Some(cur),true,Some(blks))?;
    return Ok(())
   }
   if rec==0{break}
   off+=rec;
  }
 }
 /* No slack anywhere: append one block holding a single spanning entry. */
 let d=read_inode(&sb,dir_inum)?;
 let mut blks2=d.i_block;
 let mut newb=0u32;
 let blk=file_block(&sb,&mut blks2,nblocks,true,&mut newb)?;
 if blk==0{return Err(errno::ENOSPC)}
 let mut block=[0u8;4096];
 block[0..4].copy_from_slice(&inum.to_le_bytes());
 block[4..6].copy_from_slice(&(bs as u16).to_le_bytes());
 block[6]=name.len() as u8;
 block[7]=ftype;
 block[8..8+name.len()].copy_from_slice(name);
 write_block(blk,bs,&block)?;
 let cur=inode_sector_count(dir_inum)?;
 patch_inode(dir_inum,None,Some((size+bs) as u32),None,Some(cur+newb*(bs/512) as u32),true,Some(blks2))?;
 Ok(())
}
/* Collects the current i_blocks sector count from the inode image. */
fn inode_sector_count(inum:u32)->Result<u32,i64>{
 let sb=superblock()?;
 let inode=read_inode(&sb,inum)?;
 Ok(inode.i_blocks)
}
/* Frees every data block at file index >= keep and drops indirect blocks
   left without kept children, updating the caller-owned i_block[] copy.
   Returns the freed 512-byte sector count. */
fn free_from(sb:&Super,blks:&mut [u32;15],keep:usize)->Result<u32,i64>{
 let bs=block_size(sb);
 let per=bs/4;
 let mut freed_sectors=0u32;
 let sector_units=(bs/512) as u32;
 for idx in keep.min(12)..12{
  let b=blks[idx] as u64;
  if b!=0{free_block(sb,b)?;freed_sectors+=sector_units;blks[idx]=0;}
 }
 let ib=blks[12] as u64;
 if ib!=0{
  let mut buf=[0u8;4096];
  read_block(ib,bs,&mut buf)?;
  let mut kept=false;
  for k in 0..per{
   let b=u32::from_le_bytes(buf[k*4..k*4+4].try_into().unwrap()) as u64;
   if b==0{continue}
   if 12+k>=keep{free_block(sb,b)?;freed_sectors+=sector_units;buf[k*4..k*4+4].copy_from_slice(&0u32.to_le_bytes());}
   else{kept=true;}
  }
  if !kept&&keep<=12{free_block(sb,ib)?;freed_sectors+=sector_units;blks[12]=0;}
  else{write_block(ib,bs,&buf)?;}
 }
 let dib=blks[13] as u64;
 if dib!=0{
  let mut buf=[0u8;4096];
  read_block(dib,bs,&mut buf)?;
  let mut kept_any=false;
  for i1 in 0..per{
   let ib2=u32::from_le_bytes(buf[i1*4..i1*4+4].try_into().unwrap()) as u64;
   if ib2==0{continue}
   let mut buf2=[0u8;4096];
   read_block(ib2,bs,&mut buf2)?;
   let mut kept=false;
   for i2 in 0..per{
    let b=u32::from_le_bytes(buf2[i2*4..i2*4+4].try_into().unwrap()) as u64;
    if b==0{continue}
    if 12+per+i1*per+i2>=keep{free_block(sb,b)?;freed_sectors+=sector_units;buf2[i2*4..i2*4+4].copy_from_slice(&0u32.to_le_bytes());}
    else{kept=true;}
   }
   if !kept{free_block(sb,ib2)?;freed_sectors+=sector_units;buf[i1*4..i1*4+4].copy_from_slice(&0u32.to_le_bytes());}
   else{write_block(ib2,bs,&buf2)?;kept_any=true;}
  }
  if !kept_any{free_block(sb,dib)?;freed_sectors+=sector_units;blks[13]=0;}
  else{write_block(dib,bs,&buf)?;}
 }
 Ok(freed_sectors)
}
/* Removes a name from a directory, merging its record into the previous
   entry. Returns the removed inode number. */
fn dir_remove(dir_inum:u32,name:&[u8])->Result<u32,i64>{
 if name.is_empty()||name.len()>255||name.contains(&b'/'){return Err(errno::EINVAL)}
 let sb=superblock()?;
 let bs=block_size(&sb);
 let dir=read_inode(&sb,dir_inum)?;
 if dir.i_mode&0o170000!=0o040000{return Err(errno::ENOTDIR)}
 let size=dir.i_size_lo as usize;
 let nblocks=(size+bs-1)/bs;
 let mut blks=dir.i_block;
 for bi in 0..nblocks{
  let mut newb=0u32;
  let blk=file_block(&sb,&mut blks,bi,false,&mut newb)?;
  if blk==0{continue}
  let mut block=[0u8;4096];
  read_block(blk,bs,&mut block)?;
  let mut off=0usize;
  let mut prev:Option<usize>=None;
  while off+8<=bs{
   let ino=u32::from_le_bytes(block[off..off+4].try_into().unwrap());
   let rec=u16::from_le_bytes(block[off+4..off+6].try_into().unwrap()) as usize;
   let nl=block[off+6] as usize;
   if rec==0||off+rec>bs{break}
   if ino!=0&&nl==name.len()&&&block[off+8..off+8+nl]==name{
    if let Some(po)=prev{
     let prec=u16::from_le_bytes(block[po+4..po+6].try_into().unwrap()) as usize;
     block[po+4..po+6].copy_from_slice(&((prec+rec) as u16).to_le_bytes());
    }
    block[off..off+4].copy_from_slice(&0u32.to_le_bytes());
    write_block(blk,bs,&block)?;
    patch_inode(dir_inum,None,None,None,None,true,None)?;
    return Ok(ino);
   }
   prev=Some(off);
   if rec==0{break}
   off+=rec;
  }
 }
 Err(errno::ENOENT)
}
/* True when a directory holds nothing but . and .. entries. */
fn dir_is_empty(dir_inum:u32)->Result<bool,i64>{
 let sb=superblock()?;
 let bs=block_size(&sb);
 let dir=read_inode(&sb,dir_inum)?;
 if dir.i_mode&0o170000!=0o040000{return Err(errno::ENOTDIR)}
 let size=dir.i_size_lo as usize;
 let nblocks=(size+bs-1)/bs;
 let mut blks=dir.i_block;
 for bi in 0..nblocks{
  let mut newb=0u32;
  let blk=file_block(&sb,&mut blks,bi,false,&mut newb)?;
  if blk==0{continue}
  let mut block=[0u8;4096];
  read_block(blk,bs,&mut block)?;
  let mut off=0usize;
  while off+8<=bs{
   let ino=u32::from_le_bytes(block[off..off+4].try_into().unwrap());
   let rec=u16::from_le_bytes(block[off+4..off+6].try_into().unwrap()) as usize;
   let nl=block[off+6] as usize;
   if rec==0||off+rec>bs{break}
   if ino!=0&&nl>0{
    let end=core::cmp::min(off+8+nl,bs);
    let nm=&block[off+8..end];
    if nm!=b"."&&nm!=b".."{
     return Ok(false)
    }
   }
   if rec==0{break}
   off+=rec;
  }
 }
 Ok(true)
}
fn split_parent(path:&str)->Result<(&str,&str),i64>{
 let t=path.trim_start_matches('/').trim_end_matches('/');
 if t.is_empty()||t.contains('\0'){return Err(errno::EINVAL)}
 match t.rfind('/'){
  Some(i)=>{let (d,n)=t.split_at(i);let n=&n[1..];if n.is_empty(){return Err(errno::ENOENT)}if n.len()>255{return Err(errno::ENAMETOOLONG)}Ok((d,n))}
  None=>{if t.is_empty(){return Err(errno::ENOENT)}if t.len()>255{return Err(errno::ENAMETOOLONG)}Ok(("/",t))}
 }
}
fn lookup_inum(rel:&str)->Result<u32,i64>{
 Ok(lookup(rel)?.rdev as u32)
}
/* ---------- public writable API (paths are mount-relative) ---------- */
fn vfs_inode_for(path:&str,inum:u32)->Result<vfs::Inode,i64>{
 let sb=superblock()?;
 let inode=read_inode(&sb,inum)?;
 let n=make_vfs_inode(path,inum,&inode,&sb);
 Ok(n)
}
pub fn create_file(path:&str,mode:u32)->Result<vfs::Inode,i64>{
 if !ready(){return Err(errno::ENODEV)}
 let (dir,name)=split_parent(path)?;
 let dir_inum=lookup_inum(dir)?;
 let inum=alloc_inode(&superblock()?,false)?;
 zero_inode(inum)?;
 let t=(crate::time::realtime_ns()/1_000_000_000) as u32;
 let sb=superblock()?;
 let mut e=[0u8;512];
 read_inode_raw(&sb,inum,&mut e)?;
 let isz=sb.s_inode_size as usize;
 e[0..2].copy_from_slice(&((0o100000|mode&0o7777) as u16).to_le_bytes());
 e[4..8].copy_from_slice(&0u32.to_le_bytes());
 e[12..16].copy_from_slice(&t.to_le_bytes());
 e[16..20].copy_from_slice(&t.to_le_bytes());
 e[26..28].copy_from_slice(&1u16.to_le_bytes());
 e[28..32].copy_from_slice(&0u32.to_le_bytes());
 write_inode_raw(&sb,inum,&e[..isz])?;
 if let Err(e)=dir_insert(dir_inum,inum,name.as_bytes(),1){
  let _=free_inode(&superblock()?,inum,false);
  return Err(e);
 }
 vfs_inode_for(path,inum)
}
pub fn write_file(inum:u32,off:u64,src:&[u8])->Result<usize,i64>{
 if !ready(){return Err(errno::ENODEV)}
 if src.is_empty(){return Ok(0)}
 let sb=superblock()?;
 let bs=block_size(&sb);
 let inode0=read_inode(&sb,inum)?;
 if inode0.i_mode&0o170000==0o040000{return Err(errno::EISDIR)}
 if inode0.i_mode&0o170000!=0o100000{return Err(errno::EINVAL)}
 let mut blks=inode0.i_block;
 let end=off.checked_add(src.len() as u64).ok_or(errno::EINVAL)?;
 let mut newb_total=0u32;
 let mut done=0usize;
 while done<src.len(){
  let poff=off as usize+done;
  let idx=poff/bs;
  let po=poff%bs;
  let blk=file_block(&sb,&mut blks,idx,true,&mut newb_total)?;
  if blk==0{return Err(errno::ENOSPC)}
  let n=core::cmp::min(src.len()-done,bs-po);
  if n<bs||po!=0{
   let mut block=[0u8;4096];
   read_block(blk,bs,&mut block)?;
   block[po..po+n].copy_from_slice(&src[done..done+n]);
   write_block(blk,bs,&block)?;
  }else{
   write_block(blk,bs,&src[done..done+n])?;
  }
  done+=n;
 }
 let cur=inode_sector_count(inum)?;
 let newsize=core::cmp::max(inode0.i_size_lo as u64,end);
 patch_inode(inum,None,Some(newsize as u32),None,Some(cur+newb_total*(bs/512) as u32),true,Some(blks))?;
 Ok(src.len())
}
pub fn truncate_file(inum:u32,size:u64)->Result<(),i64>{
 if !ready(){return Err(errno::ENODEV)}
 let sb=superblock()?;
 let bs=block_size(&sb);
 let inode0=read_inode(&sb,inum)?;
 if inode0.i_mode&0o170000==0o040000{return Err(errno::EISDIR)}
 if inode0.i_mode&0o170000!=0o100000{return Err(errno::EINVAL)}
 let cur_size=inode0.i_size_lo as u64;
 if size==cur_size{return Ok(())}
 if size>cur_size{
  /* Growing exposes holes; they read back as zeros. */
  patch_inode(inum,None,Some(size as u32),None,None,true,None)?;
  return Ok(());
 }
 let keep=((size as usize+bs-1)/bs) as usize;
 let mut blks=inode0.i_block;
 let freed=free_from(&sb,&mut blks,keep)?;
 let cur=inode_sector_count(inum)?;
 let dec=core::cmp::min(cur,freed);
 patch_inode(inum,None,Some(size as u32),None,Some(cur-dec),true,Some(blks))?;
 Ok(())
}
pub fn unlink_path(path:&str)->Result<(),i64>{
 if !ready(){return Err(errno::ENODEV)}
 let (dir,name)=split_parent(path)?;
 let dir_inum=lookup_inum(dir)?;
 let target=lookup(path)?;
 if target.mode&0o170000==0o040000{return Err(errno::EISDIR)}
 let inum=dir_remove(dir_inum,name.as_bytes())?;
 let sb=superblock()?;
 let inode=read_inode(&sb,inum)?;
 let links=inode.i_links_count;
 if links>1{
  patch_inode(inum,None,None,Some(links-1),None,true,None)?;
  return Ok(())
 }
 if inode.i_mode&0o170000==0o120000{
  /* Symlinks have no truncate path; free any data blocks directly. */
  free_all_blocks(inum);
  free_inode(&superblock()?,inum,false)?;
  return Ok(())
 }
 truncate_file(inum,0)?;
 free_inode(&superblock()?,inum,false)?;
 Ok(())
}
/* Hard link: the new name points at the old inode; directories are refused
   like Linux (EPERM) and the target must not exist. */
pub fn link_path(old:&str,new:&str)->Result<(),i64>{
 if !ready(){return Err(errno::ENODEV)}
 let (ndir,nname)=split_parent(new)?;
 let ndir_inum=lookup_inum(ndir)?;
 /* Linux links the target of a symlink, so follow up to 8 links. */
 let mut curbuf=[0u8;128];
 let mut curlen=core::cmp::min(old.len(),127);
 curbuf[..curlen].copy_from_slice(&old.as_bytes()[..curlen]);
 let mut found=false;
 for _ in 0..8{
  let cur=core::str::from_utf8(&curbuf[..curlen]).map_err(|_|errno::EINVAL)?;
  let t=lookup(cur)?;
  if t.mode&0o170000!=0o120000{found=true;break}
  let mut tb=[0u8;128];
  let n=readlink_into(t.rdev as u32,&mut tb)?;
  let ts=core::str::from_utf8(&tb[..n]).map_err(|_|errno::EINVAL)?;
  let (d,_)=split_parent(cur)?;
  let dl=d.len();let tl=ts.len();
  let mut next=[0u8;128];let mut nl=0usize;
  if ts.starts_with('/'){
   if tl>127{return Err(errno::ENAMETOOLONG)}
   next[..tl].copy_from_slice(ts.as_bytes());nl=tl;
  }else{
   if dl+1+tl>127{return Err(errno::ENAMETOOLONG)}
   next[..dl].copy_from_slice(d.as_bytes());
   next[dl]=b'/';
   next[dl+1..dl+1+tl].copy_from_slice(ts.as_bytes());nl=dl+1+tl;
  }
  curbuf[..nl].copy_from_slice(&next[..nl]);curlen=nl;
 }
 if !found{return Err(errno::ELOOP)}
 let ol=core::str::from_utf8(&curbuf[..curlen]).map_err(|_|errno::EINVAL)?;
 let sb=superblock()?;
 let target=lookup(ol)?;
 if target.mode&0o170000==0o040000{return Err(errno::EPERM)}
 let oinum=target.rdev as u32;
 let oinode=read_inode(&sb,oinum)?;
 let ftype=match oinode.i_mode&0o170000{0o100000=>1u8,0o120000=>7u8,_=>1u8};
 dir_insert(ndir_inum,oinum,nname.as_bytes(),ftype)?;
 let cur=read_inode(&sb,oinum)?;
 patch_inode(oinum,None,None,Some(cur.i_links_count.saturating_add(1)),None,true,None)?;
 Ok(())
}
/* Creates a symlink. Targets of 60 bytes or fewer live in the inode's
   block-pointer area (fast symlink), like Linux; longer targets use
   data blocks (capped at 4 KiB here). */
/* Frees every data block of an inode without touching link counts.
   Fast symlinks (i_blocks == 0) hold their target inline and need no
   block work; every other file or directory owns real blocks. */
fn free_all_blocks(inum:u32){
 if let Ok(sb)=superblock(){
  if let Ok(inode)=read_inode(&sb,inum){
   if inode.i_blocks!=0{
    let mut blks=inode.i_block;
    if let Ok(freed)=free_from(&sb,&mut blks,0){
     let cur=inode_sector_count(inum).unwrap_or(0);
     let _=patch_inode(inum,None,Some(0),None,Some(cur.saturating_sub(freed)),true,Some(blks));
    }
   }else{
    let _=patch_inode(inum,None,Some(0),None,None,true,None);
   }
  }
 }
}
pub fn symlink_path(path:&str,target:&str)->Result<(),i64>{
 if !ready(){return Err(errno::ENODEV)}
 let (dir,name)=split_parent(path)?;
 let dir_inum=lookup_inum(dir)?;
 let sb=superblock()?;
 let inum=alloc_inode(&sb,false)?;
 zero_inode(inum)?;
 let t=target.as_bytes();
 if t.len()>4096{let _=free_inode(&superblock()?,inum,false);return Err(errno::ENAMETOOLONG)}
 let mut e=[0u8;512];
 read_inode_raw(&sb,inum,&mut e)?;
 let isz=sb.s_inode_size as usize;
 let tm=(crate::time::realtime_ns()/1_000_000_000) as u32;
 e[0..2].copy_from_slice(&((0o120000|0o777) as u16).to_le_bytes());
 e[4..8].copy_from_slice(&(t.len() as u32).to_le_bytes());
 e[12..16].copy_from_slice(&tm.to_le_bytes());
 e[16..20].copy_from_slice(&tm.to_le_bytes());
 e[26..28].copy_from_slice(&1u16.to_le_bytes());
 if t.len()<=60{
  e[40..40+t.len()].copy_from_slice(t);
  e[28..32].copy_from_slice(&0u32.to_le_bytes());
  write_inode_raw(&sb,inum,&e[..isz])?;
 }else{
  let mut blks=[0u32;15];
  let mut newb=0u32;
  let mut done=0usize;
  while done<t.len(){
   let idx=done/block_size(&sb);
   if idx>=12{free_all_blocks(inum);let _=free_inode(&superblock()?,inum,false);return Err(errno::ENAMETOOLONG)}
   let blk=file_block(&sb,&mut blks,idx,true,&mut newb)?;
   if blk==0{free_all_blocks(inum);let _=free_inode(&superblock()?,inum,false);return Err(errno::ENOSPC)}
   let n=core::cmp::min(t.len()-done,block_size(&sb));
   let mut block=[0u8;4096];
   block[..n].copy_from_slice(&t[done..done+n]);
   write_block(blk,block_size(&sb),&block)?;
   done+=n;
  }
  e[28..32].copy_from_slice(&((newb*(block_size(&sb)/512) as u32)).to_le_bytes());
  for i in 0..15{e[40+i*4..44+i*4].copy_from_slice(&blks[i].to_le_bytes());}
  write_inode_raw(&sb,inum,&e[..isz])?;
 }
 if let Err(err)=dir_insert(dir_inum,inum,name.as_bytes(),7){
  /* truncate_file refuses symlinks; free any blocks directly so a
     failed create never leaks. */
  free_all_blocks(inum);
  let _=free_inode(&superblock()?,inum,false);
  return Err(err);
 }
 Ok(())
}
/* Reads a symlink body: fast targets come from the inode area, longer
   ones from data blocks. */
pub fn readlink_into(inum:u32,out:&mut [u8])->Result<usize,i64>{
 if !ready(){return Err(errno::ENODEV)}
 let sb=superblock()?;
 let inode=read_inode(&sb,inum)?;
 if inode.i_mode&0o170000!=0o120000{return Err(errno::EINVAL)}
 let size=inode.i_size_lo as usize;
 if size>out.len(){return Err(errno::ENAMETOOLONG)}
 if size==0{return Ok(0)}
 if size<=60{
  let raw=unsafe{core::slice::from_raw_parts((&inode.i_block as *const [u32;15]).cast::<u8>(),60)};
  out[..size].copy_from_slice(&raw[..size]);
  return Ok(size);
 }
 let mut blks=inode.i_block;
 let mut done=0usize;
 while done<size{
  let mut newb=0u32;
  let blk=file_block(&sb,&mut blks,done/block_size(&sb),false,&mut newb)?;
  if blk==0{break}
  let mut block=[0u8;4096];
  read_block(blk,block_size(&sb),&mut block)?;
  let n=core::cmp::min(size-done,core::cmp::min(block_size(&sb),out.len()-done));
  out[done..done+n].copy_from_slice(&block[..n]);
  done+=n;
 }
 Ok(done)
}
/* Renames within one filesystem, replacing files and empty directories.
   Moving a directory rewrites its ".." and the parent link counts. */
pub fn rename_path(old:&str,new:&str)->Result<(),i64>{
 if !ready(){return Err(errno::ENODEV)}
 if old==new{return Ok(())}
 if new.starts_with(old)&&new.as_bytes().get(old.len())==Some(&b'/'){return Err(errno::EINVAL)}
 let (odir,oname)=split_parent(old)?;
 let (ndir,nname)=split_parent(new)?;
 let odir_inum=lookup_inum(odir)?;
 let ndir_inum=lookup_inum(ndir)?;
 let sb=superblock()?;
 let otarget=lookup(old)?;
 let oinum=otarget.rdev as u32;
 let oinode=read_inode(&sb,oinum)?;
 let odir_mode=oinode.i_mode&0o170000;
 if odir_mode!=0o100000&&odir_mode!=0o040000{return Err(errno::EINVAL)}
 if let Ok(ntarget)=lookup(new){
  let ninum=ntarget.rdev as u32;
  if ninum==oinum{return Ok(())}
  let ninode=read_inode(&sb,ninum)?;
  let ndir_mode=ninode.i_mode&0o170000;
  if odir_mode==0o040000{
   if ndir_mode!=0o040000{return Err(errno::ENOTDIR)}
   if !dir_is_empty(ninum)?{return Err(errno::ENOTEMPTY)}
   dir_remove(ndir_inum,nname.as_bytes())?;
   let nblks=read_inode(&sb,ninum)?.i_block;
   let mut nb=nblks;
   let _=free_from(&sb,&mut nb,0)?;
   free_inode(&superblock()?,ninum,true)?;
   let psb=superblock()?;
   let pinode=read_inode(&psb,ndir_inum)?;
   patch_inode(ndir_inum,None,None,Some(pinode.i_links_count.saturating_sub(1)),None,true,None)?;
  }else{
   if ndir_mode==0o040000{return Err(errno::EISDIR)}
   dir_remove(ndir_inum,nname.as_bytes())?;
   truncate_file(ninum,0)?;
   free_inode(&superblock()?,ninum,false)?;
  }
 }
 let ftype=if odir_mode==0o040000{2u8}else{1u8};
 dir_insert(ndir_inum,oinum,nname.as_bytes(),ftype)?;
 dir_remove(odir_inum,oname.as_bytes())?;
 if odir_mode==0o040000{
  /* Point ".." at the new parent. */
  let mut blks=read_inode(&sb,oinum)?.i_block;
  let mut newb=0u32;
  let blk=file_block(&sb,&mut blks,0,false,&mut newb)?;
  if blk!=0{
   let mut block=[0u8;4096];
   read_block(blk,block_size(&sb),&mut block)?;
   if block.len()>=16{
    block[12..16].copy_from_slice(&ndir_inum.to_le_bytes());
    write_block(blk,block_size(&sb),&block)?;
   }
  }
  if odir_inum!=ndir_inum{
   let psb=superblock()?;
   let poinode=read_inode(&psb,odir_inum)?;
   patch_inode(odir_inum,None,None,Some(poinode.i_links_count.saturating_sub(1)),None,true,None)?;
   let psb2=superblock()?;
   let pninode=read_inode(&psb2,ndir_inum)?;
   patch_inode(ndir_inum,None,None,Some(pninode.i_links_count.saturating_add(1)),None,true,None)?;
  }
 }
 Ok(())
}
pub fn mkdir_path(path:&str,mode:u32)->Result<vfs::Inode,i64>{
 if !ready(){return Err(errno::ENODEV)}
 let (dir,name)=split_parent(path)?;
 let dir_inum=lookup_inum(dir)?;
 let sb=superblock()?;
 let inum=alloc_inode(&sb,true)?;
 zero_inode(inum)?;
 let t=(crate::time::realtime_ns()/1_000_000_000) as u32;
 let mut e=[0u8;512];
 read_inode_raw(&sb,inum,&mut e)?;
 let isz=sb.s_inode_size as usize;
 e[0..2].copy_from_slice(&((0o040000|mode&0o7777) as u16).to_le_bytes());
 e[4..8].copy_from_slice(&(block_size(&sb) as u32).to_le_bytes());
 e[12..16].copy_from_slice(&t.to_le_bytes());
 e[16..20].copy_from_slice(&t.to_le_bytes());
 e[26..28].copy_from_slice(&2u16.to_le_bytes());
 {
  let bs=block_size(&sb);
  let mut nb=0u32;
  let mut blks=[0u32;15];
  let b=file_block(&sb,&mut blks,0,true,&mut nb)?;
  if b==0{let _=free_inode(&superblock()?,inum,true);return Err(errno::ENOSPC)}
  let mut dot=[0u8;4096];
  dot[0..4].copy_from_slice(&inum.to_le_bytes());
  dot[4..6].copy_from_slice(&12u16.to_le_bytes());
  dot[6]=1;dot[7]=2;
  dot[8]=b'.';
  dot[12..16].copy_from_slice(&dir_inum.to_le_bytes());
  dot[16..18].copy_from_slice(&((bs-12) as u16).to_le_bytes());
  dot[18]=2;dot[19]=2;
  dot[20]=b'.';dot[21]=b'.';
  write_block(b,bs,&dot)?;
  e[28..32].copy_from_slice(&((bs/512) as u32).to_le_bytes());
  for i in 0..15{e[40+i*4..44+i*4].copy_from_slice(&blks[i].to_le_bytes());}
 }
 write_inode_raw(&sb,inum,&e[..isz])?;
 if let Err(err)=dir_insert(dir_inum,inum,name.as_bytes(),2){
  /* truncate_file refuses directories, so free the new directory's blocks
     directly (a duplicate mkdir must not leak its data block). */
  if let Ok(sb2)=superblock(){
   if let Ok(inode2)=read_inode(&sb2,inum){
    let mut blks2=inode2.i_block;
    if let Ok(freed)=free_from(&sb2,&mut blks2,0){
     let cur=inode_sector_count(inum).unwrap_or(0);
     let _=patch_inode(inum,None,Some(0),None,Some(cur.saturating_sub(freed)),true,Some(blks2));
    }
   }
  }
  let _=free_inode(&superblock()?,inum,true);
  return Err(err);
 }
 /* A new subdirectory adds a link to its parent via ... */
 let psb=superblock()?;
 let pinode=read_inode(&psb,dir_inum)?;
 patch_inode(dir_inum,None,None,Some(pinode.i_links_count.saturating_add(1)),None,true,None)?;
 vfs_inode_for(path,inum)
}
pub fn rmdir_path(path:&str)->Result<(),i64>{
 if !ready(){return Err(errno::ENODEV)}
 let (dir,name)=split_parent(path)?;
 let dir_inum=lookup_inum(dir)?;
 let inum={
  let sb=superblock()?;
  let t=path.trim_start_matches('/').trim_end_matches('/');
  let target=lookup(t)?;
  target.rdev as u32
 };
 let sb=superblock()?;
 let inode=read_inode(&sb,inum)?;
 if inode.i_mode&0o170000!=0o040000{return Err(errno::ENOTDIR)}
 if !dir_is_empty(inum)?{return Err(errno::ENOTEMPTY)}
  dir_remove(dir_inum,name.as_bytes())?;
 {
  /* Free the directory's own blocks; truncate_file refuses directories. */
  let sb=superblock()?;
  let inode=read_inode(&sb,inum)?;
  let mut blks=inode.i_block;
  let freed=free_from(&sb,&mut blks,0)?;
  let cur=inode_sector_count(inum)?;
  patch_inode(inum,None,Some(0),None,Some(cur.saturating_sub(freed)),true,Some(blks))?;
 }
 free_inode(&superblock()?,inum,true)?;
 let psb=superblock()?;
 let pinode=read_inode(&psb,dir_inum)?;
 patch_inode(dir_inum,None,None,Some(pinode.i_links_count.saturating_sub(1)),None,true,None)?;
 Ok(())
}
pub fn truncate(inum:u32,size:u64)->Result<(),i64>{truncate_file(inum,size)}
/// Link count for stat (1 plus real hard links); 1 when unreadable.
pub fn nlink(inum:u32)->u32{
 if !ready(){return 1}
 match superblock().and_then(|sb|read_inode(&sb,inum)){
  Ok(inode)=>{
   let n=inode.i_links_count as u32;
   if n==0{1}else{n}
  }
  Err(_)=>1,
 }
}
pub fn file_size(inum:u32)->Option<u64>{
 let sb=superblock().ok()?;
 let inode=read_inode(&sb,inum).ok()?;
 Some(inode.i_size_lo as u64)
}
pub fn chmod(inum:u32,mode:u32)->Result<(),i64>{
 if !ready(){return Err(errno::ENODEV)}
 let sb=superblock()?;
 let inode=read_inode(&sb,inum)?;
 let ft=inode.i_mode&0o170000;
 if ft!=0o100000&&ft!=0o040000{return Err(errno::EINVAL)}
 patch_inode(inum,Some(ft|(mode&0o7777) as u16),None,None,None,true,None)
}
