#include <unistd.h>
#include <fcntl.h>
#include <errno.h>
#include <signal.h>
#include <sys/mman.h>
#include "slop_internal.h"

#define SYS_READ 0
#define SYS_WRITE 1
#define SYS_RT_SIGACTION 13
#define SYS_RT_SIGPROCMASK 14
#define SYS_RT_SIGRETURN 15
#define SYS_CLOSE 3
#define SYS_MMAP 9
#define SYS_DUP2 33
#define SYS_GETPID 39
#define SYS_CLONE 56
#define SYS_EXECVE 59
#define SYS_WAIT4 61
#define SYS_KILL 62
#define SYS_GETPPID 110
#define SYS_OPENAT 257
#define SYS_EXIT_GROUP 231
#define SYS_PIPE2 293
#define SYS_NANOSLEEP 35
#define SYS_UNLINKAT 263
#define SYS_GETDENTS64 217
#define SYS_READV 19
#define SYS_WRITEV 20
#define SYS_PREAD64 17
#define SYS_PWRITE64 18
#define SYS_ACCESS 21
#define SYS_FACCESSAT 269
#define SYS_FACCESSAT2 439
#define SYS_READLINKAT 267
#define SYS_CHDIR 80
#define SYS_FCHDIR 81
#define SYS_RENAME 82
#define SYS_MKDIR 83
#define SYS_RMDIR 84
#define SYS_CHMOD 90
#define SYS_TRUNCATE 76
#define SYS_FTRUNCATE 77
#define SYS_DUP 32
#define SYS_GETTIMEOFDAY 96
#define SYS_GETRLIMIT 97
#define SYS_GETRUSAGE 98
#define SYS_TIMES 100
#define SYS_SELECT 23
#define SYS_UTIMENSAT 280
#define SYS_GETCWD 79
#define SYS_FCHMOD 91
#define SYS_SETRLIMIT 160
#define SYS_MKDIRAT 258
#define SYS_MUNMAP 11
#define SYS_MREMAP 25
#define SYS_MADVISE 28
#define SYS_MEMBARRIER 324
#define AT_FDCWD_C -100

/* POSIX wrapper convention: report -1 and stash the raw errno value. */
static long sys_fail(long r) {
    if (r < 0) {
        errno = (int)-r;
        return -1;
    }
    return r;
}

ssize_t write(int fd, const void *buf, size_t n) {
    return (ssize_t)sys_fail(__slop_syscall6(SYS_WRITE, fd, (long)buf, (long)n, 0, 0, 0));
}

ssize_t read(int fd, void *buf, size_t n) {
    return (ssize_t)sys_fail(__slop_syscall6(SYS_READ, fd, (long)buf, (long)n, 0, 0, 0));
}

int close(int fd) {
    return (int)sys_fail(__slop_syscall6(SYS_CLOSE, fd, 0, 0, 0, 0, 0));
}

int openat(int dirfd, const char *path, int flags, int mode) {
    return (int)sys_fail(__slop_syscall6(SYS_OPENAT, dirfd, (long)path, flags, mode, 0, 0));
}

void *mmap(void *addr, size_t len, int prot, int flags, int fd, long offset) {
    long r = __slop_syscall6(SYS_MMAP, (long)addr, (long)len, prot, flags, fd, offset);
    if (r < 0) return (void *)-1;
    return (void *)r;
}

int pipe(int fds[2]) {
    return (int)sys_fail(__slop_syscall6(SYS_PIPE2, (long)fds, 0, 0, 0, 0, 0));
}

int dup2(int oldfd, int newfd) {
    return (int)sys_fail(__slop_syscall6(SYS_DUP2, oldfd, newfd, 0, 0, 0, 0));
}

pid_t fork(void) {
    /* SIGCHLD clone with no stack or TLS is exactly a process fork. */
    return (pid_t)sys_fail(__slop_syscall6(SYS_CLONE, 17, 0, 0, 0, 0, 0));
}

