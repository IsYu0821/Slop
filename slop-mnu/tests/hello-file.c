#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <sys/wait.h>
#include <errno.h>

static int failures;

#define CHECK(cond, what)                       \
    do {                                        \
        if (!(cond)) {                          \
            puts("file: failed: " what);        \
            failures++;                         \
        }                                       \
    } while (0)

/* FUTEX_WAIT with a mismatched value must report EAGAIN, not block. */
#define SYS_FUTEX 202
#define FUTEX_WAIT 0
#define FUTEX_WAIT_PRIVATE (FUTEX_WAIT | 0x80)

static long syscall6(long n, long a1, long a2, long a3, long a4, long a5, long a6) {
    long ret;
    __asm__ volatile("syscall"
                     : "=a"(ret)
                     : "a"(n), "D"(a1), "S"(a2), "d"(a3),
                       "r"(a4), "r"(a5), "r"(a6)
                     : "rcx", "r11", "memory");
    return ret;
}

int main(int argc, char **argv, char **envp) {
    int fd;
    char buf[64];
    struct dirent64 d;
    ssize_t n;
    unsigned int observed = 0x1234;
    long r;

    (void)argc; (void)argv; (void)envp;
    puts("file: main entered");

    /* 1. O_CREAT creates the file, write persists it. */
    fd = openat(AT_FDCWD, "/tmp/hello-file", O_CREAT | O_WRONLY, 0644);
    CHECK(fd >= 0, "openat O_CREAT");
    CHECK(write(fd, "file-io-works", 13) == 13, "write created file");
    CHECK(close(fd) == 0, "close write fd");

    /* 2. Reopening and reading returns the bytes we wrote. */
    fd = openat(AT_FDCWD, "/tmp/hello-file", O_RDONLY, 0);
    CHECK(fd >= 0, "openat O_RDONLY");
    n = read(fd, buf, sizeof(buf));
    CHECK(n == 13, "read size");
    CHECK(n == 13 && memcmp(buf, "file-io-works", 13) == 0, "read content");
    CHECK(close(fd) == 0, "close read fd");

    /* 3. unlink removes the node. */
    CHECK(unlink("/tmp/hello-file") == 0, "unlink");
    fd = openat(AT_FDCWD, "/tmp/hello-file", O_RDONLY, 0);
    CHECK(fd < 0 && errno == ENOENT, "gone after unlink");
    if (fd >= 0) close(fd);

    /* 4. getdents64 lists the initramfs /bin directory. */
    fd = openat(AT_FDCWD, "/bin", O_RDONLY | O_DIRECTORY, 0);
    CHECK(fd >= 0, "openat /bin");
    if (fd >= 0) {
        int seen = 0;
        for (;;) {
            r = getdents64(fd, &d, sizeof(d));
            if (r <= 0) break;
            for (long off = 0; off < r; ) {
                struct dirent64 *e = (struct dirent64 *)((char *)&d + off);
                if (strcmp(e->d_name, "hello-signal") == 0) seen = 1;
                if (e->d_reclen == 0) break;
                off += e->d_reclen;
            }
            if (r < (long)sizeof(d)) break;
        }
        CHECK(seen, "getdents64 finds hello-signal");
        CHECK(close(fd) == 0, "close dir fd");
    }

    /* 5. futex FUTEX_WAIT on a mismatched value reports EAGAIN. */
    observed = 1;
    r = syscall6(SYS_FUTEX, (long)&observed, FUTEX_WAIT_PRIVATE,
                 0 /* expected value 0, stored is 1 */, 0, 0, 0);
    CHECK(r == -EAGAIN, "futex wait EAGAIN");
    CHECK(observed == 1, "futex wait left memory untouched");

    if (failures) {
        printf("file: %d checks failed\n", failures);
        return 1;
    }
    puts("file: create, write, read, unlink, getdents64 and futex passed");
    return 0;
}
