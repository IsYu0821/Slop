#ifndef _SLOP_STDLIB_H
#define _SLOP_STDLIB_H

#include <stddef.h>

void *malloc(size_t n);
void *calloc(size_t count, size_t size);
void *realloc(void *p, size_t n);
void free(void *p);
char *getenv(const char *name);
void exit(int status);

#endif