pid_t getpid(void) {
    return (pid_t)__slop_syscall6(SYS_GETPID, 0, 0, 0, 0, 0, 0);
}

pid_t getppid(void) {
    return (pid_t)__slop_syscall6(SYS_GETPPID, 0, 0, 0, 0, 0, 0);
}

int execve(const char *path, char *const argv[], char *const envp[]) {
    return (int)sys_fail(__slop_syscall6(SYS_EXECVE, (long)path, (long)argv, (long)envp, 0, 0, 0));
}

pid_t waitpid(pid_t pid, int *status, int options) {
    return (pid_t)sys_fail(__slop_syscall6(SYS_WAIT4, pid, (long)status, options, 0, 0, 0));
}

ssize_t readv(int fd, const struct iovec *iov, int iovcnt) {
    return (ssize_t)sys_fail(__slop_syscall6(SYS_READV, fd, (long)iov, iovcnt, 0, 0, 0));
}

ssize_t writev(int fd, const struct iovec *iov, int iovcnt) {
    return (ssize_t)sys_fail(__slop_syscall6(SYS_WRITEV, fd, (long)iov, iovcnt, 0, 0, 0));
}

ssize_t pread64(int fd, void *buf, size_t count, long offset) {
    return (ssize_t)sys_fail(__slop_syscall6(SYS_PREAD64, fd, (long)buf, count, offset, 0, 0));
}

ssize_t pwrite64(int fd, const void *buf, size_t count, long offset) {
    return (ssize_t)sys_fail(__slop_syscall6(SYS_PWRITE64, fd, (long)buf, count, offset, 0, 0));
}

int access(const char *path, int mode) {
    return (int)sys_fail(__slop_syscall6(SYS_ACCESS, (long)path, mode, 0, 0, 0, 0));
}

int faccessat(int dirfd, const char *path, int mode, int flags) {
    return (int)sys_fail(__slop_syscall6(SYS_FACCESSAT2, dirfd, (long)path, mode, flags, 0, 0));
}

long readlinkat(int dirfd, const char *path, char *buf, size_t bufsiz) {
    return sys_fail(__slop_syscall6(SYS_READLINKAT, dirfd, (long)path, (long)buf, bufsiz, 0, 0));
}

void *mremap(void *old_address, size_t old_size, size_t new_size, int flags) {
    long r = sys_fail(__slop_syscall6(SYS_MREMAP, (long)old_address, old_size, new_size, flags, 0, 0));
    if (r < 0) return (void *)-1;
    return (void *)r;
}

int munmap(void *addr, size_t len) {
    return (int)sys_fail(__slop_syscall6(SYS_MUNMAP, (long)addr, len, 0, 0, 0, 0));
}

int madvise(void *addr, size_t len, int advice) {
    return (int)sys_fail(__slop_syscall6(SYS_MADVISE, (long)addr, len, advice, 0, 0, 0));
}

int membarrier(int cmd, int flags) {
    return (int)sys_fail(__slop_syscall6(SYS_MEMBARRIER, cmd, flags, 0, 0, 0, 0));
}

int unlink(const char *path) {
    return (int)sys_fail(__slop_syscall6(SYS_UNLINKAT, AT_FDCWD_C, (long)path, 0, 0, 0, 0));
}

int getdents64(int fd, struct dirent64 *buf, size_t count) {
    return (int)sys_fail(__slop_syscall6(SYS_GETDENTS64, fd, (long)buf, count, 0, 0, 0));
}

int nanosleep(const struct timespec *req, struct timespec *rem) {
    return (int)sys_fail(__slop_syscall6(SYS_NANOSLEEP, (long)req, (long)rem, 0, 0, 0, 0));
}

void _exit(int status) {
    __slop_syscall6(SYS_EXIT_GROUP, status, 0, 0, 0, 0, 0);
    for (;;) {
    }
}


int chdir(const char *path) {
    return (int)sys_fail(__slop_syscall6(SYS_CHDIR, (long)path, 0, 0, 0, 0, 0));
}

