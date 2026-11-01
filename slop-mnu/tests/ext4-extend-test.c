/* ext4 extension test: mounts the virtio-blk ext4 image read-write,
   appends ~2 blocks to a file (block allocation + extent growth),
   writes with a gap (zero-fill check), reads back and verifies. */
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <sys/mount.h>
static char wbuf[8000];
static char rbuf[9000];
static char gbuf[100];
static char g2[20100];
int main(void) {
    if (mkdir("/mntw", 0755) != 0) { printf("ext4x: mkdir failed\n"); return 1; }
    if (mount("vda", "/mntw", "ext4", 0, NULL) != 0) { printf("ext4x: mount failed\n"); return 1; }
    const char *path = "/mntw/xdiag4/grow.bin";
    /* create a 100-byte seed file first (via O_CREAT? No creat support yet?
       use existing hello.txt and extend it instead to avoid create path). */
    const char *hpath = "/mntw/xdiag4/hello.txt";
    int fd = open(hpath, O_RDWR);
    if (fd < 0) { printf("ext4x: open failed\n"); return 1; }
    /* append 8000 bytes (file grows 35 -> 8035) (spans 2 new blocks for 4K bs) at EOF */
    if (lseek(fd, 0, SEEK_END) < 0) { printf("ext4x: seek end failed\n"); return 1; }
    for (int i = 0; i < 8000; i++) wbuf[i] = (char)((i * 37 + 11) & 0xff);
    int w = 0;
    while (w < 8000) {
        int n = write(fd, wbuf + w, 8000 - w);
        if (n <= 0) { printf("ext4x: write failed at %d\n", w); close(fd); return 1; }
        w += n;
    }
    close(fd);
    /* read back: original 35 bytes + 8000 appended = 8035 */
    fd = open(hpath, O_RDONLY);
    if (fd < 0) { printf("ext4x: reopen failed\n"); return 1; }
    int t = 0, n;
    while (t < 8035 && (n = read(fd, rbuf + t, 8035 - t)) > 0) t += n;
    close(fd);
    if (t != 8035) { printf("ext4x: short read %d\n", t); return 1; }
    if (memcmp(rbuf, "Hello from ext4.\nSecond line here.\n", 35) != 0) { printf("ext4x: prefix damaged\n"); return 1; }
    if (memcmp(rbuf + 35, wbuf, 8000) != 0) { printf("ext4x: appended mismatch\n"); return 1; }
    printf("ext4x: append+readback OK (%d bytes)\n", t);
    /* gap write: seek 20000, write 100 bytes -> gap [8035,20000) must be zeros */
    fd = open(hpath, O_RDWR);
    if (fd < 0) { printf("ext4x: reopen2 failed\n"); return 1; }
    if (lseek(fd, 20000, SEEK_SET) != 20000) { printf("ext4x: seek gap failed\n"); return 1; }
    for (int i = 0; i < 100; i++) gbuf[i] = (char)(0xA0 + i);
    if (write(fd, gbuf, 100) != 100) { printf("ext4x: gap write failed\n"); close(fd); return 1; }
    close(fd);
    fd = open(hpath, O_RDONLY);
    if (fd < 0) { printf("ext4x: reopen3 failed\n"); return 1; }
    t = 0;
    while (t < 20100 && (n = read(fd, g2 + t, 20100 - t)) > 0) t += n;
    close(fd);
    if (t != 20100) { printf("ext4x: gap read %d\n", t); return 1; }
    for (int i = 8035; i < 20000; i++) if (g2[i] != 0) { printf("ext4x: gap nonzero at %d\n", i); return 1; }
    if (memcmp(g2 + 20000, gbuf, 100) != 0) { printf("ext4x: gap data mismatch\n"); return 1; }
    printf("ext4x: gap zero-fill OK\n");
    (void)path;
    printf("EXT4X_OK\n");
    return 0;
}
