#include <linux/init.h>
#include <linux/module.h>
#include <linux/pci.h>
#include <linux/string.h>
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
	printk("edu ck t1 ok\n");
	if ((r = roundtrip(s, srb, dd, drb, 4096, 7, 0, 0x22))) return -20 + r;
	if ((r = roundtrip(s, srb, dd, drb, 4096, 64, 0, 0x33))) return -30 + r;
	if ((r = roundtrip(s, srb, dd, drb, 4096, 4095, 0, 0x44))) return -40 + r;
	if ((r = roundtrip(s, srb, dd, drb, 4096, 4096, 0, 0x55))) return -50 + r;
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
	for (i = 0; i < 20; i++) {
		if ((r = roundtrip(s, srb, dd, drb, 4096, 1024, (i & 1) ? 0 : 2048, 0xc0 + i))) return -120 - i;
	}
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
	for (i = 0; i < 100; i++) {
		if ((r = roundtrip(s, srb, dd, drb, 4096, 4096, 0, 0xee + (i & 0xf)))) return -160 - (i > 90 ? 90 : i);
	}
	/* status idle? */
	if (r32(0x20) != 0) return -170;
	printk("edu dma-ok 100iter guards-ok srb=%lx\n", srb);
	return 0;
}
static void edu_remove(struct pci_dev *d) {
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
