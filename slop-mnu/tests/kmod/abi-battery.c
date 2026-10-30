/* Normal-kbuild 6-register SysV ABI battery (no Slop flags). */
#include <linux/init.h>
#include <linux/module.h>
MODULE_LICENSE("GPL");
extern void abi_probe_6(unsigned long a0, unsigned long a1, unsigned long a2,
			unsigned long a3, unsigned long a4, unsigned long a5);
extern int abi_verify(void);
extern unsigned long test_store_addr(void);
struct abi_exp { unsigned long a0,a1,a2,a3,a4,a5,s0,s1; };
static int __init t_init(void) {
	struct abi_exp *e = (struct abi_exp *)test_store_addr();
	e->a0 = 0x1111111111111111UL; e->a1 = 0x2222222222222222UL;
	e->a2 = 0x3333333333333333UL; e->a3 = 0x4444444444444444UL;
	e->a4 = 0x5555555555555555UL; e->a5 = 0x6666666666666666UL;
	abi_probe_6(e->a0, e->a1, e->a2, e->a3, e->a4, e->a5);
	printk("abi battery probe sent\n");
	return abi_verify();
}
static void __exit t_exit(void) { }
module_init(t_init);
module_exit(t_exit);
/* Track 1 step 2 artifact. Build WITHOUT Slop flags (plain kbuild):
 *   make -C /lib/modules/$(uname -r)/build M=$PWD KBUILD_MODPOST_WARN=1 modules
 * (KBUILD_MODPOST_WARN covers only Slop-private undefined symbols; the code
 * itself is plain -mcmodel=kernel kbuild output.) Load the resulting abi.ko
 * bytes as-is via finit_module; init returns abi_verify() (0 on success:
 * all six argument registers arrived intact through the high-half alias).
 * A vermagic-mismatched copy must be refused with 'vermagic mismatch'. */
