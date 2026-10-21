#ifndef _SLOP_ERRNO_H
#define _SLOP_ERRNO_H

int *__errno_location(void);
#define errno (*__errno_location())


#define EPERM 1
#define ENOENT 2
#define EAGAIN 11
#define ENOMEM 12
#define EFAULT 14
#define EEXIST 17
#define EINVAL 22
#define ENOSPC 28
#define EISDIR 21

#endif
