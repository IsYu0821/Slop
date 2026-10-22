#[repr(C)] #[derive(Clone,Copy,Default)]
pub struct Timespec { pub tv_sec:i64, pub tv_nsec:i64 }
#[repr(C)] #[derive(Clone,Copy,Default)]
pub struct Stat {
    pub st_dev:u64, pub st_ino:u64, pub st_nlink:u64, pub st_mode:u32, pub st_uid:u32,
    pub st_gid:u32, pub __pad0:u32, pub st_rdev:u64, pub st_size:i64, pub st_blksize:i64,
    pub st_blocks:i64, pub st_atime:i64, pub st_atime_nsec:i64, pub st_mtime:i64,
    pub st_mtime_nsec:i64, pub st_ctime:i64, pub st_ctime_nsec:i64, pub __unused:[i64;3],
}
#[repr(C)] #[derive(Clone,Copy,Default)] pub struct Rlimit { pub cur:u64, pub max:u64 }
#[repr(C)] #[derive(Clone,Copy)] pub struct Iovec { pub iov_base:u64, pub iov_len:u64 }
#[repr(C)] #[derive(Clone,Copy)] pub struct Auxv { pub kind:u64, pub value:u64 }
pub const S_IFIFO:u32=0o010000; pub const S_IFCHR:u32=0o020000; pub const S_IFDIR:u32=0o040000;
pub const AT_NULL:u64=0; pub const AT_PAGESZ:u64=6; pub const AT_PHDR:u64=3; pub const AT_BASE:u64=7; pub const AT_ENTRY:u64=9;
pub const AT_PHENT:u64=4; pub const AT_PHNUM:u64=5; 
pub const AT_UID:u64=11; pub const AT_EUID:u64=12; pub const AT_GID:u64=13; pub const AT_EGID:u64=14;
pub const AT_SECURE:u64=23; pub const AT_RANDOM:u64=25; pub const AT_EXECFN:u64=31;
pub const AT_SYSINFO_EHDR:u64=33;
