#include <errno.h>

static int slop_errno;

int *__errno_location(void) {
    return &slop_errno;
}
