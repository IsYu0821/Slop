#ifndef _SLOP_UNISTD_H
#define _SLOP_UNISTD_H

#include <stddef.h>

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
int nanosleep(const struct timespec *req, struct timespec *rem);

#endif
