/* ext4 mutation test: create/write/read/unlink/mkdir/rename/truncate on the
   virtio-blk ext4 image, readback verify. Host checks e2fsck + content. */
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <sys/mount.h>
int main(void) {
    setvbuf(stdout, NULL, _IONBF, 0);
    printf("ext4m: start\n");
    printf("ext4m: mkdir\n");
    if (mkdir("/mntw", 0755) != 0) { printf("ext4m: mkdir mntw failed\n"); return 1; }
    printf("ext4m: mount\n");
    if (mount("vda", "/mntw", "ext4", 0, NULL) != 0) { printf("ext4m: mount failed\n"); return 1; }
    /* create + write + read */
    printf("ext4m: create\n");
    int fd = open("/mntw/xdiag4/newfile.bin", O_CREAT|O_RDWR, 0644);
    if (fd < 0) { printf("ext4m: create failed\n"); return 1; }
    char w[3000]; for (int i = 0; i < 3000; i++) w[i] = (char)(i * 5 + 3);
    printf("ext4m: write\n");
    if (write(fd, w, 3000) != 3000) { printf("ext4m: write failed\n"); close(fd); return 1; }
    close(fd);
    fd = open("/mntw/xdiag4/newfile.bin", O_RDONLY);
    if (fd < 0) { printf("ext4m: reopen failed\n"); return 1; }
    static char r[3000]; int t = 0, n;
    while (t < 3000 && (n = read(fd, r + t, 3000 - t)) > 0) t += n;
    close(fd);
    if (t != 3000 || memcmp(r, w, 3000)) { printf("ext4m: reread mismatch %d\n", t); return 1; }
    /* mkdir + create inside + rename + unlink */
    if (mkdir("/mntw/xdiag4/sub2", 0755) != 0) { printf("ext4m: mkdir failed\n"); return 1; }
    fd = open("/mntw/xdiag4/sub2/inner.txt", O_CREAT|O_RDWR, 0644);
    if (fd < 0) { printf("ext4m: create inner failed\n"); return 1; }
    if (write(fd, "inner-data", 10) != 10) { printf("ext4m: write inner failed\n"); close(fd); return 1; }
    close(fd);
    if (rename("/mntw/xdiag4/sub2/inner.txt", "/mntw/xdiag4/renamed.txt") != 0) { printf("ext4m: rename failed\n"); return 1; }
    fd = open("/mntw/xdiag4/renamed.txt", O_RDONLY);
    if (fd < 0) { printf("ext4m: open renamed failed\n"); return 1; }
    char rb[16] = {0};
    if (read(fd, rb, 10) != 10 || memcmp(rb, "inner-data", 10)) { printf("ext4m: renamed content bad\n"); close(fd); return 1; }
    close(fd);
    /* truncate the 3000B file to 100B */
    if (truncate("/mntw/xdiag4/newfile.bin", 100) != 0) { printf("ext4m: truncate failed\n"); return 1; }
    struct stat st;
    if (stat("/mntw/xdiag4/newfile.bin", &st) || st.st_size != 100) { printf("ext4m: stat size %ld\n", (long)st.st_size); return 1; }
    /* unlink both files + rmdir */
    if (unlink("/mntw/xdiag4/renamed.txt") != 0) { printf("ext4m: unlink renamed failed\n"); return 1; }
    if (unlink("/mntw/xdiag4/newfile.bin") != 0) { printf("ext4m: unlink newfile failed\n"); return 1; }
    if (rmdir("/mntw/xdiag4/sub2") != 0) { printf("ext4m: rmdir failed\n"); return 1; }
    /* verify gone */
    if (open("/mntw/xdiag4/newfile.bin", O_RDONLY) >= 0) { printf("ext4m: newfile still exists\n"); return 1; }
    printf("EXT4M_OK\n");
    return 0;
}
