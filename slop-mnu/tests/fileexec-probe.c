/* File-backed execve probe: mount the guest root disk and execve the test
 * binaries off it one by one, reporting each child's result through wait4.
 * argv[1..] select targets: a bare name maps to /mntpy/<name>, an
 * absolute path is exec'd as-is (used to exec initramfs binaries via fork
 * for comparison). Success is asserted by the harness from the child's
 * own stdout marker; this probe only reports exec+wait mechanics. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/mount.h>
#include <sys/stat.h>
#include <sys/wait.h>

int main(int argc, char **argv) {
    if (argc < 2 || strstr(argv[1], "") == NULL) return 1;
    mkdir("/mntpy", 0755);
    /* ext4 first: a journaled/extent ext4 image mounts as ext2 only in a
     * broken, read-as-linear state; its incompatible features must send us
     * to the ext4 driver. The ext4 driver refuses plain ext2, so the
     * fallback order is safe. */
    const char *fs = NULL;
    if (mount("vda", "/mntpy", "ext4", 0, NULL) == 0) fs = "ext4";
    else if (mount("vda", "/mntpy", "ext2", 0, NULL) == 0) fs = "ext2";
    if (!fs) {
        /* The initramfs-control targets do not need the disk; only fail when
         * a bare (disk) target is requested. */
        int need_disk = 0;
        for (int i = 1; i < argc; i++) if (argv[i][0] != '/') need_disk = 1;
        if (need_disk) { printf("FILEEXEC_FAIL mount\n"); return 1; }
    } else {
        printf("FILEEXEC_MOUNT fs=%s\n", fs);
    }
    int failures = 0;
    for (int i = 1; i < argc; i++) {
        const char *target = argv[i];
        char path[128];
        if (target[0] == '/') snprintf(path, sizeof path, "%s", target);
        else snprintf(path, sizeof path, "/mntpy/%s", target);
        pid_t pid = fork();
        if (pid == 0) {
            char *child_argv[] = { (char *)target, 0 };
            char *envp[] = { 0 };
            execve(path, child_argv, envp);
            printf("FILEEXEC_FAIL exec target=%s\n", target);
            _exit(127);
        }
        int st = 0;
        if (waitpid(pid, &st, 0) < 0) {
            printf("FILEEXEC_FAIL wait target=%s\n", target);
            failures++;
            continue;
        }
        if (WIFEXITED(st)) {
            printf("FILEEXEC_CHILD rc=%d target=%s\n", WEXITSTATUS(st), target);
        } else {
            printf("FILEEXEC_FAIL signal st=%#x target=%s\n", st, target);
            failures++;
        }
    }
    printf("FILEEXEC_DONE failures=%d\n", failures);
    return failures ? 1 : 0;
}
