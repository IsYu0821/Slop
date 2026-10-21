/* Heap stress: exercises the allocator the way a shell applet does.
 *
 * The test reproduces the allocator setup that musl performs on the first
 * allocation: it reads the current program break, grows it, places a guard
 * page below the heap and then allocates and frees many blocks of different
 * sizes. A kernel that loses or aliases a page inside the heap shows up as a
 * corrupted allocator state and an abnormal exit.
 */
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <stdint.h>
#include <errno.h>
#include <sys/mman.h>
#include <sys/syscall.h>
#include <sys/wait.h>

#define SLOTS 96

static void *slots[SLOTS];
static size_t sizes[SLOTS];

/* The raw break syscall, the way musl's allocator uses it. */
static char *raw_brk(char *addr)
{
    return (char *)syscall(SYS_brk, addr);
}

int main(void)
{
    char *brk_now = raw_brk(0);
    if (brk_now == (char *)-1) {
        printf("heap: brk query failed\n");
        return 1;
    }

    /* This is the setup musl performs on the first allocation: align the
       break, grow it by two pages and put a guard page on the first page of
       the heap with a fixed mapping. */
    char *aligned = (char *)((uintptr_t)brk_now & ~(uintptr_t)4095);
    if (raw_brk(aligned + 8192) == (char *)-1) {
        printf("heap: brk grow failed\n");
        return 1;
    }
    void *guard = mmap(aligned, 4096, PROT_NONE,
                       MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0);
    if (guard == MAP_FAILED) {
        printf("heap: guard mmap failed\n");
        return 1;
    }

    for (int round = 0; round < 200; round++) {
        for (int i = 0; i < SLOTS; i++) {
            size_t n = 1 + (size_t)((round * SLOTS + i) % 997);
            slots[i] = malloc(n);
            if (!slots[i]) {
                printf("heap: malloc failed at round %d slot %d\n", round, i);
                return 2;
            }
            memset(slots[i], i & 0xff, n);
            sizes[i] = n;
        }
        for (int i = 0; i < SLOTS; i++) {
            unsigned char *p = slots[i];
            for (size_t k = 0; k < sizes[i]; k++) {
                if (p[k] != (unsigned char)(i & 0xff)) {
                    printf("heap: block %d corrupted at %zu\n", i, k);
                    return 3;
                }
            }
            free(slots[i]);
            slots[i] = NULL;
        }
    }

    /* A forked child must not disturb the parent heap. */
    for (int i = 0; i < SLOTS; i++) {
        sizes[i] = 64;
        slots[i] = malloc(64);
        if (!slots[i]) {
            printf("heap: parent malloc failed\n");
            return 4;
        }
        memset(slots[i], 0x5a, 64);
    }
    pid_t pid = fork();
    if (pid < 0) {
        printf("heap: fork failed\n");
        return 5;
    }
    if (pid == 0) {
        for (int i = 0; i < SLOTS; i++) {
            memset(slots[i], 0xa5, 64);
            void *q = malloc(4096);
            if (!q) _exit(9);
            memset(q, 0x5a, 4096);
            free(q);
        }
        _exit(0);
    }
    int status = 0;
    if (waitpid(pid, &status, 0) != pid || !WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        printf("heap: child failed status=%d\n", status);
        return 6;
    }
    for (int i = 0; i < SLOTS; i++) {
        unsigned char *p = slots[i];
        for (size_t k = 0; k < 64; k++) {
            if (p[k] != 0x5a) {
                printf("heap: parent block %d corrupted by child\n", i);
                return 7;
            }
        }
        free(slots[i]);
    }

    printf("heap: allocator stress passed\n");
    return 0;
}
