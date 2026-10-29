#include <linux/init.h>
#include <linux/module.h>
#include <linux/pci.h>
#include <linux/string.h>
#include <linux/interrupt.h>
#include <linux/delay.h>
MODULE_LICENSE("GPL");
/* Track A1: EDU real device-DMA. RAM<->EDU-SRAM(0x40000) via 0x80/0x88/0x90/0x98.
 * 64-bit regs via 32-bit pairs (spec allows size-4 for >=0x80). */
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
static int probed_ok = 0;
static irqreturn_t edu_irq(int irq, void *dev_id) {
	if ((unsigned long)dev_id != irq_cookie) return 0;
	if (irq != (int)irq_vec) return 0;
	{
		unsigned int st = ioread32((unsigned long)(bar + 0x24));
		if (st) iowrite32(st, (unsigned long)(bar + 0x64));
		irq_count++;
	}
	return 1;
}
static inline void w32(unsigned long o, unsigned int v) { iowrite32(v, (unsigned long)(bar + o)); }
static inline unsigned int r32(unsigned long o) { return ioread32((unsigned long)(bar + o)); }
static inline void w64(unsigned long o, unsigned long v) {
	iowrite32((unsigned int)v, (unsigned long)(bar + o));
	iowrite32((unsigned int)(v >> 32), (unsigned long)(bar + o + 4));
}
/* Run one EDU DMA: dev_src -> dev_dst, cnt bytes. Returns 0 when cmd clears. */
static int edu_dma(unsigned long s, unsigned long d, unsigned int n, unsigned int cmd) {
	int t = 0;
	w64(0x80, s); w64(0x88, d); w32(0x90, n); w32(0x98, cmd);
	while (r32(0x98) & 1) {
		udelay(10);
		if (++t > 10000) return -90; /* ~100ms timeout */
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
/* Roundtrip: src RAM -> SRAM@so -> dst RAM. Verifies bytes. */
static int roundtrip(unsigned char *s, unsigned long sb, unsigned char *d, unsigned long db,
		     unsigned int off, unsigned int n, unsigned int so, unsigned int seed) {
	int r;
	fillp(s + off, n, seed);
	memset(d + off, 0, n);
	/* x86 coherent: no sync needed (Slop core selftests the sync path). */
	if ((r = edu_dma(sb + off, EDU_SRAM + so, n, 1))) return -1000 + r;
	if ((r = edu_dma(EDU_SRAM + so, db + off, n, 3))) return -2000 + r;
	if (cmp8(s + off, d + off, n)) return -3000 - (int)n;
	return 0;
}
static int edu_probe(struct pci_dev *d, const struct pci_device_id *id) {
	int r, i;
	unsigned char *s, *dd;
	if (pci_enable_device(d)) return -11;
	pci_set_master(d);
	if (pci_request_regions(d, "edu")) { pci_disable_device(d); return -12; }
	bar = (unsigned long)pci_ioremap_bar(d, 0);
	if (!bar) { pci_release_regions(d); pci_disable_device(d); return -13; }
	{ unsigned long sp; __asm__ volatile ("mov %%rsp, %0" : "=r"(sp)); printk("edu ck bar=%lx addr=%lx rsp=%lx\n", (unsigned long)bar, (unsigned long)&bar, sp); }
	printk("edu ck id=%x\n", r32(0x00));
	if (r32(0x00) != 0x010000EDU) return -3;
	/* 3-page src/dst arenas; outer pages are guards. */
	sbuf = dma_alloc_coherent(0, 16384, &shan, 0);
	dbuf = dma_alloc_coherent(0, 16384, &dhan, 0);
	if (!sbuf || !dbuf) return -4;
	srb = (unsigned long)shan; drb = (unsigned long)dhan;
	s = (unsigned char *)sbuf; dd = (unsigned char *)dbuf;
	printk("edu ck alloc s=%lx d=%lx\n", srb, drb);
	memset(s, 0x5a, 16384); memset(dd, 0x5a, 16384);
	/* sizes: 1, small, boundary-1, full page */
	printk("edu ck t1\n");
	if ((r = roundtrip(s, srb, dd, drb, 4096, 1, 0, 0x11))) return -10 + r;
	printk("edu ck rt7\n");
	if ((r = roundtrip(s, srb, dd, drb, 4096, 7, 0, 0x22))) return -20 + r;
	printk("edu ck rt64\n");
	if ((r = roundtrip(s, srb, dd, drb, 4096, 64, 0, 0x33))) return -30 + r;
	printk("edu ck rt4095\n");
	if ((r = roundtrip(s, srb, dd, drb, 4096, 4095, 0, 0x44))) return -40 + r;
	printk("edu ck rt4096\n");
	if ((r = roundtrip(s, srb, dd, drb, 4096, 4096, 0, 0x55))) return -50 + r;
	printk("edu ck matrix ok\n");
	/* page-boundary crossing + odd alignment */
	if ((r = roundtrip(s, srb, dd, drb, 8192 - 1, 2, 0, 0x66))) return -60 + r;
	if ((r = roundtrip(s, srb, dd, drb, 8192 - 16, 32, 512, 0x77))) return -70 + r;
	if ((r = roundtrip(s, srb, dd, drb, 4096 + 1, 100, 1024, 0x88))) return -80 + r;
	if ((r = roundtrip(s, srb, dd, drb, 8192 + 3, 100, 2048, 0x99))) return -90 + r;
	/* multi-page via 2 chunks */
	fillp(s + 4096, 8192, 0xaa); memset(dd + 4096, 0, 8192);
	if ((r = edu_dma(srb + 4096, EDU_SRAM, 4096, 1))) return -100 + r;
	if ((r = edu_dma(EDU_SRAM, drb + 4096, 4096, 3))) return -101 + r;
	if ((r = edu_dma(srb + 8192, EDU_SRAM, 4096, 1))) return -102 + r;
	if ((r = edu_dma(EDU_SRAM, drb + 8192, 4096, 3))) return -103 + r;
	if (cmp8(s + 4096, dd + 4096, 8192)) return -104;
	/* SRAM offset isolation */
	if ((r = roundtrip(s, srb, dd, drb, 4096, 16, 3072, 0xbb))) return -110 + r;
	/* 20x back-to-back alternating offsets */
	printk("edu ck 20x begin\n");
	for (i = 0; i < 20; i++) {
		if ((r = roundtrip(s, srb, dd, drb, 4096, 1024, (i & 1) ? 0 : 2048, 0xc0 + i))) return -120 - i;
	}
	printk("edu ck 20x ok\n");
	/* Slop-layer streaming validation lives in the Slop DMA core selftest
	   (map_single mask/contiguity/leak). Device DMA below uses coherent. */
	/* guards intact? */
	{
		unsigned int k;
		for (k = 0; k < 4096; k++) if (s[k] != 0x5a || dd[k] != 0x5a) return -130;
		for (k = 12288; k < 16384; k++) if (s[k] != 0x5a || dd[k] != 0x5a) return -131;
	}
	/* NOTE: QEMU aborts the VM on OOB/zero-count device DMA (fail-stop at
	   the emulator), so invalid device addresses are validated at the
	   Slop layer (map_single mask/contiguity) and never programmed. The
	   poll timeout below remains as defense-in-depth for a hung device. */
	if ((r = roundtrip(s, srb, dd, drb, 4096, 64, 0, 0xdd))) return -150 + r;
	/* 100x iteration */
	printk("edu ck 100x begin\n");
	for (i = 0; i < 100; i++) {
		if ((r = roundtrip(s, srb, dd, drb, 4096, 4096, 0, 0xee + (i & 0xf)))) return -160 - (i > 90 ? 90 : i);
		if ((i % 10) == 9) printk("edu ck 100x %d\n", i + 1);
	}
	printk("edu ck 100x ok\n");
	/* status idle? */
	printk("edu ck irq begin\n");
	/* INTx IRQ via IOAPIC (narrow STI window). */
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
		{ unsigned int i;
		for (i = 0; i < 200 && irq_count < 1; i++) { udelay(50); }
		__asm__ volatile ("cli" ::: "memory");
		if (irq_count != 1) { irq_rc = -304; goto irq_out; } }
		if (r32(0x24) != 0) { irq_rc = -305; goto irq_out; }
		fillp(s + 4096, 64, 0x77);
		__asm__ volatile ("sti" ::: "memory");
		if (edu_dma(srb + 4096, EDU_SRAM, 64, 0x05)) { __asm__ volatile ("cli" ::: "memory"); irq_rc = -306; goto irq_out; }
		{ unsigned int i;
		for (i = 0; i < 200 && irq_count < 2; i++) { udelay(50); }
		__asm__ volatile ("cli" ::: "memory");
		if (irq_count != 2) { irq_rc = -307; goto irq_out; } }
		if (r32(0x24) != 0) { irq_rc = -308; goto irq_out; }
		/* verify the IRQ'd transfer content: SRAM back to dst, byte compare */
		memset(dd + 4096, 0, 64);
		if (edu_dma(EDU_SRAM, drb + 4096, 64, 3)) { irq_rc = -309; goto irq_out; }
		if (cmp8(s + 4096, dd + 4096, 64)) { irq_rc = -310; goto irq_out; }
		free_irq(irq_vec, (void *)irq_cookie);
		irq_registered = 0;
		{
			unsigned int before = irq_count;
			iowrite32(0x2, (unsigned long)(bar + 0x60));
			udelay(100);
			if (irq_count != before) { irq_rc = -311; goto irq_out; }
			iowrite32(0x2, (unsigned long)(bar + 0x64));
			if (r32(0x24) != 0) { irq_rc = -312; goto irq_out; }
		}
		if (pci_irq_vector(d, 0) == 0) { irq_rc = -313; goto irq_out; }
	irq_out:
		__asm__ volatile ("cli" ::: "memory");
		if (irq_registered) { free_irq(irq_vec, (void *)irq_cookie); irq_registered = 0; }
		pci_free_irq_vectors(d);
		if (irq_rc) return irq_rc;
		if (pci_irq_vector(d, 0) != 0) return -314;
	}
	if (r32(0x20) != 0) return -170;
	probed_ok = 1;
	printk("edu dma-ok 100iter guards-ok srb=%lx\n", srb);
	return 0;
}
static void edu_remove(struct pci_dev *d) {
	probed_ok = 0;
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
static int __init t_init(void) {
	int r = pci_register_driver((void *)&edu_drv);
	if (r) return r;
	return probed_ok ? 0 : -19;
}
static void __exit t_exit(void) { pci_unregister_driver((void *)&edu_drv); }
module_init(t_init);
module_exit(t_exit);
