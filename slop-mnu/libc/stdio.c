#include <stdio.h>
#include <string.h>
#include <unistd.h>

struct outbuf {
    char data[512];
    size_t len;
};

static void emit(struct outbuf *o, char c) {
    if (o->len < sizeof(o->data)) o->data[o->len++] = c;
}

static void emit_str(struct outbuf *o, const char *s) {
    while (*s) emit(o, *s++);
}

static void emit_uint(struct outbuf *o, unsigned long value, unsigned base, int upper) {
    char tmp[32];
    const char *digits = upper ? "0123456789ABCDEF" : "0123456789abcdef";
    int n = 0;
    if (value == 0) tmp[n++] = '0';
    while (value) {
        tmp[n++] = digits[value % base];
        value /= base;
    }
    while (n) emit(o, tmp[--n]);
}

static void format(struct outbuf *o, const char *fmt, va_list ap) {
    for (; *fmt; fmt++) {
        if (*fmt != '%') {
            emit(o, *fmt);
            continue;
        }
        switch (*++fmt) {
        case '%':
            emit(o, '%');
            break;
        case 'c':
            emit(o, (char)va_arg(ap, int));
            break;
        case 's': {
            const char *s = va_arg(ap, const char *);
            emit_str(o, s ? s : "(null)");
            break;
        }
        case 'd': {
            int v = va_arg(ap, int);
            if (v < 0) {
                emit(o, '-');
                emit_uint(o, (unsigned long)(-(long)v), 10, 0);
            } else {
                emit_uint(o, (unsigned long)v, 10, 0);
            }
            break;
        }
        case 'u':
            emit_uint(o, va_arg(ap, unsigned int), 10, 0);
            break;
        case 'x':
            emit_uint(o, va_arg(ap, unsigned int), 16, 0);
            break;
        case 'p':
            emit_str(o, "0x");
            emit_uint(o, (unsigned long)(size_t)va_arg(ap, void *), 16, 0);
            break;
        default:
            emit(o, '%');
            emit(o, *fmt);
            break;
        }
    }
}

int printf(const char *fmt, ...) {
    struct outbuf o;
    va_list ap;
    o.len = 0;
    va_start(ap, fmt);
    format(&o, fmt, ap);
    va_end(ap);
    if (write(1, o.data, o.len) != (ssize_t)o.len) return -1;
    return (int)o.len;
}

int puts(const char *s) {
    size_t n = strlen(s);
    if (write(1, s, n) != (ssize_t)n) return -1;
    if (write(1, "\n", 1) != 1) return -1;
    return 0;
}

int putchar(int c) {
    char b = (char)c;
    if (write(1, &b, 1) != 1) return -1;
    return c;
}
