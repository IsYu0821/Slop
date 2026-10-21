#ifndef _SLOP_FCNTL_H
#define _SLOP_FCNTL_H

#define AT_FDCWD (-100)

#define O_RDONLY 0
#define O_WRONLY 1
#define O_RDWR 2
#define O_CREAT 0x40
#define O_DIRECTORY 0x10000

int openat(int dirfd, const char *path, int flags, int mode);

#endif
