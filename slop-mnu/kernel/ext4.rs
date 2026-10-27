//! Minimal read-only ext4 filesystem on the first virtio-blk device.
//!
//! Shares the 0xEF53 superblock magic with ext2 but stores file data in
//! extent trees (`i_flags & EXTENTS`) instead of indirect block lists.
//! Only 32-bit filesystems without the 64bit feature are accepted; anything
//! else fails loudly at probe time instead of being misread. Directories
//! use the same `dir_entry_2` layout as ext2, and fast symlinks (target
//! inline in `i_block`) work like on ext2. There is no write path yet:
//! every mutating VFS operation on an ext4 mount fails with EROFS.
use crate::{errno, vfs};
const EXT4_MAGIC: u16 = 0xEF53;
/* Incompatible feature bits we understand (low 32 bits of s_feature_incompat). */
const INCOMPAT_EXTENTS: u32 = 0x40;
const INCOMPAT_64BIT: u32 = 0x80;
/* Inode flags. */
const EXTENTS_FL: u32 = 0x0008_0000;
/* Extent tree magic. */
const EXT_MAGIC: u16 = 0xF30A;
/* vfs::Inode numbers for on-disk ext4 inodes (disjoint from ext2's 0x100000). */
pub const INO_BASE: u64 = 0x200000;
pub const INO_END: u64 = 0x300000;
pub fn is_ext4_ino(ino: u64) -> bool {
    ino >= INO_BASE && ino < INO_END
}
pub fn ready() -> bool {
    crate::driver::virtio_blk::ready()
}
fn read_bytes(off: u64, dst: &mut [u8]) -> Result<(), i64> {
    let cap = crate::driver::virtio_blk::capacity() * 512;
    if off as usize + dst.len() > cap as usize {
        return Err(errno::EINVAL);
    }
    let mut done = 0usize;
    while done < dst.len() {
        let sec = (off as usize + done) / 512;
        let soff = (off as usize + done) % 512;
        let n = core::cmp::min(dst.len() - done, 512 - soff);
        let mut frame = [0u8; 512];
        if crate::driver::virtio_blk::read_sector(sec as u64, &mut frame).is_err() {
            return Err(errno::EIO);
        }
        dst[done..done + n].copy_from_slice(&frame[soff..soff + n]);
        done += n;
    }
    Ok(())
}
fn read_block(block: u64, blksize: usize, buf: &mut [u8]) -> Result<(), i64> {
    if buf.len() < blksize {
        return Err(errno::EINVAL);
    }
    let off = block * blksize as u64;
    read_bytes(off, &mut buf[..blksize])
}
#[repr(C)]
struct Super {
    s_inodes_count: u32,      // 0
    s_blocks_count_lo: u32,   // 4
    _r0: u32,                 // 8
    _r1: u32,                 // 12
    _r2: u32,                 // 16
    s_first_data_block: u32,  // 20
    s_log_block_size: u32,    // 24
    _r3: u32,                 // 28
    s_blocks_per_group: u32,  // 32
    _r4: u32,                 // 36
    s_inodes_per_group: u32,  // 40
    _r5: u32,                 // 44
    _r6: u32,                 // 48
    _mnt_count: u16,          // 52
    _max_mnt_count: u16,      // 54
    s_magic: u16,             // 56
    _r7: [u8; 30],            // 58..88
    s_inode_size: u16,        // 88
    _block_group_nr: u16,     // 90
    s_feature_compat: u32,    // 92
    s_feature_incompat: u32,  // 96
}
pub fn superblock() -> Result<Super, i64> {
    let mut buf = [0u8; 1024];
    read_bytes(1024, &mut buf)?;
    let sb: Super = unsafe { core::ptr::read(buf.as_ptr() as *const Super) };
    if sb.s_magic != EXT4_MAGIC {
        return Err(errno::EINVAL);
    }
    Ok(sb)
}
/// True when the attached image is an ext4 filesystem we can read:
/// same magic as ext2, extents present, 64bit absent.
pub fn probe() -> bool {
    if !ready() {
        return false;
    }
    match superblock() {
        Ok(sb) => {
            (sb.s_feature_incompat & INCOMPAT_EXTENTS) != 0
                && (sb.s_feature_incompat & INCOMPAT_64BIT) == 0
        }
        Err(_) => false,
    }
}
fn block_size(sb: &Super) -> usize {
    1024usize << (sb.s_log_block_size as usize)
}
fn groups_count(sb: &Super) -> usize {
    (sb.s_blocks_count_lo as usize + sb.s_blocks_per_group as usize - 1)
        / sb.s_blocks_per_group as usize
}
fn group_desc_block(sb: &Super) -> u64 {
    if block_size(sb) == 1024 {
        2
    } else {
        1
    }
}
#[repr(C)]
struct GroupDesc {
    bg_block_bitmap_lo: u32,
    bg_inode_bitmap_lo: u32,
    bg_inode_table_lo: u32,
    bg_free_blocks_count_lo: u16,
    bg_free_inodes_count_lo: u16,
    bg_used_dirs_count_lo: u16,
    bg_pad: u16,
    _r: [u8; 12],
}
fn read_group_desc(sb: &Super, g: usize) -> Result<GroupDesc, i64> {
    if g >= groups_count(sb) {
        return Err(errno::EINVAL);
    }
    let bs = block_size(sb);
    let mut buf = [0u8; 4096];
    read_block(group_desc_block(sb), bs, &mut buf)?;
    let off = g * 32;
    let gd: GroupDesc = unsafe { core::ptr::read(buf[off..].as_ptr() as *const GroupDesc) };
    Ok(gd)
}
#[repr(C)]
struct Inode {
    i_mode: u16,
    i_uid: u16,
    i_size_lo: u32,
    i_atime: u32,
    i_ctime: u32,
    i_mtime: u32,
    i_dtime: u32,
    i_gid: u16,
    i_links_count: u16,
    i_blocks_lo: u32,
    i_flags: u32,
    i_osd1: u32,
    i_block: [u32; 15],
    i_generation: u32,
    i_file_acl_lo: u32,
    i_size_high: u32,
    i_obso_faddr: u32,
    i_osd2: [u8; 12],
}
fn read_inode(sb: &Super, inum: u32) -> Result<Inode, i64> {
    if inum == 0 {
        return Err(errno::EINVAL);
    }
    let g = ((inum - 1) as usize) / sb.s_inodes_per_group as usize;
    let idx = (inum - 1) as usize % sb.s_inodes_per_group as usize;
    let gd = read_group_desc(sb, g)?;
    let bs = block_size(sb);
    let inode_size = sb.s_inode_size as usize;
    if inode_size < 128 || inode_size > 1024 {
        return Err(errno::EINVAL);
    }
    let table_block = gd.bg_inode_table_lo as u64;
    let byte_off = idx * inode_size;
    let block = table_block + (byte_off / bs) as u64;
    let off = byte_off % bs;
    let mut buf = [0u8; 4096];
    read_block(block, bs, &mut buf)?;
    if off + 128 > bs {
        return Err(errno::EINVAL);
    }
    let inode: Inode = unsafe { core::ptr::read(buf[off..].as_ptr() as *const Inode) };
    Ok(inode)
}
fn inode_len(inode: &Inode) -> u64 {
    (inode.i_size_lo as u64) | ((inode.i_size_high as u64) << 32)
}
fn is_extents(inode: &Inode) -> bool {
    (inode.i_flags & EXTENTS_FL) != 0
}
#[repr(C)]
#[derive(Clone, Copy)]
struct ExtentHeader {
    eh_magic: u16,
    eh_entries: u16,
    eh_max: u16,
    eh_depth: u16,
    eh_generation: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct ExtentIdx {
    ei_block: u32,
    ei_leaf_lo: u32,
    ei_leaf_hi: u16,
    ei_unused: u16,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Extent {
    ee_block: u32,
    ee_len: u16,
    ee_start_hi: u16,
    ee_start_lo: u32,
}
fn read_header(bytes: &[u8]) -> Result<ExtentHeader, i64> {
    if bytes.len() < 12 {
        return Err(errno::EINVAL);
    }
    let h: ExtentHeader =
        unsafe { core::ptr::read(bytes.as_ptr() as *const ExtentHeader) };
    if h.eh_magic != EXT_MAGIC {
        return Err(errno::EINVAL);
    }
    Ok(h)
}
/// Maps a file logical block to a filesystem physical block through the
/// extent tree rooted at `node` (the inode's `i_block` for depth 0, or a
/// full filesystem block for deeper levels). Returns 0 for holes.
fn extent_lookup(
    sb: &Super,
    node: &[u8],
    lb: u32,
    depth_left: usize,
) -> Result<u64, i64> {
    if depth_left > 8 {
        return Err(errno::EINVAL);
    }
    let bs = block_size(sb);
    let h = read_header(node)?;
    let used = 12 + (h.eh_entries as usize) * 12;
    if used > node.len() || used > bs {
        return Err(errno::EINVAL);
    }
    if h.eh_depth == 0 {
        for i in 0..h.eh_entries as usize {
            let off = 12 + i * 12;
            let e: Extent =
                unsafe { core::ptr::read(node[off..].as_ptr() as *const Extent) };
            if e.ee_start_hi != 0 {
                return Err(errno::EINVAL);
            }
            let len = (e.ee_len & 0x7FFF) as u32;
            if len == 0 {
                continue;
            }
            if lb >= e.ee_block && lb < e.ee_block.wrapping_add(len) {
                if (e.ee_len & 0x8000) != 0 {
                    return Ok(0);
                }
                return Ok(e.ee_start_lo as u64 + (lb - e.ee_block) as u64);
            }
        }
        return Ok(0);
    }
    for i in 0..h.eh_entries as usize {
        let off = 12 + i * 12;
        let x: ExtentIdx =
            unsafe { core::ptr::read(node[off..].as_ptr() as *const ExtentIdx) };
        if x.ei_leaf_hi != 0 {
            return Err(errno::EINVAL);
        }
        let next_is_last = i + 1 == h.eh_entries as usize;
        if lb >= x.ei_block && (next_is_last || lb < unsafe {
            core::ptr::read(node[off + 12..].as_ptr() as *const ExtentIdx).ei_block
        }) {
            let bs = block_size(sb);
            let mut buf = [0u8; 4096];
            read_block(x.ei_leaf_lo as u64, bs, &mut buf)?;
            return extent_lookup(sb, &buf[..bs], lb, depth_left + 1);
        }
    }
    Ok(0)
}
fn read_file_block(sb: &Super, inode: &Inode, block_index: u32) -> Result<u64, i64> {
    if !is_extents(inode) {
        return Err(errno::EINVAL);
    }
    let raw: &[u8] = unsafe {
        core::slice::from_raw_parts(
            (&inode.i_block as *const [u32; 15]).cast::<u8>(),
            60,
        )
    };
    extent_lookup(sb, raw, block_index, 0)
}
fn make_vfs_inode(path: &str, inum: u32, inode: &Inode, sb: &Super) -> vfs::Inode {
    let _ = sb;
    let mode = inode.i_mode as u32;
    let size = inode_len(inode);
    let mut name = [0u8; 127 + 1];
    let b = path.as_bytes();
    let nl = b.len().min(127);
    name[..nl].copy_from_slice(&b[..nl]);
    vfs::Inode {
        ino: (inum as u64) + INO_BASE,
        mode: if mode & 0o170000 == 0 {
            vfs::S_IFREG | (mode & 0o777)
        } else {
            mode
        },
        size,
        capacity: size,
        indirect: 0,
        dindirect: 0,
        data: core::ptr::null_mut(),
        rdev: inum as u64,
        pages: [0; 8],
        npages: 0,
        name,
        name_len: nl,
    }
}
pub fn lookup(path: &str) -> Result<vfs::Inode, i64> {
    if !ready() {
        return Err(errno::ENODEV);
    }
    let sb = superblock()?;
    if (sb.s_feature_incompat & INCOMPAT_EXTENTS) == 0
        || (sb.s_feature_incompat & INCOMPAT_64BIT) != 0
    {
        return Err(errno::EINVAL);
    }
    let mut inum = 2u32;
    let mut inode = read_inode(&sb, inum)?;
    let trimmed = path.trim_start_matches('/').trim_end_matches('/');
    if trimmed.is_empty() {
        return Ok(make_vfs_inode("/mnt", inum, &inode, &sb));
    }
    for comp in trimmed.split('/') {
        if comp.is_empty() || comp == "." {
            continue;
        }
        if comp == ".." {
            inum = 2;
            inode = read_inode(&sb, inum)?;
            continue;
        }
        let size = inode_len(&inode) as usize;
        let mut off = 0usize;
        let mut found = None;
        let bs = block_size(&sb);
        while off < size {
            let blk = read_file_block(&sb, &inode, (off / bs) as u32)?;
            if blk == 0 {
                break;
            }
            let mut block = [0u8; 4096];
            read_block(blk, bs, &mut block)?;
            let boff = off % bs;
            let ino = u32::from_le_bytes(block[boff..boff + 4].try_into().unwrap());
            let rec_len =
                u16::from_le_bytes(block[boff + 4..boff + 6].try_into().unwrap()) as usize;
            let name_len = block[boff + 6] as usize;
            if rec_len == 0 || boff + rec_len > bs {
                break;
            }
            if ino != 0
                && name_len == comp.len()
                && &block[boff + 8..boff + 8 + name_len] == comp.as_bytes()
            {
                found = Some(ino);
                break;
            }
            off += rec_len;
        }
        let ino = found.ok_or(errno::ENOENT)?;
        inode = read_inode(&sb, ino)?;
        inum = ino;
    }
    let n = make_vfs_inode(path, inum, &inode, &sb);
    Ok(n)
}
pub fn read(inode_rdev: u32, off: u64, dst: &mut [u8]) -> Result<usize, i64> {
    let sb = superblock()?;
    let inode = read_inode(&sb, inode_rdev)?;
    if !is_extents(&inode) {
        return Err(errno::EINVAL);
    }
    let size = inode_len(&inode) as usize;
    if off as usize >= size {
        return Ok(0);
    }
    let bs = block_size(&sb);
    let mut done = 0usize;
    while done < dst.len() && off as usize + done < size {
        let poff = off as usize + done;
        let blk = read_file_block(&sb, &inode, (poff / bs) as u32)?;
        let po = poff % bs;
        let n = core::cmp::min(dst.len() - done, core::cmp::min(bs - po, size - poff));
        if n == 0 {
            break;
        }
        if blk == 0 {
            dst[done..done + n].fill(0);
            done += n;
            continue;
        }
        let mut block = [0u8; 4096];
        read_block(blk, bs, &mut block)?;
        dst[done..done + n].copy_from_slice(&block[po..po + n]);
        done += n;
    }
    Ok(done)
}
pub fn is_dir(path: &str) -> bool {
    if path == "/mnt" {
        return true;
    }
    if let Ok(inode) = lookup(path) {
        inode.mode & 0o040000 != 0
    } else {
        false
    }
}
pub fn readdir(path: &str, mut cb: impl FnMut(u64, u32, &str)) -> Result<usize, i64> {
    let inum = if path == "/" || path.is_empty() {
        2
    } else {
        let t = path.trim_start_matches('/').trim_end_matches('/');
        if t.is_empty() {
            2
        } else {
            lookup(t)?.rdev as u32
        }
    };
    let sb = superblock()?;
    let inode = read_inode(&sb, inum)?;
    let size = inode_len(&inode) as usize;
    let mut off = 0usize;
    let mut count = 0;
    let bs = block_size(&sb);
    while off < size {
        let blk = read_file_block(&sb, &inode, (off / bs) as u32)?;
        if blk == 0 {
            break;
        }
        let mut block = [0u8; 4096];
        read_block(blk, bs, &mut block)?;
        let mut boff = off % bs;
        if boff != 0 {
            let rec_len =
                u16::from_le_bytes(block[boff..boff + 2].try_into().unwrap()) as usize;
            if rec_len == 0 {
                break;
            }
            off += rec_len;
            continue;
        }
        while boff < bs && off < size {
            let ino = u32::from_le_bytes(block[boff..boff + 4].try_into().unwrap());
            let rec_len =
                u16::from_le_bytes(block[boff + 4..boff + 6].try_into().unwrap()) as usize;
            let name_len = block[boff + 6] as usize;
            let ftype = block[boff + 7] as u32;
            if rec_len == 0 || boff + rec_len > bs {
                break;
            }
            if ino != 0 && name_len > 0 && boff + 8 + name_len <= bs {
                if let Ok(name) =
                    core::str::from_utf8(&block[boff + 8..boff + 8 + name_len])
                {
                    cb(ino as u64, ftype, name);
                    count += 1;
                }
            }
            boff += rec_len;
            off += rec_len;
        }
    }
    Ok(count)
}
/// Reads a symlink target: fast symlinks store it inline in `i_block`,
/// longer ones in data blocks (extent-mapped, like regular files).
pub fn readlink_into(inum: u32, out: &mut [u8]) -> Result<usize, i64> {
    let sb = superblock()?;
    let inode = read_inode(&sb, inum)?;
    let size = inode_len(&inode) as usize;
    if size == 0 || size > out.len() {
        return Err(errno::EINVAL);
    }
    if !is_extents(&inode) && size < 60 {
        let raw: &[u8] = unsafe {
            core::slice::from_raw_parts(
                (&inode.i_block as *const [u32; 15]).cast::<u8>(),
                60,
            )
        };
        out[..size].copy_from_slice(&raw[..size]);
        return Ok(size);
    }
    let mut tmp = [0u8; 4096];
    let want = core::cmp::min(size, tmp.len());
    let n = read(inum, 0, &mut tmp[..want])?;
    if n < size {
        return Err(errno::EIO);
    }
    out[..size].copy_from_slice(&tmp[..size]);
    Ok(size)
}
pub fn file_size(inum: u32) -> Option<u64> {
    let sb = superblock().ok()?;
    let inode = read_inode(&sb, inum).ok()?;
    Some(inode_len(&inode))
}

pub fn nlink(inum: u32) -> u32 {
    let sb = match superblock() {
        Ok(v) => v,
        Err(_) => return 1,
    };
    match read_inode(&sb, inum) {
        Ok(inode) => inode.i_links_count as u32,
        Err(_) => 1,
    }
}

/* ---------- write path (overwrite within EOF; no allocation yet) ---------- */

fn write_bytes_at(off: u64, src: &[u8]) -> Result<(), i64> {
    let cap = crate::driver::virtio_blk::capacity() * 512;
    if off as usize + src.len() > cap as usize {
        return Err(errno::EINVAL);
    }
    let mut done = 0usize;
    while done < src.len() {
        let sec = (off as usize + done) / 512;
        let soff = (off as usize + done) % 512;
        let n = core::cmp::min(src.len() - done, 512 - soff);
        let mut frame = [0u8; 512];
        if soff != 0 || n < 512 {
            if crate::driver::virtio_blk::read_sector(sec as u64, &mut frame).is_err() {
                return Err(crate::errno::EIO);
            }
        }
        frame[soff..soff + n].copy_from_slice(&src[done..done + n]);
        if crate::driver::virtio_blk::write_sector(sec as u64, &frame).is_err() {
            return Err(crate::errno::EIO);
        }
        done += n;
    }
    Ok(())
}

fn write_block(block: u64, blksize: usize, buf: &[u8]) -> Result<(), i64> {
    if buf.len() < blksize {
        return Err(errno::EINVAL);
    }
    write_bytes_at(block * blksize as u64, &buf[..blksize])
}

fn write_inode(sb: &Super, inum: u32, inode: &Inode) -> Result<(), i64> {
    if inum == 0 {
        return Err(errno::EINVAL);
    }
    let g = ((inum - 1) as usize) / sb.s_inodes_per_group as usize;
    let idx = (inum - 1) as usize % sb.s_inodes_per_group as usize;
    let gd = read_group_desc(sb, g)?;
    let bs = block_size(sb);
    let inode_size = sb.s_inode_size as usize;
    if inode_size < 128 || inode_size > 1024 {
        return Err(errno::EINVAL);
    }
    let table_block = gd.bg_inode_table_lo as u64;
    let byte_off = idx * inode_size;
    let block = table_block + (byte_off / bs) as u64;
    let off = byte_off % bs;
    if off + 128 > bs {
        return Err(errno::EINVAL);
    }
    let mut buf = [0u8; 4096];
    read_block(block, bs, &mut buf)?;
    let dst = &mut buf[off..off + core::mem::size_of::<Inode>()];
    let src = unsafe {
        core::slice::from_raw_parts(
            (inode as *const Inode) as *const u8,
            core::mem::size_of::<Inode>(),
        )
    };
    dst.copy_from_slice(src);
    write_block(block, bs, &buf)?;
    Ok(())
}

/* Overwrites bytes within the current EOF (no allocation, no extension).
   Extending writes, holes, and all metadata allocation return EROFS until
   the allocator lands. Returns bytes written. */
pub fn write_file(inum: u32, off: u64, src: &[u8]) -> Result<usize, i64> {
    if !ready() {
        return Err(errno::ENODEV);
    }
    if src.is_empty() {
        return Ok(0);
    }
    let sb = superblock()?;
    let mut inode = read_inode(&sb, inum)?;
    if inode.i_mode & 0o170000 == 0o040000 {
        return Err(errno::EISDIR);
    }
    if inode.i_mode & 0o170000 != 0o100000 {
        return Err(errno::EINVAL);
    }
    if !is_extents(&inode) {
        return Err(errno::EINVAL);
    }
    let size = inode_len(&inode);
    let end = off.checked_add(src.len() as u64).ok_or(errno::EINVAL)?;
    if end > size {
        return Err(errno::EROFS);
    }
    let bs = block_size(&sb);
    let mut done = 0usize;
    while done < src.len() {
        let poff = off as usize + done;
        let blk = read_file_block(&sb, &inode, (poff / bs) as u32)?;
        if blk == 0 {
            return Err(errno::EROFS);
        }
        let po = poff % bs;
        let n = core::cmp::min(src.len() - done, bs - po);
        if n < bs || po != 0 {
            let mut block = [0u8; 4096];
            read_block(blk, bs, &mut block)?;
            block[po..po + n].copy_from_slice(&src[done..done + n]);
            write_block(blk, bs, &block)?;
        } else {
            write_block(blk, bs, &src[done..done + n])?;
        }
        done += n;
    }
    let t = (crate::time::realtime_ns() / 1_000_000_000) as u32;
    inode.i_mtime = t;
    inode.i_ctime = t;
    write_inode(&sb, inum, &inode)?;
    Ok(src.len())
}