int fchdir(int fd) {
    return (int)sys_fail(__slop_syscall6(SYS_FCHDIR, fd, 0, 0, 0, 0, 0));
}

char *getcwd(char *buf, size_t size) {
    long r = __slop_syscall6(SYS_GETCWD, (long)buf, size, 0, 0, 0, 0);
    if (r < 0) { errno = (int)-r; return NULL; }
    return buf;
}

int mkdir(const char *path, int mode) {
    return (int)sys_fail(__slop_syscall6(SYS_MKDIR, (long)path, mode, 0, 0, 0, 0));
}

int rmdir(const char *path) {
    return (int)sys_fail(__slop_syscall6(SYS_RMDIR, (long)path, 0, 0, 0, 0, 0));
}

int rename(const char *old, const char *new) {
    return (int)sys_fail(__slop_syscall6(SYS_RENAME, (long)old, (long)new, 0, 0, 0, 0));
}

int chmod(const char *path, int mode) {
    return (int)sys_fail(__slop_syscall6(SYS_CHMOD, (long)path, mode, 0, 0, 0, 0));
}

int fchmod(int fd, int mode) {
    return (int)sys_fail(__slop_syscall6(SYS_FCHMOD, fd, mode, 0, 0, 0, 0));
}

int truncate(const char *path, long length) {
    return (int)sys_fail(__slop_syscall6(SYS_TRUNCATE, (long)path, length, 0, 0, 0, 0));
}

int ftruncate(int fd, long length) {
    return (int)sys_fail(__slop_syscall6(SYS_FTRUNCATE, fd, length, 0, 0, 0, 0));
}

int dup(int oldfd) {
    return (int)sys_fail(__slop_syscall6(SYS_DUP, oldfd, 0, 0, 0, 0, 0));
}

int gettimeofday(struct timeval *tv, struct timezone *tz) {
    return (int)sys_fail(__slop_syscall6(SYS_GETTIMEOFDAY, (long)tv, (long)tz, 0, 0, 0, 0));
}

int getrlimit(int resource, struct rlimit *rlim) {
    return (int)sys_fail(__slop_syscall6(SYS_GETRLIMIT, resource, (long)rlim, 0, 0, 0, 0));
}

int setrlimit(int resource, const struct rlimit *rlim) {
    return (int)sys_fail(__slop_syscall6(SYS_SETRLIMIT, resource, (long)rlim, 0, 0, 0, 0));
}

int getrusage(int who, void *ru) {
    return (int)sys_fail(__slop_syscall6(SYS_GETRUSAGE, who, (long)ru, 0, 0, 0, 0));
}

long times(void *buf) {
    return (long)sys_fail(__slop_syscall6(SYS_TIMES, (long)buf, 0, 0, 0, 0, 0));
}

int select(int nfds, void *readfds, void *writefds, void *exceptfds, struct timeval *timeout) {
    return (int)sys_fail(__slop_syscall6(SYS_SELECT, nfds, (long)readfds, (long)writefds, (long)exceptfds, (long)timeout, 0));
}

int utimensat(int dirfd, const char *path, const struct timespec times[2], int flags) {
    return (int)sys_fail(__slop_syscall6(SYS_UTIMENSAT, dirfd, (long)path, (long)times, flags, 0, 0));
}

int mkdirat(int dirfd, const char *path, int mode) {
    return (int)sys_fail(__slop_syscall6(SYS_MKDIRAT, dirfd, (long)path, mode, 0, 0, 0));
}

int renameat(int olddirfd, const char *oldpath, int newdirfd, const char *newpath) {
    return (int)sys_fail(__slop_syscall6(264, olddirfd, (long)oldpath, newdirfd, (long)newpath, 0, 0));
}

int fchmodat(int dirfd, const char *path, int mode, int flags) {
    return (int)sys_fail(__slop_syscall6(268, dirfd, (long)path, mode, flags, 0, 0));
}
