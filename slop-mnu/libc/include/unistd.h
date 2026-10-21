#ifndef _SLOP_UNISTD_H
#define _SLOP_UNISTD_H

#include <stddef.h>

typedef long ssize_t;
#ifndef _SLOP_PID_T
#define _SLOP_PID_T
typedef int pid_t;
#endif

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

#endif
