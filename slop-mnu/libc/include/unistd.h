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

struct timespec { long tv_sec; long tv_nsec; };
typedef long ssize_t;
typedef int pid_t;

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
int nanosleep(const struct timespec *req, struct timespec *rem);

#endif
