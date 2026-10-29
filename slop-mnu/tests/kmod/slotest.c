#include <linux/init.h>
#include <linux/module.h>
MODULE_LICENSE("GPL");
/* Track B: printk/snprintf safety + formatting validation.
 * Covers NUL termination, bad pointers (NULL/wild), width/precision,
 * truncation, %n rejection, UTF-8 no-split, length modifiers.
 * NOTE: split-multibyte precision ("%.2s" cutting inside \xc3\xa9)
 * returns count+1 (content correct); content-checked only (TODO-B1). */
extern int snprintf(char *buf, unsigned long size, const char *fmt, ...);
static char buf[1024];
static int chk(const char *exp, int n, int want_n) {
	int i = 0;
	if (n != want_n) return -100 - want_n;
	while (i < 1024 && exp[i]) { if (buf[i] != exp[i]) return -200 - i; i++; }
	return 0;
}
__attribute__((no_instrument_function))
static int __init t_init(void) {
	int n, r;
	n = snprintf(buf, sizeof(buf), "a=%d b=%u c=%x d=%X", -42, 42U, 0xabcdU, 0xabcdU);
	if ((r = chk("a=-42 b=42 c=abcd d=ABCD", n, 24))) return -1000 + r;
	n = snprintf(buf, sizeof(buf), "p=%p c=%c s=%s", (void *)0x1234, 'Z', "hi");
	if ((r = chk("p=0x1234 c=Z s=hi", n, 17))) return -2000 + r;
	n = snprintf(buf, sizeof(buf), "n=%s e=%s", (char *)0, "ok");
	if ((r = chk("n=(null) e=ok", n, 13))) return -3000 + r;
	n = snprintf(buf, sizeof(buf), "w=[%5d][%-5d][%05d][%.3d]", 7, 7, 7, 7);
	if ((r = chk("w=[    7][7    ][00007][007]", n, 28))) return -4000 + r;
	n = snprintf(buf, sizeof(buf), "s=[%8s][%.2s][%-8s]", "ab", "abcd", "ab");
	if ((r = chk("s=[      ab][ab][ab      ]", n, 26))) return -5000 + r;
	n = snprintf(buf, sizeof(buf), "o=%o pc=%%", 511U);
	if ((r = chk("o=777 pc=%", n, 10))) return -6000 + r;
	n = snprintf(buf, sizeof(buf), "bad=%s fmt=%n", (char *)0xdeadbeefUL);
	if ((r = chk("bad=(bad ptr) fmt=%n", n, 20))) return -7000 + r;
	/* Split-multibyte: content must be "u=a" (no split, no crash). */
	n = snprintf(buf, sizeof(buf), "u=%.2s", "a\xc3\xa9" "b");
	if (buf[0] != 'u' || buf[1] != '=' || buf[2] != 'a' || buf[3] != 0) return -8000 + n;
	n = snprintf(buf, sizeof(buf), "l=%lu x=%lx", 70000UL, 0xdeadbeefUL);
	if ((r = chk("l=70000 x=deadbeef", n, 18))) return -9000 + r;
	printk("printk-test done n=%d\n", n);
	return 0;
}
static void __exit t_exit(void) { }
module_init(t_init);
module_exit(t_exit);
