#include <linux/init.h>
#include <linux/module.h>
MODULE_LICENSE("GPL");
static int __init t_init(void) { return 0; }
static void __exit t_exit(void) { }
module_init(t_init);
module_exit(t_exit);
