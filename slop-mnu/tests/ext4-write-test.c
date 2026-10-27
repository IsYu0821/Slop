/* ext4 overwrite test: opens /mnt/xdiag4/hello.txt, overwrites bytes in
   place (no size change, no allocation), reads back and verifies. */
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>

int main(void) {
    const char *path = "/mnt/xdiag4/hello.txt";
    printf("ext4w: opening\n");
    int fd = open(path, O_RDWR);
    printf("ext4w: opened fd=%d\n", fd);
    if (fd < 0) { printf("ext4w: open failed\n"); return 1; }
    /* Overwrite "econd l" (offsets 19-24) with "SECOND" (same length). */
    if (lseek(fd, 19, SEEK_SET) != 19) { printf("ext4w: seek failed\n"); return 1; }
    const char *patch = "SECOND";
    printf("ext4w: writing\n");
    if (write(fd, patch, 6) != 6) { printf("ext4w: write failed\n"); return 1; }
    close(fd);
    /* Read back and verify. */
    fd = open(path, O_RDONLY);
    if (fd < 0) { printf("ext4w: reopen failed\n"); return 1; }
    char buf[128];
    printf("ext4w: reading\n");
    int n = (int)read(fd, buf, sizeof buf - 1);
    close(fd);
    if (n < 25) { printf("ext4w: short read %d\n", n); return 1; }
    buf[n] = 0;
    if (memcmp(buf + 19, "SECOND", 6) != 0) {
        printf("ext4w: reread mismatch: %.30s\n", buf);
        return 1;
    }
    if (memcmp(buf, "Hello from ext4.\n", 17) != 0) {
        printf("ext4w: prefix damaged\n");
        return 1;
    }
    printf("ext4w: overwrite+readback OK [%s]\n", buf);
    printf("EXT4W_OK\n");
    return 0;
}
