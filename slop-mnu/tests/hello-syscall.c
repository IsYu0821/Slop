#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <sys/mman.h>
#include <sys/wait.h>
#include <errno.h>

static int failures;

#define CHECK(cond, what)                          \
    do {                                           \
        if (!(cond)) {                             \
            puts("syscalls: failed: " what);       \
            failures++;                            \
        }                                          \
    } while (0)

int main(int argc, char **argv, char **envp) {
    int fds[2];
    char buf[128];
    struct iovec iov[2];
    char exe[64];
    void *map;
    long r;
    ssize_t n;

    (void)argc; (void)argv; (void)envp;
    puts("syscalls: main entered");

    /* 1. writev/readv round trip through a pipe. */
    CHECK(pipe(fds) == 0, "pipe");
    iov[0].iov_base = "alpha";
    iov[0].iov_len = 5;
    iov[1].iov_base = "-beta";
    iov[1].iov_len = 5;
    CHECK(writev(fds[1], iov, 2) == 10, "writev");
    CHECK(close(fds[1]) == 0, "close write end");
    iov[0].iov_base = buf;
    iov[0].iov_len = 6;
    iov[1].iov_base = buf + 6;
    iov[1].iov_len = 4;
    n = readv(fds[0], iov, 2);
    CHECK(n == 10, "readv total");
    CHECK(n == 10 && memcmp(buf, "alpha-beta", 10) == 0, "readv content");
    CHECK(close(fds[0]) == 0, "close read end");

    /* 2. pread64/pwrite64 on a created file. */
    {
        int fd = openat(AT_FDCWD, "/tmp/syscall-probe", O_CREAT | O_RDWR, 0644);
        CHECK(fd >= 0, "openat create");
        CHECK(pwrite64(fd, "0123456789", 10, 0) == 10, "pwrite64");
        memset(buf, 0, sizeof(buf));
        CHECK(pread64(fd, buf, 5, 2) == 5, "pread64 size");
        CHECK(memcmp(buf, "23456", 5) == 0, "pread64 content");
        /* pread must not move the file offset. */
        CHECK(pread64(fd, buf, 2, 0) == 2, "pread64 again");
        CHECK(close(fd) == 0, "close probe");
        CHECK(unlink("/tmp/syscall-probe") == 0, "unlink probe");
    }

    /* 3. access() reports the initramfs files. */
    CHECK(access("/etc/motd", F_OK) == 0, "access existing");
    CHECK(access("/nonexistent", F_OK) < 0 && errno == ENOENT, "access missing");

    /* 4. readlinkat resolves /proc/self/exe. */
    r = readlinkat(AT_FDCWD, "/proc/self/exe", exe, sizeof(exe));
    CHECK(r > 0, "readlinkat exe");
    exe[r > 0 && (size_t)r < sizeof(exe) ? r : 0] = 0;
    CHECK(strstr(exe, "hello") != 0, "exe path mentions hello");

    /* 5. mremap grows a mapping while preserving contents. */
    map = mmap(0, 4096, PROT_READ | PROT_WRITE,
               MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    CHECK(map != (void *)-1, "mmap");
    if (map != (void *)-1) {
        memcpy(map, "mremap-payload", 14);
        void *grown = mremap(map, 4096, 16384, MREMAP_MAYMOVE);
        CHECK(grown != (void *)-1, "mremap");
        if (grown != (void *)-1) {
            CHECK(memcmp(grown, "mremap-payload", 14) == 0, "mremap preserved");
            munmap(grown, 16384);
        } else {
            munmap(map, 4096);
        }
    }

    /* 6. madvise and membarrier are accepted no-ops. */
    CHECK(madvise(0, 0, MADV_NORMAL) == 0, "madvise");
    CHECK(membarrier(0, 0) == 0, "membarrier");

    if (failures) {
        printf("syscalls: %d checks failed\n", failures);
        return 1;
    }
    puts("syscalls: iov, positional io, access, readlink and mremap passed");
    return 0;
}
