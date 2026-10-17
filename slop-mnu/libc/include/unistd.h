#ifndef _SLOP_UNISTD_H
#define _SLOP_UNISTD_H

#include <stddef.h>

typedef long ssize_t;

ssize_t write(int fd, const void *buf, size_t n);
ssize_t read(int fd, void *buf, size_t n);
int close(int fd);
void _exit(int status) __attribute__((noreturn));

#endif
