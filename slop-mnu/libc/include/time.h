#ifndef _SLOP_TIME_H
#define _SLOP_TIME_H

#include <stddef.h>

#ifndef _SLOP_TIMESPEC_DEFINED
#define _SLOP_TIMESPEC_DEFINED
struct timespec {
    long tv_sec;
    long tv_nsec;
};
#endif

int nanosleep(const struct timespec *req, struct timespec *rem);

#endif
