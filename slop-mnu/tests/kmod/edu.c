#include <linux/init.h>
#include <linux/module.h>
#include <linux/pci.h>
MODULE_LICENSE("GPL");
#define EDU_VID 0x1234
#define EDU_DID 0x11e8
/* QEMU EDU spec: 0x00 ID 0xRRrr00edu, 0x04 liveness INVERTS (~), MSI+INTx. */
static void __iomem *bar;
static void *dbuf;
static dma_addr_t dhandle;
static int edu_probe(struct pci_dev *d, const struct pci_device_id *id) {
	if (pci_enable_device(d)) return -11;
	if (pci_request_regions(d, "edu")) { pci_disable_device(d); return -12; }
	bar = pci_ioremap_bar(d, 0);
	if (!bar) { pci_release_regions(d); pci_disable_device(d); return -13; }
	{
		unsigned int ident = ioread32(bar + 0x00);
		if (ident != 0x010000EDU) return -3;
		iowrite32(0xA5A5A5A5U, bar + 0x04);
		if (ioread32(bar + 0x04) != (unsigned int)~0xA5A5A5A5U) return -2;
		iowrite32(0x00000000U, bar + 0x04);
		printk("edu ident=%x live-ok\n", ident);
	}
	dbuf = dma_alloc_coherent(0, 4096, &dhandle, 0);
	if (!dbuf) return -4;
	if (dhandle == 0) { dma_free_coherent(0, 4096, dbuf, dhandle); return -5; }
	*(volatile unsigned int *)dbuf = 0xDEADBEEFU;
	if (*(volatile unsigned int *)dbuf != 0xDEADBEEFU) { dma_free_coherent(0, 4096, dbuf, dhandle); return -6; }
	printk("edu dma-ok bus=%lx\n", dhandle);
	return 0;
}
static void edu_remove(struct pci_dev *d) {
	if (dbuf) { dma_free_coherent(0, 4096, dbuf, dhandle); dbuf = 0; }
	if (bar) { pci_iounmap(d, bar); bar = 0; }
	pci_release_regions(d);
	pci_disable_device(d);
}
static const struct pci_device_id edu_ids[] = { { EDU_VID, EDU_DID, 0, 0, 0, 0, 0 }, { 0, } };
static struct pci_driver edu_drv = { "edu", edu_ids, edu_probe, edu_remove };
__attribute__((no_instrument_function))
static int __init t_init(void) { return pci_register_driver((void *)&edu_drv); }
static void __exit t_exit(void) { pci_unregister_driver((void *)&edu_drv); }
module_init(t_init);
module_exit(t_exit);
