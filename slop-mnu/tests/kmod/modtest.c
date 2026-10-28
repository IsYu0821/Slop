/* Load /lib/slotest.ko via finit_module(313), print result, unload via
   delete_module(176). Proves unmodified Linux .ko loading end-to-end. */
#define _GNU_SOURCE
#include <stdio.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/syscall.h>
#include <errno.h>
#include <string.h>

#ifndef SYS_finit_module
#define SYS_finit_module 313
#endif
#ifndef SYS_delete_module
#define SYS_delete_module 176
#endif

int main(void) {
    int fd = open("/lib/slotest.ko", O_RDONLY);
    if (fd < 0) { printf("MODTEST_FAIL open %d\n", errno); return 1; }
    long rc = syscall(SYS_finit_module, fd, "", 0);
    if (rc != 0) { printf("MODTEST_FAIL finit %d\n", errno); return 1; }
    printf("MODTEST_OK loaded\n");
    close(fd);
    rc = syscall(SYS_delete_module, "slotest", 0);
    if (rc != 0) { printf("MODTEST_FAIL delete %d\n", errno); return 1; }
    printf("MODTEST_OK unloaded\n");
    return 0;
}
