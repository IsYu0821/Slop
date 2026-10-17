#include <unistd.h>
#include <fcntl.h>
#include <sys/mman.h>
#include "slop_internal.h"

#define SYS_READ 0
#define SYS_WRITE 1
#define SYS_CLOSE 3
#define SYS_MMAP 9
#define SYS_OPENAT 257
#define SYS_EXIT_GROUP 231

ssize_t write(int fd, const void *buf, size_t n) {
    return (ssize_t)__slop_syscall6(SYS_WRITE, fd, (long)buf, (long)n, 0, 0, 0);
}

ssize_t read(int fd, void *buf, size_t n) {
    return (ssize_t)__slop_syscall6(SYS_READ, fd, (long)buf, (long)n, 0, 0, 0);
}

int close(int fd) {
    return (int)__slop_syscall6(SYS_CLOSE, fd, 0, 0, 0, 0, 0);
}

int openat(int dirfd, const char *path, int flags, int mode) {
    return (int)__slop_syscall6(SYS_OPENAT, dirfd, (long)path, flags, mode, 0, 0);
}

void *mmap(void *addr, size_t len, int prot, int flags, int fd, long offset) {
    long r = __slop_syscall6(SYS_MMAP, (long)addr, (long)len, prot, flags, fd, offset);
    if (r < 0) return (void *)-1;
    return (void *)r;
}

void _exit(int status) {
    __slop_syscall6(SYS_EXIT_GROUP, status, 0, 0, 0, 0, 0);
    for (;;) {
    }
}
