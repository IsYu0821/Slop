#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <sys/mman.h>

static int failures;

#define CHECK(cond, what)                     \
    do {                                      \
        if (!(cond)) {                        \
            puts("c runtime: failed: " what); \
            failures++;                       \
        }                                     \
    } while (0)

int main(int argc, char **argv, char **envp) {
    char a[16];
    char b[16];
    char *p;
    char *q;
    char *z;
    char fibuf[64];
    ssize_t n;
    int fd;
    int i;
    int zeroed;
    int written;

    puts("c runtime: main entered");
    CHECK(argc == 2, "argc");
    CHECK(argv != 0 && argv[1] != 0 && strcmp(argv[1], "from-asm") == 0, "argv[1]");
    CHECK(getenv("SLOP_MNU_C") != 0 && strcmp(getenv("SLOP_MNU_C"), "1") == 0, "envp");
    CHECK(envp != 0 && envp[0] != 0, "envp pointer");

    memset(a, 'A', 8);
    a[8] = 0;
    CHECK(strlen(a) == 8, "strlen");
    memcpy(b, a, 9);
    CHECK(memcmp(a, b, 9) == 0, "memcmp");
    CHECK(strcmp("slop", "slop") == 0 && strcmp("slop", "mnu") != 0, "strcmp");
    CHECK(strchr("slop/mnu", '/') != 0 && strchr("slop/mnu", '?') == 0, "strchr");

    p = malloc(40);
    CHECK(p != 0, "malloc");
    if (p) memset(p, 0x5a, 40);
    q = malloc(40);
    CHECK(q != 0, "malloc second");
    free(q);
    q = malloc(40);
    CHECK(q != 0, "malloc reuse");
    zeroed = 1;
    z = calloc(4, 16);
    CHECK(z != 0, "calloc");
    if (z) {
        for (i = 0; i < 64; i++) {
            if (z[i]) zeroed = 0;
        }
    }
    CHECK(zeroed, "calloc zero");
    if (p) {
        for (i = 0; i < 40; i++) {
            if (((unsigned char *)p)[i] != 0x5a) {
                CHECK(0, "malloc content");
                break;
            }
        }
    }
    z = realloc(z, 128);
    CHECK(z != 0, "realloc");
    free(z);
    free(p);
    free(q);

    fd = openat(AT_FDCWD, "/etc/motd", O_RDONLY, 0);
    CHECK(fd >= 0, "openat");
    if (fd >= 0) {
        n = read(fd, fibuf, 63);
        CHECK(n > 0, "read");
        if (n > 0) {
            fibuf[n] = 0;
            CHECK(strcmp(fibuf, "Slop/MNU initramfs\n") == 0, "motd content");
        }
        CHECK(close(fd) == 0, "close");
    }

    written = printf("c runtime: printf %s %d %u %x\n", "works", -7, 42u, 0x2a);
    CHECK(written > 0, "printf");

    if (failures) {
        printf("c runtime: %d checks failed\n", failures);
        return 1;
    }
    puts("c runtime: passed");
    {
        char *av[] = {"hello-posix", "from-c", 0};
        char *ev[] = {"SLOP_MNU_POSIX=1", 0};
        execve("/bin/hello-posix", av, ev);
    }
    puts("c runtime: failed: execve /bin/hello-posix");
    return 1;
}
