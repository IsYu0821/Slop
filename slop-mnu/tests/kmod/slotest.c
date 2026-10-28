#include <linux/init.h>
#include <linux/module.h>
MODULE_LICENSE("GPL");
extern unsigned long test_u64_arg(unsigned long x, unsigned long y);
static int __init t_init(void) {
	unsigned long r = test_u64_arg(0xAAAAAAAAAAAAAAAAUL, 0xBBBBBBBBBBBBBBBBUL);
	return (r == 0xAAAAAAAAAAAAAAAAUL + 0xBBBBBBBBBBBBBBBBUL) ? 0 : -1;
}
static void __exit t_exit(void) { }
module_init(t_init);
module_exit(t_exit);
