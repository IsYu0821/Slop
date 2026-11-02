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
    crate::driver::blk::ready()
}
fn read_bytes(off: u64, dst: &mut [u8]) -> Result<(), i64> {
    let cap = crate::driver::blk::capacity() * 512;
    if off as usize + dst.len() > cap as usize {
        return Err(errno::EINVAL);
    }
    let mut done = 0usize;
    while done < dst.len() {
        let sec = (off as usize + done) / 512;
        let soff = (off as usize + done) % 512;
        let n = core::cmp::min(dst.len() - done, 512 - soff);
        let mut frame = [0u8; 512];
        if crate::driver::blk::read_sector(sec as u64, &mut frame).is_err() {
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
    s_r_blocks_count_lo: u32, // 8
    s_free_blocks_count_lo: u32, // 12
    s_free_inodes_count: u32, // 16
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
/* Resolve an already-open file by on-disk inode number (for page faults and
   other ino-keyed paths that cannot re-walk a pathname). The returned inode
   carries no name; callers must not rely on it. None on any error. */
pub fn stat_ino(inum: u32) -> Option<vfs::Inode> {
    if !ready() {
        return None;
    }
    let sb = superblock().ok()?;
    let inode = read_inode(&sb, inum).ok()?;
    Some(make_vfs_inode("", inum, &inode, &sb))
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
    let cap = crate::driver::blk::capacity() * 512;
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
            if crate::driver::blk::read_sector(sec as u64, &mut frame).is_err() {
                return Err(crate::errno::EIO);
            }
        }
        frame[soff..soff + n].copy_from_slice(&src[done..done + n]);
        if crate::driver::blk::write_sector(sec as u64, &frame).is_err() {
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

/* Overwrites bytes within the current EOF and extends the file (block
   allocation + inline extent growth; depth-0 trees with <=4 extents only,
   no holes — gaps are zero-filled real blocks). Returns bytes written. */
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
    let bs = block_size(&sb);
    /* Extend: allocate + zero every new block, grow the extent tree. */
    if end > size {
        /* Zero the tail of the last partial block so newly exposed bytes
           read as zeros (Linux semantics, no stale-data leak). */
        if size % bs as u64 != 0 {
            let last_lb = (size as usize) / bs;
            if let Ok(blk) = read_file_block(&sb, &inode, last_lb as u32) {
                if blk != 0 {
                    let mut block = [0u8; 4096];
                    if read_block(blk, bs, &mut block).is_ok() {
                        let zoff = (size as usize) % bs;
                        for b in block[zoff..bs].iter_mut() { *b = 0; }
                        let _ = write_block(blk, bs, &block);
                    }
                }
            }
        }
        let first_new_lb = (size as usize + bs - 1) / bs;
        let last_lb = (end as usize - 1) / bs;
        let zero = [0u8; 4096];
        for lb in first_new_lb..=last_lb {
            let pblk = match alloc_block(&sb) {
                Ok(b) => b,
                Err(_) => {
                    /* Persist the partial extension (zero-filled) and fail.
                       No corruption: allocated blocks are attached + sized. */
                    let covered = (lb as u64) * bs as u64;
                    let new_size = core::cmp::max(size, core::cmp::min(end, covered));
                    inode.i_size_lo = new_size as u32;
                    inode.i_size_high = (new_size >> 32) as u32;
                    let t = (crate::time::realtime_ns() / 1_000_000_000) as u32;
                    inode.i_mtime = t;
                    inode.i_ctime = t;
                    let _ = write_inode(&sb, inum, &inode);
                    return Err(errno::ENOSPC);
                }
            };
            if write_block(pblk, bs, &zero[..bs.min(4096)]).is_err() {
                return Err(errno::EIO);
            }
            if extent_append(&sb, &mut inode, lb as u32, pblk).is_err() {
                return Err(errno::ENOSPC);
            }
            inode.i_blocks_lo = inode.i_blocks_lo.saturating_add((bs / 512) as u32);
        }
        let new_size = core::cmp::max(size, end);
        inode.i_size_lo = new_size as u32;
        inode.i_size_high = (new_size >> 32) as u32;
    }
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

/* ================= Stage B1: block allocation + extent growth ========== */
/* Synchronous, journal-less allocator (crash consistency is a later track;
   all metadata is written before returning so clean e2fsck passes). */
fn write_superblock(sb: &Super) -> Result<(), i64> {
    let mut buf = [0u8; 1024];
    read_bytes(1024, &mut buf)?;
    let src = unsafe {
        core::slice::from_raw_parts(
            (sb as *const Super) as *const u8,
            core::mem::size_of::<Super>(),
        )
    };
    buf[..src.len()].copy_from_slice(src);
    write_bytes_at(1024, &buf)?;
    Ok(())
}
fn write_group_desc(sb: &Super, g: usize, gd: &GroupDesc) -> Result<(), i64> {
    let bs = block_size(sb);
    let mut buf = [0u8; 4096];
    read_block(group_desc_block(sb), bs, &mut buf)?;
    let off = g * 32;
    let dst = &mut buf[off..off + core::mem::size_of::<GroupDesc>()];
    let src = unsafe {
        core::slice::from_raw_parts(
            (gd as *const GroupDesc) as *const u8,
            core::mem::size_of::<GroupDesc>(),
        )
    };
    dst.copy_from_slice(src);
    write_block(group_desc_block(sb), bs, &buf)?;
    Ok(())
}
/* First-fit block allocator: scans groups for a free bit, sets it, and
   debits group + superblock free counts. Returns the filesystem block. */
fn alloc_block(sb: &Super) -> Result<u64, i64> {
    let bs = block_size(sb);
    let ngroups = groups_count(sb);
    let blocks_per_group = sb.s_blocks_per_group as u64;
    let total_blocks = sb.s_blocks_count_lo as u64;
    let mut sb_mut = superblock()?;
    for g in 0..ngroups {
        let mut gd = read_group_desc(&sb_mut, g)?;
        if gd.bg_free_blocks_count_lo == 0 {
            continue;
        }
        let bitmap_blk = gd.bg_block_bitmap_lo as u64;
        let mut bmp = [0u8; 4096];
        read_block(bitmap_blk, bs, &mut bmp)?;
        let base = g as u64 * blocks_per_group;
        /* Block 0 of group 0 is never allocatable (superblock); start after
           s_first_data_block on group 0. */
        let start_bit = if g == 0 { sb.s_first_data_block as usize } else { 0 };
        let max_bit = core::cmp::min(blocks_per_group as usize, (total_blocks - base) as usize);
        let map_len = core::cmp::min(bs, (max_bit + 7) / 8);
        for i in start_bit..max_bit {
            if bmp[i / 8] & (1 << (i % 8)) == 0 {
                bmp[i / 8] |= 1 << (i % 8);
                write_block(bitmap_blk, bs, &bmp)?;
                gd.bg_free_blocks_count_lo -= 1;
                write_group_desc(&sb_mut, g, &gd)?;
                sb_mut.s_free_blocks_count_lo -= 1;
                write_superblock(&sb_mut)?;
                return Ok(base + i as u64);
            }
            if i / 8 >= map_len {
                break;
            }
        }
    }
    Err(errno::ENOSPC)
}
/* Appends one physical block to the file's extent tree at logical block lb
   (depth-0 inline only: grows the last extent if contiguous, else adds a
   new one; fails ENOSPC when the 4 inline slots are exhausted). */
fn extent_append(sb: &Super, inode: &mut Inode, lb: u32, pblk: u64) -> Result<(), i64> {
    if pblk > u32::MAX as u64 {
        return Err(errno::ENOSPC);
    }
    let iblk = unsafe {
        core::slice::from_raw_parts_mut(
            inode.i_block.as_mut_ptr() as *mut u8,
            core::mem::size_of::<u32>() * 15,
        )
    };
    let mut h = read_header(&iblk[..12])?;
    if h.eh_depth != 0 {
        return Err(errno::ENOSPC);
    }
    if h.eh_entries > 4 {
        return Err(errno::EINVAL);
    }
    /* Grow last extent if this block continues it. */
    if h.eh_entries > 0 {
        let last_off = 12 + (h.eh_entries as usize - 1) * 12;
        let e = unsafe { &mut *(iblk[last_off..].as_mut_ptr() as *mut Extent) };
        let e_len = (e.ee_len & 0x7fff) as u64;
        let e_start = ((e.ee_start_hi as u64) << 32) | e.ee_start_lo as u64;
        if e.ee_block + e_len as u32 == lb && e_start + e_len == pblk && e_len < 0x7fff {
            e.ee_len += 1;
            return Ok(());
        }
    }
    if h.eh_entries >= h.eh_max || h.eh_entries >= 4 {
        return Err(errno::ENOSPC);
    }
    let off = 12 + h.eh_entries as usize * 12;
    let e = unsafe { &mut *(iblk[off..].as_mut_ptr() as *mut Extent) };
    e.ee_block = lb;
    e.ee_len = 1;
    e.ee_start_hi = (pblk >> 32) as u16;
    e.ee_start_lo = pblk as u32;
    h.eh_entries += 1;
    iblk[2..4].copy_from_slice(&h.eh_entries.to_ne_bytes());
    Ok(())
}

/* ================= Stage B1b: create/unlink/mkdir/rename/truncate ====== */
/* Path helpers (parent dir + basename, ext4 lookup for inums). */
fn split_parent(path: &str) -> Result<(&str, &str), i64> {
    let t = path.trim_start_matches('/');
    if t.is_empty() || t.contains('\0') {
        return Err(errno::EINVAL);
    }
    match t.rfind('/') {
        Some(i) => {
            let (d, n) = (&t[..i], &t[i + 1..]);
            if n.is_empty() || n.len() > 255 || n.contains('/') {
                return Err(errno::EINVAL);
            }
            Ok((d, n))
        },
        None => {
            if t.len() > 255 {
                return Err(errno::EINVAL);
            }
            Ok(("", t))
        },
    }
}
fn lookup_inum(rel: &str) -> Result<u32, i64> {
    if rel.is_empty() {
        return Ok(2);
    }
    Ok(lookup(rel)?.rdev as u32)
}
/* Inode allocator: first free bit in an inode bitmap with room. */
fn alloc_inode(sb: &Super, is_dir: bool) -> Result<u32, i64> {
    let ngroups = groups_count(sb);
    let per = sb.s_inodes_per_group as usize;
    let bs = block_size(sb);
    let mut sb_mut = superblock()?;
    for g in 0..ngroups {
        let gd = read_group_desc(&sb_mut, g)?;
        if gd.bg_free_inodes_count_lo == 0 {
            continue;
        }
        let mut bmp = [0u8; 4096];
        read_block(gd.bg_inode_bitmap_lo as u64, bs, &mut bmp)?;
        let bits = core::cmp::min(per, bs * 8);
        for bit in 0..bits {
            if bmp[bit / 8] & (1 << (bit % 8)) != 0 {
                continue;
            }
            bmp[bit / 8] |= 1 << (bit % 8);
            write_block(gd.bg_inode_bitmap_lo as u64, bs, &bmp)?;
            let mut gd2 = gd;
            gd2.bg_free_inodes_count_lo -= 1;
            if is_dir {
                gd2.bg_used_dirs_count_lo += 1;
            }
            write_group_desc(&sb_mut, g, &gd2)?;
            sb_mut.s_free_inodes_count -= 1;
            write_superblock(&sb_mut)?;
            return Ok(g as u32 * per as u32 + bit as u32 + 1);
        }
    }
    Err(errno::ENOSPC)
}
fn free_inode(sb: &Super, inum: u32, is_dir: bool) -> Result<(), i64> {
    if inum == 0 {
        return Err(errno::EINVAL);
    }
    let g = ((inum - 1) as usize) / sb.s_inodes_per_group as usize;
    let idx = ((inum - 1) as usize) % sb.s_inodes_per_group as usize;
    let gd = read_group_desc(sb, g)?;
    let bs = block_size(sb);
    let mut bmp = [0u8; 4096];
    read_block(gd.bg_inode_bitmap_lo as u64, bs, &mut bmp)?;
    bmp[idx / 8] &= !(1 << (idx % 8));
    write_block(gd.bg_inode_bitmap_lo as u64, bs, &bmp)?;
    let mut gd2 = gd;
    gd2.bg_free_inodes_count_lo += 1;
    if is_dir && gd2.bg_used_dirs_count_lo > 0 {
        gd2.bg_used_dirs_count_lo -= 1;
    }
    write_group_desc(sb, g, &gd2)?;
    let mut sb_mut = superblock()?;
    sb_mut.s_free_inodes_count += 1;
    write_superblock(&sb_mut)?;
    Ok(())
}
/* Directory entry length (8-byte header + aligned name). */
fn dir_entry_len(nlen: usize) -> usize {
    (8 + nlen + 3) & !3
}
/* Frees all data blocks of an inode (extent walk, depth-0 only). */
fn free_all_blocks(sb: &Super, inode: &mut Inode) -> Result<(), i64> {
    let iblk = unsafe {
        core::slice::from_raw_parts(inode.i_block.as_ptr() as *const u8, 60)
    };
    let h = read_header(&iblk[..12])?;
    if h.eh_depth != 0 {
        return Err(errno::ENOSPC);
    }
    let bs = block_size(sb);
    for i in 0..h.eh_entries as usize {
        let off = 12 + i * 12;
        let e = unsafe { &*(iblk[off..].as_ptr() as *const Extent) };
        let start = ((e.ee_start_hi as u64) << 32) | e.ee_start_lo as u64;
        let len = (e.ee_len & 0x7fff) as u64;
        for b in 0..len {
            free_block(sb, start + b)?;
        }
        let _ = bs;
    }
    /* Clear the extent tree (zero header, entries gone). */
    let iblk_mut = unsafe {
        core::slice::from_raw_parts_mut(inode.i_block.as_mut_ptr() as *mut u8, 60)
    };
    for b in iblk_mut.iter_mut() {
        *b = 0;
    }
    /* Re-init empty extent header (magic + max). */
    iblk_mut[0..2].copy_from_slice(&0xF30Au16.to_ne_bytes());
    iblk_mut[4..6].copy_from_slice(&4u16.to_ne_bytes());
    inode.i_blocks_lo = 0;
    Ok(())
}
fn free_block(sb: &Super, blk: u64) -> Result<(), i64> {
    let bs = block_size(sb);
    let ngroups = groups_count(sb);
    let blocks_per_group = sb.s_blocks_per_group as u64;
    for g in 0..ngroups {
        let base = g as u64 * blocks_per_group;
        if blk < base || blk >= base + blocks_per_group {
            continue;
        }
        let bit = (blk - base) as usize;
        let gd = read_group_desc(sb, g)?;
        let mut bmp = [0u8; 4096];
        read_block(gd.bg_block_bitmap_lo as u64, bs, &mut bmp)?;
        bmp[bit / 8] &= !(1 << (bit % 8));
        write_block(gd.bg_block_bitmap_lo as u64, bs, &bmp)?;
        let mut gd2 = gd;
        gd2.bg_free_blocks_count_lo += 1;
        write_group_desc(sb, g, &gd2)?;
        let mut sb_mut = superblock()?;
        sb_mut.s_free_blocks_count_lo += 1;
        write_superblock(&sb_mut)?;
        return Ok(());
    }
    Err(errno::EINVAL)
}
/* Inserts a directory entry (slack reuse, else appends a new dir block). */
fn dir_insert(dir_inum: u32, name: &[u8], child: u32, ftype: u8) -> Result<(), i64> {
    if name.is_empty() || name.len() > 255 || name.contains(&b'/') {
        return Err(errno::EINVAL);
    }
    let sb = superblock()?;
    let bs = block_size(&sb);
    let need = dir_entry_len(name.len());
    let mut dir = read_inode(&sb, dir_inum)?;
    if dir.i_mode & 0o170000 != 0o040000 {
        return Err(errno::ENOTDIR);
    }
    let size = inode_len(&dir) as usize;
    let nblocks = (size + bs - 1) / bs;
    for bi in 0..nblocks {
        let blk = read_file_block(&sb, &dir, bi as u32)?;
        if blk == 0 {
            continue;
        }
        let mut block = [0u8; 4096];
        read_block(blk, bs, &mut block)?;
        let mut off = 0usize;
        while off + 8 <= bs {
            let ino = u32::from_le_bytes(block[off..off + 4].try_into().unwrap());
            let rec = u16::from_le_bytes(block[off + 4..off + 6].try_into().unwrap()) as usize;
            let nl = block[off + 6] as usize;
            if rec == 0 || off + rec > bs {
                break;
            }
            if ino != 0 && nl == name.len() && &block[off + 8..off + 8 + nl] == name {
                return Err(errno::EEXIST);
            }
            let actual = if ino == 0 { 8 } else { dir_entry_len(nl) };
            if rec >= actual && rec - actual >= need {
                let no = if ino == 0 { off } else { off + actual };
                let nrec = if ino == 0 { rec } else { rec - actual };
                if ino != 0 {
                    block[off + 4..off + 6].copy_from_slice(&(actual as u16).to_ne_bytes());
                }
                block[no..no + 4].copy_from_slice(&child.to_le_bytes());
                block[no + 4..no + 6].copy_from_slice(&(nrec as u16).to_ne_bytes());
                block[no + 6] = name.len() as u8;
                block[no + 7] = ftype;
                block[no + 8..no + 8 + name.len()].copy_from_slice(name);
                write_block(blk, bs, &block)?;
                return Ok(());
            }
            if rec == 0 {
                break;
            }
            off += rec;
        }
    }
    /* Append a new directory block (single spanning entry). */
    let pblk = alloc_block(&sb)?;
    let zero = [0u8; 4096];
    write_block(pblk, bs, &zero[..bs.min(4096)])?;
    let lb = nblocks as u32;
    extent_append(&sb, &mut dir, lb, pblk)?;
    dir.i_blocks_lo = dir.i_blocks_lo.saturating_add((bs / 512) as u32);
    let new_size = ((nblocks + 1) * bs) as u64;
    dir.i_size_lo = new_size as u32;
    dir.i_size_high = (new_size >> 32) as u32;
    let t = (crate::time::realtime_ns() / 1_000_000_000) as u32;
    dir.i_mtime = t;
    dir.i_ctime = t;
    write_inode(&sb, dir_inum, &dir)?;
    let mut block = [0u8; 4096];
    block[0..4].copy_from_slice(&child.to_le_bytes());
    block[4..6].copy_from_slice(&(bs as u16).to_ne_bytes());
    block[6] = name.len() as u8;
    block[7] = ftype;
    block[8..8 + name.len()].copy_from_slice(name);
    write_block(pblk, bs, &block)?;
    Ok(())
}
/* Removes a directory entry (marks ino=0; slack reused by future inserts). */
fn dir_remove(dir_inum: u32, name: &[u8]) -> Result<u32, i64> {
    let sb = superblock()?;
    let bs = block_size(&sb);
    let dir = read_inode(&sb, dir_inum)?;
    if dir.i_mode & 0o170000 != 0o040000 {
        return Err(errno::ENOTDIR);
    }
    let size = inode_len(&dir) as usize;
    let nblocks = (size + bs - 1) / bs;
    for bi in 0..nblocks {
        let blk = read_file_block(&sb, &dir, bi as u32)?;
        if blk == 0 {
            continue;
        }
        let mut block = [0u8; 4096];
        read_block(blk, bs, &mut block)?;
        let mut off = 0usize;
        while off + 8 <= bs {
            let ino = u32::from_le_bytes(block[off..off + 4].try_into().unwrap());
            let rec = u16::from_le_bytes(block[off + 4..off + 6].try_into().unwrap()) as usize;
            let nl = block[off + 6] as usize;
            if rec == 0 || off + rec > bs {
                break;
            }
            if ino != 0 && nl == name.len() && &block[off + 8..off + 8 + nl] == name {
                block[off..off + 4].copy_from_slice(&0u32.to_le_bytes());
                write_block(blk, bs, &block)?;
                return Ok(ino);
            }
            off += rec;
        }
    }
    Err(errno::ENOENT)
}
/* Creates a regular file: alloc inode, init empty extent tree, link. */
pub fn create_file(path: &str, mode: u32) -> Result<crate::vfs::Inode, i64> {
    if !ready() {
        return Err(errno::ENODEV);
    }
    let (dir_rel, name) = split_parent(path)?;
    let dir_inum = lookup_inum(dir_rel)?;
    let sb = superblock()?;
    let inum = alloc_inode(&sb, false)?;
    let t = (crate::time::realtime_ns() / 1_000_000_000) as u32;
    let mut inode = read_inode(&sb, inum)?;
    inode.i_mode = 0o100000 | (mode & 0o7777) as u16;
    inode.i_uid = 0;
    inode.i_gid = 0;
    inode.i_size_lo = 0;
    inode.i_size_high = 0;
    inode.i_atime = t;
    inode.i_ctime = t;
    inode.i_mtime = t;
    inode.i_dtime = 0;
    inode.i_links_count = 1;
    inode.i_blocks_lo = 0;
    inode.i_flags = 0x80000; /* EXTENTS_FL */
    for w in inode.i_block.iter_mut() {
        *w = 0;
    }
    /* Empty extent header (magic, max 4 inline). */
    let iblk = unsafe {
        core::slice::from_raw_parts_mut(inode.i_block.as_mut_ptr() as *mut u8, 60)
    };
    iblk[0..2].copy_from_slice(&0xF30Au16.to_ne_bytes());
    iblk[4..6].copy_from_slice(&4u16.to_ne_bytes());
    write_inode(&sb, inum, &inode)?;
    if let Err(e) = dir_insert(dir_inum, name.as_bytes(), inum, 1) {
        let _ = free_inode(&superblock()?, inum, false);
        return Err(e);
    }
    Ok(make_vfs_inode(path, inum, &inode, &sb))
}
/* Unlinks a regular file: remove dir entry, free blocks + inode. */
pub fn unlink_path(path: &str) -> Result<(), i64> {
    if !ready() {
        return Err(errno::ENODEV);
    }
    let (dir_rel, name) = split_parent(path)?;
    let dir_inum = lookup_inum(dir_rel)?;
    let inum = dir_remove(dir_inum, name.as_bytes())?;
    let sb = superblock()?;
    let mut inode = read_inode(&sb, inum)?;
    if inode.i_mode & 0o170000 == 0o040000 {
        /* Restore entry (rmdir must be used); best-effort reinsert. */
        let _ = dir_insert(dir_inum, name.as_bytes(), inum, 2);
        return Err(errno::EISDIR);
    }
    if inode.i_links_count > 0 {
        inode.i_links_count -= 1;
    }
    let t = (crate::time::realtime_ns() / 1_000_000_000) as u32;
    inode.i_ctime = t;
    if inode.i_links_count == 0 {
        inode.i_dtime = t;
        free_all_blocks(&sb, &mut inode)?;
        write_inode(&sb, inum, &inode)?;
        free_inode(&sb, inum, false)?;
    } else {
        write_inode(&sb, inum, &inode)?;
    }
    Ok(())
}
/* Makes a directory with . and .. entries. */
pub fn mkdir_path(path: &str, mode: u32) -> Result<crate::vfs::Inode, i64> {
    if !ready() {
        return Err(errno::ENODEV);
    }
    let (dir_rel, name) = split_parent(path)?;
    let dir_inum = lookup_inum(dir_rel)?;
    let sb = superblock()?;
    let bs = block_size(&sb);
    let inum = alloc_inode(&sb, true)?;
    let t = (crate::time::realtime_ns() / 1_000_000_000) as u32;
    let mut inode = read_inode(&sb, inum)?;
    inode.i_mode = 0o040000 | (mode & 0o7777) as u16;
    inode.i_uid = 0;
    inode.i_gid = 0;
    inode.i_size_lo = bs as u32;
    inode.i_size_high = 0;
    inode.i_atime = t;
    inode.i_ctime = t;
    inode.i_mtime = t;
    inode.i_dtime = 0;
    inode.i_links_count = 2;
    inode.i_flags = 0x80000;
    for w in inode.i_block.iter_mut() {
        *w = 0;
    }
    let iblk = unsafe {
        core::slice::from_raw_parts_mut(inode.i_block.as_mut_ptr() as *mut u8, 60)
    };
    iblk[0..2].copy_from_slice(&0xF30Au16.to_ne_bytes());
    iblk[4..6].copy_from_slice(&4u16.to_ne_bytes());
    /* Allocate the first directory block (. and ..). */
    let pblk = match alloc_block(&sb) {
        Ok(b) => b,
        Err(e) => {
            let _ = free_inode(&superblock()?, inum, true);
            return Err(e);
        },
    };
    if extent_append(&sb, &mut inode, 0, pblk).is_err() {
        let _ = free_inode(&superblock()?, inum, true);
        return Err(errno::ENOSPC);
    }
    inode.i_blocks_lo = (bs / 512) as u32;
    write_inode(&sb, inum, &inode)?;
    let mut block = [0u8; 4096];
    /* "." entry (rec_len sized to leave room for ".."). */
    let dot_len = dir_entry_len(1);
    block[0..4].copy_from_slice(&inum.to_le_bytes());
    block[4..6].copy_from_slice(&(dot_len as u16).to_ne_bytes());
    block[6] = 1;
    block[7] = 2;
    block[8] = b'.';
    /* ".." entry (spans rest of block). */
    let off2 = dot_len;
    block[off2..off2 + 4].copy_from_slice(&dir_inum.to_le_bytes());
    block[off2 + 4..off2 + 6].copy_from_slice(&((bs - off2) as u16).to_ne_bytes());
    block[off2 + 6] = 2;
    block[off2 + 7] = 2;
    block[off2 + 8..off2 + 10].copy_from_slice(b"..");
    write_block(pblk, bs, &block)?;
    if let Err(e) = dir_insert(dir_inum, name.as_bytes(), inum, 2) {
        let _ = free_inode(&superblock()?, inum, true);
        return Err(e);
    }
    /* Parent gains a subdirectory link. */
    if let Ok(mut parent) = read_inode(&sb, dir_inum) {
        parent.i_links_count += 1;
        let _ = write_inode(&sb, dir_inum, &parent);
    }
    Ok(make_vfs_inode(path, inum, &inode, &sb))
}
/* Removes an empty directory (only . and .. allowed). */
pub fn rmdir_path(path: &str) -> Result<(), i64> {
    if !ready() {
        return Err(errno::ENODEV);
    }
    let (dir_rel, name) = split_parent(path)?;
    let dir_inum = lookup_inum(dir_rel)?;
    let sb = superblock()?;
    /* Verify empty (only . and ..). */
    let inum = {
        let blk0 = {
            let dir = read_inode(&sb, dir_inum)?;
            let size = inode_len(&dir) as usize;
            let bs = block_size(&sb);
            let nblocks = (size + bs - 1) / bs;
            let mut found = None;
            for bi in 0..nblocks {
                let blk = read_file_block(&sb, &dir, bi as u32)?;
                if blk == 0 {
                    continue;
                }
                let mut block = [0u8; 4096];
                read_block(blk, bs, &mut block)?;
                let mut off = 0usize;
                while off + 8 <= bs {
                    let ino = u32::from_le_bytes(block[off..off + 4].try_into().unwrap());
                    let rec = u16::from_le_bytes(block[off + 4..off + 6].try_into().unwrap()) as usize;
                    let nl = block[off + 6] as usize;
                    if rec == 0 || off + rec > bs {
                        break;
                    }
                    if ino != 0 && nl == name.len() && &block[off + 8..off + 8 + nl] == name.as_bytes() {
                        found = Some(ino);
                        break;
                    }
                    off += rec;
                }
                if found.is_some() {
                    break;
                }
            }
            found.ok_or(errno::ENOENT)?
        };
        blk0
    };
    let mut inode = read_inode(&sb, inum)?;
    if inode.i_mode & 0o170000 != 0o040000 {
        return Err(errno::ENOTDIR);
    }
    /* Count entries; fail if beyond . and ... */
    let size = inode_len(&inode) as usize;
    let bs = block_size(&sb);
    let nblocks = (size + bs - 1) / bs;
    let mut entries = 0u32;
    for bi in 0..nblocks {
        let blk = read_file_block(&sb, &inode, bi as u32)?;
        if blk == 0 {
            continue;
        }
        let mut block = [0u8; 4096];
        read_block(blk, bs, &mut block)?;
        let mut off = 0usize;
        while off + 8 <= bs {
            let ino = u32::from_le_bytes(block[off..off + 4].try_into().unwrap());
            let rec = u16::from_le_bytes(block[off + 4..off + 6].try_into().unwrap()) as usize;
            if rec == 0 || off + rec > bs {
                break;
            }
            if ino != 0 {
                entries += 1;
            }
            off += rec;
        }
    }
    if entries > 2 {
        return Err(errno::ENOTEMPTY);
    }
    dir_remove(dir_inum, name.as_bytes())?;
    free_all_blocks(&sb, &mut inode)?;
    write_inode(&sb, inum, &inode)?;
    free_inode(&sb, inum, true)?;
    if let Ok(mut parent) = read_inode(&sb, dir_inum) {
        if parent.i_links_count > 0 {
            parent.i_links_count -= 1;
            let _ = write_inode(&sb, dir_inum, &parent);
        }
    }
    Ok(())
}
/* Renames (files only in v1; dirs fail EXDEV to keep link-count logic simple
   and honest). Overwrites an existing file target atomically-ish. */
pub fn rename_path(old: &str, new: &str) -> Result<(), i64> {
    if !ready() {
        return Err(errno::ENODEV);
    }
    let (odir_rel, oname) = split_parent(old)?;
    let (ndir_rel, nname) = split_parent(new)?;
    let odir = lookup_inum(odir_rel)?;
    let ndir = lookup_inum(ndir_rel)?;
    let sb = superblock()?;
    let inum = dir_remove(odir, oname.as_bytes())?;
    let inode = read_inode(&sb, inum)?;
    if inode.i_mode & 0o170000 == 0o040000 {
        let _ = dir_insert(odir, oname.as_bytes(), inum, 2);
        return Err(errno::EXDEV);
    }
    let ftype = 1u8;
    /* If target exists and is a file, unlink it first (overwrite). */
    let sb2 = superblock()?;
    let mut dir = read_inode(&sb2, ndir)?;
    if dir.i_mode & 0o170000 != 0o040000 {
        let _ = dir_insert(odir, oname.as_bytes(), inum, ftype);
        return Err(errno::ENOTDIR);
    }
    /* Best-effort overwrite: remove existing target file if present. */
    let _ = unlink_path(new);
    if let Err(e) = dir_insert(ndir, nname.as_bytes(), inum, ftype) {
        let _ = dir_insert(odir, oname.as_bytes(), inum, ftype);
        return Err(e);
    }
    Ok(())
}
/* Truncates (shrink frees tail blocks; grow sets size, holes read as zeros). */
pub fn truncate_file(inum: u32, size: u64) -> Result<(), i64> {
    if !ready() {
        return Err(errno::ENODEV);
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
    let bs = block_size(&sb);
    let cur = inode_len(&inode);
    if size == cur {
        return Ok(());
    }
    if size > cur {
        inode.i_size_lo = size as u32;
        inode.i_size_high = (size >> 32) as u32;
        let t = (crate::time::realtime_ns() / 1_000_000_000) as u32;
        inode.i_mtime = t;
        inode.i_ctime = t;
        write_inode(&sb, inum, &inode)?;
        return Ok(());
    }
    /* Shrink: walk extents, free blocks at/after the cut, trim last extent. */
    let keep_lb = ((size as usize + bs - 1) / bs) as u32;
    let iblk = unsafe {
        core::slice::from_raw_parts_mut(inode.i_block.as_mut_ptr() as *mut u8, 60)
    };
    let h = read_header(&iblk[..12])?;
    if h.eh_depth != 0 {
        return Err(errno::ENOSPC);
    }
    let mut kept_sectors = 0u32;
    let mut new_entries = 0u16;
    for i in 0..h.eh_entries as usize {
        let off = 12 + i * 12;
        let e = unsafe { &mut *(iblk[off..].as_mut_ptr() as *mut Extent) };
        let eb = e.ee_block;
        let elen = (e.ee_len & 0x7fff) as u32;
        let estart = ((e.ee_start_hi as u64) << 32) | e.ee_start_lo as u64;
        if eb + elen <= keep_lb {
            kept_sectors += elen * (bs / 512) as u32;
            new_entries += 1;
            continue;
        }
        if eb < keep_lb {
            /* Partial keep: trim length, free the tail. */
            let keep = keep_lb - eb;
            for b in keep..elen {
                let _ = free_block(&sb, estart + b as u64);
            }
            e.ee_len = keep as u16;
            kept_sectors += keep * (bs / 512) as u32;
            new_entries += 1;
        } else {
            for b in 0..elen {
                let _ = free_block(&sb, estart + b as u64);
            }
        }
    }
    /* Compact: move kept extents down (they're already ordered; just fix count). */
    iblk[2..4].copy_from_slice(&new_entries.to_ne_bytes());
    inode.i_blocks_lo = kept_sectors;
    inode.i_size_lo = size as u32;
    inode.i_size_high = (size >> 32) as u32;
    let t = (crate::time::realtime_ns() / 1_000_000_000) as u32;
    inode.i_mtime = t;
    inode.i_ctime = t;
    write_inode(&sb, inum, &inode)?;
    Ok(())
}
