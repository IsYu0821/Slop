#ifndef _SLOP_TIME_H
#define _SLOP_TIME_H

#include <stddef.h>

struct timespec {
    long tv_sec;
    long tv_nsec;
};

int nanosleep(const struct timespec *req, struct timespec *rem);

#endif
