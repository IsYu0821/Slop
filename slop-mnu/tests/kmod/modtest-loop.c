// Freestanding load/unload repeat test (no libc). Build: gcc -nostdlib -static -O2.
static long sc6(long n, long a, long b, long c, long d, long e, long f) {
	long r;
	__asm__ volatile ("syscall" : "=a"(r) : "a"(n), "D"(a), "S"(b), "d"(c), "r"(d), "r"(e), "r"(f) : "rcx", "r11", "memory");
	return r;
}
static long sc3(long n, long a, long b, long c) { return sc6(n,a,b,c,0,0,0); }
static void pch(char c) { char ch=c; sc6(1, 1, (long)&ch, 1, 0,0,0); }
static void puts(const char *s) { while (*s) pch(*s++); }
static void putu(unsigned long v) { char t[20]; int l=0; if(!v){pch('0');return;} while(v){t[l++]='0'+(v%10);v/=10;} while(l--)pch(t[l]); }
#ifndef LOOPS
#define LOOPS 100
#endif
void _start(void) {
	int fd = (int)sc6(2, (long)"/lib/slotest.ko", 0, 0, 0,0,0); // open RO
	if (fd < 0) { puts("MODTEST_FAIL open\n"); sc6(60, 1,0,0,0,0,0); }
	for (int i = 0; i < LOOPS; i++) {
		sc6(8, fd, 0, 0, 0, 0, 0); // lseek(fd,0,SEEK_SET): finit reads from current offset
		long rc = sc6(313, fd, (long)"", 0, 0,0,0); // finit
		if (rc != 0) { puts("MODTEST_FAIL finit iter="); putu(i); puts("\n"); sc6(60,1,0,0,0,0,0); }
		rc = sc6(176, (long)"slotest", 0, 0, 0,0,0); // delete
		if (rc != 0) { puts("MODTEST_FAIL delete iter="); putu(i); puts("\n"); sc6(60,1,0,0,0,0,0); }
		if ((i % 10) == 9) { puts("MODTEST_ITER "); putu(i+1); puts(" ok\n"); }
	}
	puts("MODTEST_OK loaded+unloaded\n");
	sc6(60, 0,0,0,0,0,0);
}
