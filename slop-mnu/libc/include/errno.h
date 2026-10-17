#ifndef _SLOP_ERRNO_H
#define _SLOP_ERRNO_H

int *__errno_location(void);
#define errno (*__errno_location())

#endif
