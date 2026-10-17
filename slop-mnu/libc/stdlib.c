#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/mman.h>
#include "slop_internal.h"

/* First-fit free-list allocator over anonymous mappings. Block headers live
   inside the mapped chunk so free() can coalesce neighbours. */
struct block {
    size_t size;
    struct block *next;
    int free;
    size_t pad;
};

static struct block *arena;
char **environ;

static struct block *grow(size_t need) {
    size_t want = (need + sizeof(struct block) + 4095UL) & ~4095UL;
    void *mem = mmap(0, want, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (mem == (void *)-1) return 0;
    struct block *b = mem;
    b->size = want - sizeof(struct block);
    b->free = 1;
    b->next = arena;
    arena = b;
    return b;
}

void *malloc(size_t n) {
    struct block *b;
    if (n == 0) n = 1;
    n = (n + 15UL) & ~15UL;
    for (b = arena; b; b = b->next) {
        if (!b->free || b->size < n) continue;
        if (b->size >= n + sizeof(struct block) + 16) {
            struct block *split = (struct block *)((char *)(b + 1) + n);
            split->size = b->size - n - sizeof(struct block);
            split->next = b->next;
            split->free = 1;
            split->pad = 0;
            b->next = split;
            b->size = n;
        }
        b->free = 0;
        return b + 1;
    }
    b = grow(n);
    if (!b) return 0;
    b->free = 0;
    return b + 1;
}

void free(void *p) {
    struct block *b;
    if (!p) return;
    b = (struct block *)p - 1;
    b->free = 1;
    for (b = arena; b && b->next; b = b->next) {
        if (b->free && b->next->free &&
            (char *)b + sizeof(struct block) + b->size == (char *)b->next) {
            b->size += sizeof(struct block) + b->next->size;
            b->next = b->next->next;
        }
    }
}

void *calloc(size_t count, size_t size) {
    size_t total;
    void *p;
    if (count && size > (size_t)-1 / count) return 0;
    total = count * size;
    p = malloc(total);
    if (p) memset(p, 0, total);
    return p;
}

void *realloc(void *p, size_t n) {
    struct block *b;
    void *q;
    if (!p) return malloc(n);
    if (n == 0) {
        free(p);
        return 0;
    }
    b = (struct block *)p - 1;
    if (b->size >= n) return p;
    q = malloc(n);
    if (!q) return 0;
    memcpy(q, p, b->size < n ? b->size : n);
    free(p);
    return q;
}

char *getenv(const char *name) {
    size_t len = strlen(name);
    char **e;
    if (!environ) return 0;
    for (e = environ; *e; e++) {
        if (strncmp(*e, name, len) == 0 && (*e)[len] == '=') return *e + len + 1;
    }
    return 0;
}

void exit(int status) {
    _exit(status);
}
