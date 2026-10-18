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
pub const MREMAP:u64=25; pub const MADVISE:u64=28; pub const RT_SIGSUSPEND:u64=130; pub const SIGALTSTACK:u64=131;
pub const WAITID:u64=247; pub const SCHED_GETAFFINITY:u64=204;
pub const READLINKAT:u64=267; pub const FACCESSAT:u64=269; pub const EXECVEAT:u64=322; pub const MEMBARRIER:u64=324;
pub const RSEQ:u64=334; pub const STATX:u64=332; pub const ACCESS:u64=21; pub const FACCESSAT2:u64=439; pub const CLONE3:u64=435; pub const POLL:u64=7; pub const PPOLL:u64=271;
pub const EPOLL_CREATE1:u64=291; pub const EPOLL_CTL:u64=233; pub const EPOLL_WAIT:u64=232;
pub const PKEY_ALLOC:u64=330; pub const TIMER_CREATE:u64=222; pub const GETDENTS:u64=78; pub const PREAD64:u64=17; pub const PWRITE64:u64=18;
pub const READV:u64=19; pub const WRITEV:u64=20; pub const GETDENTS64:u64=217; pub const UNLINKAT:u64=263;
