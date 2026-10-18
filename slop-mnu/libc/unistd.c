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

