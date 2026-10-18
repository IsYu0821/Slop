//! Linux x86-64 syscall numbers. ABI facts only; no Linux implementation code.
pub const READ:u64=0; pub const WRITE:u64=1; pub const IOCTL:u64=16; pub const RT_SIGACTION:u64=13; pub const RT_SIGPROCMASK:u64=14; pub const RT_SIGRETURN:u64=15; pub const CLOSE:u64=3; pub const FSTAT:u64=5;
pub const LSEEK:u64=8; pub const MMAP:u64=9; pub const MPROTECT:u64=10; pub const MUNMAP:u64=11;
pub const BRK:u64=12; pub const PIPE:u64=22; pub const SCHED_YIELD:u64=24; pub const DUP2:u64=33; pub const NANOSLEEP:u64=35;
pub const GETPID:u64=39; pub const CLONE:u64=56; pub const EXECVE:u64=59; pub const EXIT:u64=60; pub const WAIT4:u64=61; pub const KILL:u64=62; pub const UNAME:u64=63;
pub const FCNTL:u64=72; pub const GETCWD:u64=79;
pub const GETUID:u64=102; pub const GETGID:u64=104; pub const GETEUID:u64=107; pub const GETEGID:u64=108;
pub const GETPPID:u64=110; pub const ARCH_PRCTL:u64=158; pub const GETTID:u64=186;
pub const FUTEX:u64=202; pub const CLOCK_GETTIME:u64=228; pub const EXIT_GROUP:u64=231;
pub const OPENAT:u64=257; pub const NEWFSTATAT:u64=262; pub const SET_TID_ADDRESS:u64=218;
pub const SET_ROBUST_LIST:u64=273; pub const DUP3:u64=292; pub const PIPE2:u64=293; pub const PRLIMIT64:u64=302; pub const GETRANDOM:u64=318;
pub const READV:u64=19; pub const WRITEV:u64=20; pub const GETDENTS64:u64=217;
