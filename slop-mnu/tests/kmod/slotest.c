#include <linux/init.h>
#include <linux/module.h>
MODULE_LICENSE("GPL");
extern unsigned long test_store_addr(void);
extern void abi_probe_6(unsigned long a0, unsigned long a1, unsigned long a2,
			unsigned long a3, unsigned long a4, unsigned long a5);
extern int abi_verify(void);
struct abi_exp { unsigned long a0,a1,a2,a3,a4,a5,s0,s1; };
static int __init t_init(void) {
	struct abi_exp *e = (struct abi_exp *)test_store_addr();
	e->a0 = 0x1111111111111111UL; e->a1 = 0x2222222222222222UL;
	e->a2 = 0x3333333333333333UL; e->a3 = 0x4444444444444444UL;
	e->a4 = 0x5555555555555555UL; e->a5 = 0x6666666666666666UL;
	e->s0 = 0; e->s1 = 0;
	abi_probe_6(0x1111111111111111UL, 0x2222222222222222UL, 0x3333333333333333UL,
		    0x4444444444444444UL, 0x5555555555555555UL, 0x6666666666666666UL);
	return abi_verify();
}
static void __exit t_exit(void) { }
module_init(t_init);
module_exit(t_exit);
