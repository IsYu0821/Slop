#include <linux/init.h>
#include <linux/module.h>
#include <linux/pci.h>
#include <linux/string.h>
#include <linux/interrupt.h>
MODULE_LICENSE("GPL");
/* A2/A3: EDU complete vertical - probe/MMIO/DMA/INTx IRQ/remove/unload.
 * INTx via IOAPIC (GSI swizzle in Slop core). Narrow STI only around wait. */
#define EDU_VID 0x1234
#define EDU_DID 0x11e8
#define EDU_SRAM 0x40000UL
static unsigned long bar;
static void *sbuf, *dbuf;
static dma_addr_t shan, dhan;
static unsigned long srb, drb;
static unsigned int irq_vec;
static volatile unsigned int irq_count;
static unsigned long irq_cookie = 0xED1u;
static int irq_registered = 0;
static inline void w32(unsigned long o, unsigned int v) { iowrite32(v, (unsigned long)(bar + o)); }
static inline unsigned int r32(unsigned long o) { return ioread32((unsigned long)(bar + o)); }
static inline void w64(unsigned long o, unsigned long v) {
	iowrite32((unsigned int)v, (unsigned long)(bar + o));
	iowrite32((unsigned int)(v >> 32), (unsigned long)(bar + o + 4));
}
static void spin_relax(unsigned int n) {
	volatile unsigned int z;
	for (z = 0; z < n; z++) { __asm__ volatile ("" ::: "memory"); }
}
static int edu_dma(unsigned long s, unsigned long d, unsigned int n, unsigned int cmd) {
	unsigned int t = 0;
	w64(0x80, s); w64(0x88, d); w32(0x90, n); w32(0x98, cmd);
	while (r32(0x98) & 1) {
		spin_relax(1000);
		if (++t > 100000) return -90;
	}
	return 0;
}
static int cmp8(const unsigned char *a, const unsigned char *b, unsigned int n) {
	unsigned int i;
	for (i = 0; i < n; i++) if (a[i] != b[i]) return -1;
	return 0;
}
static void fillp(unsigned char *p, unsigned int n, unsigned int seed) {
	unsigned int i;
	for (i = 0; i < n; i++) p[i] = (unsigned char)((i * 0x11 + seed) & 0xff);
}
static int roundtrip(unsigned char *s, unsigned long sb, unsigned char *d, unsigned long db,
		     unsigned int off, unsigned int n, unsigned int so, unsigned int seed) {
	int r;
	fillp(s + off, n, seed);
	memset(d + off, 0, n);
	if ((r = edu_dma(sb + off, EDU_SRAM + so, n, 1))) return -1000 + r;
	if ((r = edu_dma(EDU_SRAM + so, db + off, n, 3))) return -2000 + r;
	if (cmp8(s + off, d + off, n)) return -3000 - (int)n;
	return 0;
}
static irqreturn_t edu_irq(int irq, void *dev_id) {
	if ((unsigned long)dev_id != irq_cookie) return 0;
	if (irq != (int)irq_vec) return 0;
	{
		unsigned int st = r32(0x24);
		if (st) iowrite32(st, (unsigned long)(bar + 0x64));
		irq_count++;
	}
	return 1;
}
static int edu_probe(struct pci_dev *d, const struct pci_device_id *id) {
	int r, i;
	unsigned char *s, *dd;
	if (pci_enable_device(d)) return -11;
	if (pci_request_regions(d, "edu")) { pci_disable_device(d); return -12; }
	bar = (unsigned long)pci_ioremap_bar(d, 0);
	if (!bar) { pci_release_regions(d); pci_disable_device(d); return -13; }
	if (r32(0x00) != 0x010000EDU) return -3;
	sbuf = dma_alloc_coherent(0, 16384, &shan, 0);
	dbuf = dma_alloc_coherent(0, 16384, &dhan, 0);
	if (!sbuf || !dbuf) return -4;
	srb = (unsigned long)shan; drb = (unsigned long)dhan;
	s = (unsigned char *)sbuf; dd = (unsigned char *)dbuf;
	memset(s, 0x5a, 16384); memset(dd, 0x5a, 16384);
	/* DMA matrix (subset for speed; full matrix in A1/A3 gates). */
	if ((r = roundtrip(s, srb, dd, drb, 4096, 1, 0, 0x11))) return -10 + r;
	if ((r = roundtrip(s, srb, dd, drb, 4096, 4096, 0, 0x55))) return -20 + r;
	if ((r = roundtrip(s, srb, dd, drb, 8192 - 1, 2, 0, 0x66))) return -30 + r;
	{
		unsigned int k;
		for (k = 0; k < 4096; k++) if (s[k] != 0x5a || dd[k] != 0x5a) return -130;
		for (k = 12288; k < 16384; k++) if (s[k] != 0x5a || dd[k] != 0x5a) return -131;
	}
	/* INTx IRQ: alloc, register, trigger 0x1, wait, verify+ack, cleanup. */
	{
		int irq_rc = 0;
		int nv = pci_alloc_irq_vectors(d, 1, 1, 1);
		if (nv != 1) return -300 + nv;
		irq_vec = pci_irq_vector(d, 0);
		if (irq_vec != 0x58) return -301;
		if (request_irq(irq_vec, edu_irq, 0, "edu", (void *)irq_cookie)) return -302;
		irq_registered = 1;
		if (request_irq(irq_vec, edu_irq, 0, "edu", (void *)irq_cookie) != -16) { irq_rc = -303; goto irq_out; }
		irq_count = 0;
		__asm__ volatile ("sti" ::: "memory");
		iowrite32(0x1, (unsigned long)(bar + 0x60));
		for (i = 0; i < 200; i++) { spin_relax(50000); if (irq_count >= 1) break; }
		__asm__ volatile ("cli" ::: "memory");
		if (irq_count != 1) { irq_rc = -304; goto irq_out; }
		if (r32(0x24) != 0) { irq_rc = -305; goto irq_out; }
		/* DMA-done IRQ (0x100). */
		fillp(s + 4096, 64, 0x77);
		__asm__ volatile ("sti" ::: "memory");
		if (edu_dma(srb + 4096, EDU_SRAM, 64, 0x05)) { __asm__ volatile ("cli" ::: "memory"); irq_rc = -306; goto irq_out; }
		for (i = 0; i < 200; i++) { spin_relax(50000); if (irq_count >= 2) break; }
		__asm__ volatile ("cli" ::: "memory");
		if (irq_count != 2) { irq_rc = -307; goto irq_out; }
		if (r32(0x24) != 0) { irq_rc = -308; goto irq_out; }
		free_irq(irq_vec, (void *)irq_cookie);
		irq_registered = 0;
		{
			unsigned int before = irq_count;
			volatile unsigned int z;
			iowrite32(0x2, (unsigned long)(bar + 0x60));
			for (z = 0; z < 100000; z++) { __asm__ volatile ("" ::: "memory"); }
			if (irq_count != before) { irq_rc = -310; goto irq_out; }
			iowrite32(0x2, (unsigned long)(bar + 0x64));
			if (r32(0x24) != 0) { irq_rc = -311; goto irq_out; }
		}
		if (pci_irq_vector(d, 0) == 0) { irq_rc = -312; goto irq_out; }
	irq_out:
		__asm__ volatile ("cli" ::: "memory");
		if (irq_registered) { free_irq(irq_vec, (void *)irq_cookie); irq_registered = 0; }
		pci_free_irq_vectors(d);
		if (irq_rc) return irq_rc;
		if (pci_irq_vector(d, 0) != 0) return -313;
	}
	if (r32(0x20) != 0) return -170;
	printk("edu full-ok dma+irq vec=%x\n", irq_vec);
	return 0;
}
static void edu_remove(struct pci_dev *d) {
	if (irq_registered) { free_irq(irq_vec, (void *)irq_cookie); irq_registered = 0; }
	pci_free_irq_vectors(d);
	if (sbuf) { dma_free_coherent(0, 16384, sbuf, shan); sbuf = 0; }
	if (dbuf) { dma_free_coherent(0, 16384, dbuf, dhan); dbuf = 0; }
	if (bar) { pci_iounmap(d, (void *)bar); bar = 0; }
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
