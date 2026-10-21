#ifndef _SLOP_UNISTD_H
#define _SLOP_UNISTD_H

#include <stddef.h>
#include <stdint.h>

/* linux_dirent64 as returned by getdents64. */
struct dirent64 {
    uint64_t d_ino;
    int64_t d_off;
    uint16_t d_reclen;
    uint8_t d_type;
    char d_name[256];
};

#define DT_UNKNOWN 0
#define DT_REG 8
#define DT_DIR 4

#ifndef _SLOP_TIMESPEC_DEFINED
#define _SLOP_TIMESPEC_DEFINED
struct timespec { long tv_sec; long tv_nsec; };
#endif
struct timeval { long tv_sec; long tv_usec; };
struct timezone { int tz_minuteswest; int tz_dsttime; };
typedef long ssize_t;
typedef int pid_t;
struct rlimit { unsigned long rlim_cur; unsigned long rlim_max; };

ssize_t write(int fd, const void *buf, size_t n);
ssize_t read(int fd, void *buf, size_t n);
int close(int fd);
int pipe(int fds[2]);
int dup2(int oldfd, int newfd);
pid_t fork(void);
pid_t getpid(void);
pid_t getppid(void);
int execve(const char *path, char *const argv[], char *const envp[]);
void _exit(int status) __attribute__((noreturn));
int unlink(const char *path);
int getdents64(int fd, struct dirent64 *buf, size_t count);

struct iovec {
    void *iov_base;
    size_t iov_len;
};

#define R_OK 4
#define W_OK 2
#define X_OK 1
#define F_OK 0

#define MREMAP_MAYMOVE 1

#define MADV_NORMAL 0

ssize_t readv(int fd, const struct iovec *iov, int iovcnt);
ssize_t writev(int fd, const struct iovec *iov, int iovcnt);
ssize_t pread64(int fd, void *buf, size_t count, long offset);
ssize_t pwrite64(int fd, const void *buf, size_t count, long offset);
int access(const char *path, int mode);
int faccessat(int dirfd, const char *path, int mode, int flags);
long readlinkat(int dirfd, const char *path, char *buf, size_t bufsiz);
void *mremap(void *old_address, size_t old_size, size_t new_size, int flags);
int madvise(void *addr, size_t len, int advice);
int membarrier(int cmd, int flags);
int nanosleep(const struct timespec *req, struct timespec *rem);


int chdir(const char *path);
int fchdir(int fd);
char *getcwd(char *buf, size_t size);
int mkdir(const char *path, int mode);
int rmdir(const char *path);
int rename(const char *old, const char *new);
int chmod(const char *path, int mode);
int fchmod(int fd, int mode);
int truncate(const char *path, long length);
int ftruncate(int fd, long length);
int dup(int oldfd);
int gettimeofday(struct timeval *tv, struct timezone *tz);
int getrlimit(int resource, struct rlimit *rlim);
int setrlimit(int resource, const struct rlimit *rlim);
int getrusage(int who, void *ru);
long times(void *buf);
int select(int nfds, void *readfds, void *writefds, void *exceptfds, struct timeval *timeout);
int utimensat(int dirfd, const char *path, const struct timespec times[2], int flags);
#endif
