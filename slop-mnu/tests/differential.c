#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <sys/mman.h>
#include <sys/time.h>
#include <sys/resource.h>
#include <sys/utsname.h>
#include <sys/epoll.h>
#include <sys/inotify.h>
#include <sys/eventfd.h>
#include <sys/timerfd.h>
#include <sys/signalfd.h>
#include <sys/random.h>
#include <sys/socket.h>
#include <sys/timerfd.h>
#include <sys/signalfd.h>
#include <sys/sysinfo.h>
#include <sys/syscall.h>
#include <poll.h>
#include <signal.h>
#include <dirent.h>
#include <errno.h>
#include <sys/file.h>
#include <time.h>
#include <stdint.h>

#ifndef SYS_io_uring_setup
#define SYS_io_uring_setup 425
#define SYS_io_uring_enter 426
#endif

struct slop_io_sq_off {
    uint32_t head, tail, ring_mask, ring_entries, flags, dropped, array, reserved;
    uint64_t user_addr;
};
struct slop_io_cq_off {
    uint32_t head, tail, ring_mask, ring_entries, overflow, cqes, flags, reserved;
    uint64_t user_addr;
};
struct slop_io_params {
    uint32_t sq_entries, cq_entries, flags, sq_thread_cpu, sq_thread_idle;
    uint32_t features, wq_fd, reserved[3];
    struct slop_io_sq_off sq_off;
    struct slop_io_cq_off cq_off;
};

static int fail(const char *what) {
    printf("DIFF_FAIL: %s (errno=%d)\n", what, errno);
    return 1;
}

int main(int argc, char **argv) {
    if (argc > 1 && strcmp(argv[1], "cloexec-child") == 0) {
        if (fcntl(9, F_GETFD) == -1 && errno == EBADF) _exit(42);
        _exit(1);
    }
    setvbuf(stdout, NULL, _IONBF, 0);
    printf("DIFF_OK\n");

    /* File, directory and path APIs. */
    int fd = open("/tmp/differential", O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0) return fail("open");
    if (write(fd, "data", 4) != 4) return fail("write");
    close(fd);

    fd = open("/tmp/differential", O_RDONLY);
    if (fd < 0) return fail("open read");
    char buf[8];
    if (read(fd, buf, 4) != 4 || memcmp(buf, "data", 4) != 0) return fail("read");
    if (lseek(fd, 0, SEEK_CUR) < 0) return fail("lseek");
    struct stat st;
    if (fstat(fd, &st) != 0 || st.st_size != 4) return fail("fstat");
    close(fd);
    if (unlink("/tmp/differential") != 0) return fail("unlink");

    errno = 0;
    if (open("/tmp/slop-differential-does-not-exist", O_RDONLY) != -1 || errno != ENOENT)
        return fail("open ENOENT");
    errno = 0;
    if (close(-1) != -1 || errno != EBADF) return fail("close EBADF");
    errno = 0;
    if (mmap(NULL, 4096, PROT_READ, MAP_PRIVATE | MAP_SHARED | MAP_ANONYMOUS, -1, 0) != MAP_FAILED || errno != EINVAL)
        return fail("mmap EINVAL");
    errno = 0;
    if (unlink("/tmp/slop-differential-does-not-exist") != -1 || errno != ENOENT)
        return fail("unlink ENOENT");
    printf("DIFF_ERRNO_OK\n");

    fd = open("/proc/self/status", O_RDONLY);
    if (fd < 0) return fail("open proc status");
    char proc_status[256];
    ssize_t proc_bytes = read(fd, proc_status, sizeof(proc_status));
    if (proc_bytes <= 0) return fail("read proc status");
    close(fd);
    printf("DIFF_PROC_OK\n");

    if (mkdir("/tmp/differential-dir", 0755) != 0) return fail("mkdir");
    if (rmdir("/tmp/differential-dir") != 0) return fail("rmdir");
    if (stat("/etc/motd", &st) != 0 || st.st_size <= 0) return fail("stat");

    char cwd[128];
    if (getcwd(cwd, sizeof(cwd)) == NULL) return fail("getcwd");
    if (chdir("/") != 0) return fail("chdir");

    /* Process and identity APIs. */
    if (getpid() <= 0) return fail("getpid");
    if (getppid() < 0) return fail("getppid");
    if (getuid() != geteuid() || getgid() != getegid()) return fail("uid");
    if (gettid() <= 0) return fail("gettid");

    /* Pipe, dup and fork. */
    int pfd[2];
    if (pipe(pfd) != 0) return fail("pipe");
    if (write(pfd[1], "pipe", 4) != 4) return fail("pipe write");
    if (read(pfd[0], buf, 4) != 4 || memcmp(buf, "pipe", 4) != 0) return fail("pipe read");
    int dupfd = dup(pfd[0]);
    if (dupfd < 0) return fail("dup");
    close(dupfd);
    close(pfd[0]);
    close(pfd[1]);

    int sv[2];
    if (socketpair(AF_UNIX, SOCK_STREAM, 0, sv) != 0) return fail("socketpair");
    if (write(sv[0], "sp", 2) != 2) return fail("socketpair write");
    if (read(sv[1], buf, 2) != 2 || memcmp(buf, "sp", 2) != 0) return fail("socketpair read");
    close(sv[0]);
    close(sv[1]);

    pid_t child = fork();
    if (child < 0) return fail("fork");
    if (child == 0) {
        _exit(42);
    }
    int status = 0;
    if (waitpid(child, &status, 0) != child) return fail("waitpid");
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 42) return fail("wait status");

    /* Memory APIs. */
    void *m = mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (m == MAP_FAILED) return fail("mmap");
    if (mprotect(m, 4096, PROT_READ | PROT_WRITE) != 0) return fail("mprotect");
    ((char *)m)[0] = 'x';
    if (munmap(m, 4096) != 0) return fail("munmap");

    /* Time and randomness. */
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) return fail("clock_gettime");
    char rnd[16];
    if (getrandom(rnd, sizeof(rnd), 0) != (ssize_t)sizeof(rnd)) return fail("getrandom");

    struct utsname uts;
    if (uname(&uts) != 0) return fail("uname");
    if (gethostname(buf, sizeof(buf)) != 0) return fail("gethostname");

    /* Resource and identity queries. */
    struct rlimit rl;
    if (getrlimit(RLIMIT_NOFILE, &rl) != 0) return fail("getrlimit");
    struct rusage ru;
    if (getrusage(RUSAGE_SELF, &ru) != 0) return fail("getrusage");
    struct sysinfo si;
    if (sysinfo(&si) != 0) return fail("sysinfo");

    struct slop_cap_header { uint32_t version; int32_t pid; } cap_header = {0x20080522, 0};
    struct slop_cap_data { uint32_t effective, permitted, inheritable; } cap_data[2] = {{0}};
    if (syscall(SYS_capget, &cap_header, cap_data) != 0 || cap_header.version != 0x20080522)
        return fail("capget");
    if (syscall(SYS_capset, &cap_header, cap_data) != 0) return fail("capset unchanged");
    printf("DIFF_CAP_OK\n");

    struct slop_sock_filter { uint16_t code; uint8_t jt, jf; uint32_t k; };
    struct slop_sock_fprog { uint16_t len; struct slop_sock_filter *filter; };
    struct slop_sock_filter filter[] = {
        {0x20, 0, 0, 0},
        {0x15, 0, 1, SYS_getpid},
        {0x06, 0, 0, 0x00050000u | EPERM},
        {0x06, 0, 0, 0x7fff0000u},
    };
    struct slop_sock_fprog program = {4, filter};
    if (syscall(SYS_prctl, 38, 1, 0, 0, 0) != 0) return fail("no_new_privs");
    if (syscall(SYS_prctl, 22, 2, &program, 0, 0) != 0) return fail("seccomp install");
    pid_t seccomp_child = fork();
    if (seccomp_child < 0) return fail("seccomp fork");
    if (seccomp_child == 0) _exit(0);
    int seccomp_status = 0;
    if (waitpid(seccomp_child, &seccomp_status, 0) != seccomp_child ||
        !WIFEXITED(seccomp_status) || WEXITSTATUS(seccomp_status) != 0)
        return fail("seccomp fork/wait");
    errno = 0;
    if (syscall(SYS_getpid) != -1 || errno != EPERM) return fail("seccomp errno");
    printf("DIFF_SECCOMP_OK\n");

    /* Directory enumeration. */
    DIR *dir = opendir("/");
    if (!dir) return fail("opendir");
    struct dirent *de = readdir(dir);
    if (!de) return fail("readdir");
    closedir(dir);

    /* Event and notification APIs. */
    int ep = epoll_create1(0);
    if (ep < 0) return fail("epoll_create1");
    close(ep);

    int evfd = eventfd(0, 0);
    if (evfd < 0) return fail("eventfd");
    close(evfd);

    int tfd = timerfd_create(CLOCK_MONOTONIC, 0);
    if (tfd < 0) return fail("timerfd_create");
    close(tfd);

    const char *watch_dir = "/tmp/slop-differential-watch";
    const char *watch_file = "/tmp/slop-differential-watch/created";
    (void)rmdir(watch_dir);
    if (mkdir(watch_dir, 0700) != 0) return fail("inotify mkdir");
    int inofd = inotify_init1(IN_NONBLOCK | IN_CLOEXEC);
    if (inofd < 0) return fail("inotify_init");
    int wd = inotify_add_watch(inofd, watch_dir, IN_CREATE | IN_MODIFY);
    if (wd < 0) return fail("inotify_add_watch");
    int watchfd = open(watch_file, O_WRONLY | O_CREAT | O_TRUNC, 0600);
    if (watchfd < 0) return fail("inotify watched create");
    if (write(watchfd, "x", 1) != 1) return fail("inotify watched write");
    close(watchfd);
    char event_buf[256];
    ssize_t event_bytes = read(inofd, event_buf, sizeof(event_buf));
    if (event_bytes < (ssize_t)sizeof(struct inotify_event)) return fail("inotify read");
    int saw_create = 0, saw_modify = 0;
    for (ssize_t off = 0; off + (ssize_t)sizeof(struct inotify_event) <= event_bytes;) {
        struct inotify_event *event = (struct inotify_event *)(event_buf + off);
        if (event->wd != wd) return fail("inotify watch descriptor");
        saw_create |= (event->mask & IN_CREATE) != 0;
        saw_modify |= (event->mask & IN_MODIFY) != 0;
        off += (ssize_t)sizeof(struct inotify_event) + event->len;
    }
    if (!saw_create || !saw_modify) return fail("inotify event masks");
    if (inotify_rm_watch(inofd, wd) != 0) return fail("inotify_rm_watch");
    close(inofd);
    unlink(watch_file);
    rmdir(watch_dir);
    printf("DIFF_INOTIFY_OK\n");

    struct slop_io_params io_params = {0};
    if (sizeof(io_params) != 120) return fail("io_uring params layout");
    int ringfd = (int)syscall(SYS_io_uring_setup, 2, &io_params);
    if (ringfd < 0) {
        if (errno == EPERM || errno == EACCES || errno == ENOSYS || errno == EOPNOTSUPP)
            printf("DIFF_IOURING_SKIP\n");
        else
            return fail("io_uring_setup");
    } else {
        char *sq = mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_SHARED, ringfd, 0);
        char *cq = mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_SHARED, ringfd, 0x08000000);
        char *sqes = mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_SHARED, ringfd, 0x10000000);
        if (sq == MAP_FAILED || cq == MAP_FAILED || sqes == MAP_FAILED) return fail("io_uring mmap");
        uint32_t *sq_tail = (uint32_t *)(sq + io_params.sq_off.tail);
        uint32_t *sq_array = (uint32_t *)(sq + io_params.sq_off.array);
        uint8_t *sqe = (uint8_t *)sqes;
        sqe[0] = 0; /* IORING_OP_NOP */
        *(uint64_t *)(sqe + 32) = 0x534c4f50ULL;
        sq_array[0] = 0;
        __sync_synchronize();
        *sq_tail = 1;
        long submitted = syscall(SYS_io_uring_enter, ringfd, 1, 0, 0, NULL, 0);
        uint32_t cq_tail = *(uint32_t *)(cq + io_params.cq_off.tail);
        uint64_t cqe_data = *(uint64_t *)(cq + io_params.cq_off.cqes);
        int32_t cqe_result = *(int32_t *)(cq + io_params.cq_off.cqes + 8);
        if (submitted != 1 || cq_tail != 1 || cqe_data != 0x534c4f50ULL || cqe_result != 0)
            return fail("io_uring NOP completion");
        if (munmap(sq, 4096) != 0 || munmap(cq, 4096) != 0 || munmap(sqes, 4096) != 0)
            return fail("io_uring munmap");
        close(ringfd);
        printf("DIFF_IOURING_OK\n");
    }

    /* Lazy demand-paged anonymous mapping: write and read back a pattern. */
    {
        unsigned char *lazy = mmap(NULL, 65536, PROT_READ | PROT_WRITE,
                                   MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        if (lazy == MAP_FAILED) return fail("lazy anon mmap");
        for (size_t i = 0; i < 65536; i++) lazy[i] = (unsigned char)(i * 7 + 3);
        for (size_t i = 0; i < 65536; i++)
            if (lazy[i] != (unsigned char)(i * 7 + 3)) return fail("lazy anon content");
        if (munmap(lazy, 65536) != 0) return fail("lazy anon munmap");
        printf("DIFF_LAZY_ANON_OK\n");
    }

    /* Lazy file-backed mapping: contents come from the file on first touch. */
    {
        int ffd = open("/tmp/differential-mmap", O_WRONLY | O_CREAT | O_TRUNC, 0644);
        if (ffd < 0) return fail("file mmap create");
        unsigned char pattern[8192];
        for (size_t i = 0; i < sizeof(pattern); i++) pattern[i] = (unsigned char)(i * 13);
        if (write(ffd, pattern, sizeof(pattern)) != (ssize_t)sizeof(pattern))
            return fail("file mmap write");
        close(ffd);
        ffd = open("/tmp/differential-mmap", O_RDONLY);
        if (ffd < 0) return fail("file mmap reopen");
        unsigned char *fm = mmap(NULL, 8192, PROT_READ, MAP_PRIVATE, ffd, 0);
        if (fm == MAP_FAILED) return fail("file mmap");
        if (memcmp(fm, pattern, 8192) != 0) return fail("file mmap content");
        if (memcmp(fm + 4088, pattern + 4088, 16) != 0) return fail("file mmap page boundary");
        if (munmap(fm, 8192) != 0) return fail("file mmap munmap");
        close(ffd);
        unlink("/tmp/differential-mmap");
        printf("DIFF_MMAP_FILE_OK\n");
    }

    /* Demand-paged brk: small allocations and large (mmap) allocations. */
    {
        char *small = malloc(3000);
        char *large = malloc(300000);
        if (!small || !large) return fail("malloc");
        memset(small, 0x5a, 3000);
        memset(large, 0xa5, 300000);
        for (int i = 0; i < 3000; i++) if (small[i] != 0x5a) return fail("brk content");
        for (int i = 0; i < 300000; i += 4093) if (large[i] != (char)0xa5) return fail("large malloc content");
        free(large);
        free(small);
        printf("DIFF_BRK_OK\n");
    }

    /* mremap grows a mapping preserving contents, then moves it to a fixed address. */
    {
        char *m = mmap(NULL, 4096, PROT_READ | PROT_WRITE,
                       MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        if (m == MAP_FAILED) return fail("mremap mmap");
        memcpy(m, "remap-src", 10);
        void *grown = mremap(m, 4096, 16384, MREMAP_MAYMOVE);
        if (grown == MAP_FAILED) return fail("mremap grow");
        if (memcmp(grown, "remap-src", 10) != 0) return fail("mremap grow content");
        void *target = mmap(NULL, 8192, PROT_READ | PROT_WRITE,
                            MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        if (target == MAP_FAILED) return fail("mremap target");
        void *moved = mremap(grown, 4096, 4096, MREMAP_MAYMOVE | MREMAP_FIXED, target);
        if (moved != target) return fail("mremap FIXED address");
        if (memcmp(moved, "remap-src", 10) != 0) return fail("mremap FIXED content");
        if (munmap(moved, 4096) != 0) return fail("mremap FIXED munmap");
        munmap((char *)target + 4096, 4096);
        munmap((char *)grown + 4096, 12288);
        printf("DIFF_MREMAP_OK\n");
    }

    /* Copy-on-write fork: children see the parent's bytes but diverge privately. */
    {
        char *cow = mmap(NULL, 4096, PROT_READ | PROT_WRITE,
                         MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        if (cow == MAP_FAILED) return fail("cow mmap");
        memcpy(cow, "parent-view", 12);
        pid_t pid = fork();
        if (pid < 0) return fail("cow fork");
        if (pid == 0) {
            if (memcmp(cow, "parent-view", 12) != 0) _exit(7);
            memcpy(cow, "child-view", 11);
            if (memcmp(cow, "child-view", 11) != 0) _exit(8);
            _exit(0);
        }
        int status = 0;
        if (waitpid(pid, &status, 0) != pid || !WIFEXITED(status) || WEXITSTATUS(status) != 0)
            return fail("cow child");
        if (memcmp(cow, "parent-view", 12) != 0) return fail("cow parent isolation");
        if (munmap(cow, 4096) != 0) return fail("cow munmap");
        printf("DIFF_COW_OK\n");
    }

    /* mprotect splits a PROT_NONE mapping; the guard page still faults. */
    {
        char *guard = mmap(NULL, 8192, PROT_NONE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        if (guard == MAP_FAILED) return fail("guard mmap");
        if (mprotect(guard + 4096, 4096, PROT_READ | PROT_WRITE) != 0)
            return fail("guard mprotect");
        strcpy(guard + 4096, "usable");
        pid_t pid = fork();
        if (pid < 0) return fail("guard fork");
        if (pid == 0) { *(volatile char *)guard = 1; _exit(0); }
        int status = 0;
        if (waitpid(pid, &status, 0) != pid || !WIFSIGNALED(status) || WTERMSIG(status) != SIGSEGV)
            return fail("guard sigsegv");
        if (strcmp(guard + 4096, "usable") != 0) return fail("guard usable page");
        if (munmap(guard, 8192) != 0) return fail("guard munmap");
        printf("DIFF_MPROTECT_SPLIT_OK\n");
    }

    /* /proc exposes real process and system data. */
    {
        char sbuf[2048];
        ssize_t n;
        int fd = open("/proc/self/status", O_RDONLY);
        if (fd < 0) return fail("proc status open");
        n = read(fd, sbuf, sizeof(sbuf) - 1);
        close(fd);
        if (n <= 0) return fail("proc status read");
        sbuf[n] = 0;
        char name[64] = "";
        if (sscanf(sbuf, "Name:\t%63s", name) != 1 || name[0] == 0) return fail("proc status name");
        if (!strstr(sbuf, "Pid:") || !strstr(sbuf, "PPid:")) return fail("proc status fields");
        char *pidline = strstr(sbuf, "\nPid:\t");
        int self_pid = 0;
        if (!pidline || sscanf(pidline + 6, "%d", &self_pid) != 1 || self_pid <= 0)
            return fail("proc status pid");
        fd = open("/proc/self/cmdline", O_RDONLY);
        if (fd < 0) return fail("proc cmdline open");
        n = read(fd, sbuf, sizeof(sbuf) - 1);
        close(fd);
        if (n <= 0 || sbuf[n - 1] != 0) return fail("proc cmdline content");
        fd = open("/proc/self/stat", O_RDONLY);
        if (fd < 0) return fail("proc stat open");
        n = read(fd, sbuf, sizeof(sbuf) - 1);
        close(fd);
        if (n <= 0) return fail("proc stat read");
        sbuf[n] = 0;
        if (atoi(sbuf) != self_pid || !strchr(sbuf, ')')) return fail("proc stat pid");
        char pidpath[64];
        snprintf(pidpath, sizeof(pidpath), "/proc/%d/stat", self_pid);
        fd = open(pidpath, O_RDONLY);
        if (fd < 0) return fail("proc pid stat open");
        n = read(fd, sbuf, sizeof(sbuf) - 1);
        close(fd);
        if (n <= 0) return fail("proc pid stat read");
        sbuf[n] = 0;
        if (atoi(sbuf) != self_pid) return fail("proc pid stat match");
        fd = open("/proc/self/maps", O_RDONLY);
        if (fd < 0) return fail("proc maps open");
        n = read(fd, sbuf, sizeof(sbuf) - 1);
        close(fd);
        if (n <= 0) return fail("proc maps read");
        sbuf[n] = 0;
        if (!strstr(sbuf, "[stack]")) return fail("proc maps stack");
        if (!strstr(sbuf, "r-xp") && !strstr(sbuf, "r--p")) return fail("proc maps perms");
        fd = open("/proc/uptime", O_RDONLY);
        if (fd < 0) return fail("proc uptime open");
        n = read(fd, sbuf, sizeof(sbuf) - 1);
        close(fd);
        if (n <= 0) return fail("proc uptime read");
        sbuf[n] = 0;
        if (!strchr(sbuf, '.')) return fail("proc uptime format");
        fd = open("/proc/meminfo", O_RDONLY);
        if (fd < 0) return fail("proc meminfo open");
        n = read(fd, sbuf, sizeof(sbuf) - 1);
        close(fd);
        if (n <= 0) return fail("proc meminfo read");
        sbuf[n] = 0;
        if (!strstr(sbuf, "MemTotal:")) return fail("proc meminfo content");
        char link[128];
        n = readlink("/proc/self/fd/1", link, sizeof(link) - 1);
        if (n <= 0) return fail("proc fd readlink");
        link[n] = 0;
        if (link[0] != '/' && strncmp(link, "pipe:[", 6) != 0 &&
            strncmp(link, "socket:[", 8) != 0 && strncmp(link, "anon_inode", 10) != 0)
            return fail("proc fd link format");
        printf("DIFF_PROCFS_OK\n");
    }

    /* MADV_DONTNEED discards anonymous pages; they read back as zero. */
    {
        char *m = mmap(NULL, 8192, PROT_READ | PROT_WRITE,
                       MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        if (m == MAP_FAILED) return fail("madvise mmap");
        memset(m, 0x7b, 8192);
        if (madvise(m, 8192, MADV_DONTNEED) != 0) return fail("madvise call");
        for (int i = 0; i < 8192; i++) if (m[i] != 0) return fail("madvise dontneed zero");
        if (munmap(m, 8192) != 0) return fail("madvise munmap");
        printf("DIFF_MADVISE_OK\n");
    }

    /* mincore reports residency; MAP_POPULATE makes fresh pages resident. */
    {
        unsigned char *m = mmap(NULL, 12288, PROT_READ | PROT_WRITE,
                                MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        if (m == MAP_FAILED) return fail("mincore mmap");
        m[0] = 1;
        m[4096] = 2;
        unsigned char vec[3] = {9, 9, 9};
        if (mincore(m, 12288, vec) != 0) return fail("mincore call");
        if ((vec[0] & 1) != 1 || (vec[1] & 1) != 1 || (vec[2] & 1) != 0)
            return fail("mincore vec");
        unsigned char *p = mmap(NULL, 8192, PROT_READ | PROT_WRITE,
                                MAP_PRIVATE | MAP_ANONYMOUS | MAP_POPULATE, -1, 0);
        if (p == MAP_FAILED) return fail("populate mmap");
        unsigned char pv[2] = {9, 9};
        if (mincore(p, 8192, pv) != 0) return fail("populate mincore");
        if ((pv[0] & 1) != 1 || (pv[1] & 1) != 1) return fail("populate resident");
        if (munmap(m, 12288) != 0) return fail("mincore munmap");
        if (munmap(p, 8192) != 0) return fail("populate munmap");
        printf("DIFF_MINCORE_OK\n");
    }

    /* flock: exclusive lock excludes a second open, unlocks wake waiters. */
    {
        int fd = open("/tmp/slop-flock", O_CREAT | O_RDWR, 0644);
        if (fd < 0) return fail("flock open");
        if (flock(fd, LOCK_EX | LOCK_NB) != 0) return fail("flock acquire");
        pid_t pid = fork();
        if (pid < 0) return fail("flock fork");
        if (pid == 0) {
            int f2 = open("/tmp/slop-flock", O_RDWR);
            if (f2 < 0) _exit(1);
            errno = 0;
            if (flock(f2, LOCK_EX | LOCK_NB) != -1 || errno != EWOULDBLOCK) _exit(2);
            errno = 0;
            if (flock(f2, LOCK_SH | LOCK_NB) != -1 || errno != EWOULDBLOCK) _exit(3);
            _exit(0);
        }
        int st = 0;
        if (waitpid(pid, &st, 0) != pid || !WIFEXITED(st) || WEXITSTATUS(st) != 0)
            return fail("flock excluded");
        if (flock(fd, LOCK_UN) != 0) return fail("flock unlock");
        pid = fork();
        if (pid < 0) return fail("flock fork2");
        if (pid == 0) {
            int f2 = open("/tmp/slop-flock", O_RDWR);
            if (f2 < 0) _exit(1);
            if (flock(f2, LOCK_EX | LOCK_NB) != 0) _exit(2);
            _exit(0);
        }
        st = 0;
        if (waitpid(pid, &st, 0) != pid || !WIFEXITED(st) || WEXITSTATUS(st) != 0)
            return fail("flock reacquire");
        if (flock(fd, LOCK_SH | LOCK_NB) != 0) return fail("flock shared");
        close(fd);
        unlink("/tmp/slop-flock");
        printf("DIFF_FLOCK_OK\n");
    }

    /* O_NONBLOCK, F_GETFL/F_SETFL, F_GETFD/F_SETFD. */
    {
        int pfd[2];
        if (pipe2(pfd, O_NONBLOCK | O_CLOEXEC) != 0) return fail("pipe2 nonblock");
        if ((fcntl(pfd[1], F_GETFL) & O_NONBLOCK) == 0) return fail("getfl nonblock");
        char c = 0;
        errno = 0;
        if (read(pfd[0], &c, 1) != -1 || errno != EAGAIN) return fail("nonblock eagain");
        if ((fcntl(pfd[0], F_GETFL) & O_NONBLOCK) == 0) return fail("read end nonblock");
        if (fcntl(pfd[0], F_SETFL, 0) != 0) return fail("setfl clear");
        if (fcntl(pfd[0], F_GETFL) & O_NONBLOCK) return fail("still nonblocking");
        if ((fcntl(pfd[0], F_GETFD) & FD_CLOEXEC) != FD_CLOEXEC) return fail("getfd cloexec");
        if (fcntl(pfd[0], F_SETFD, 0) != 0) return fail("setfd clear");
        if (fcntl(pfd[0], F_GETFD) & FD_CLOEXEC) return fail("still cloexec");
        if (write(pfd[1], "x", 1) != 1) return fail("pipe write");
        if (read(pfd[0], &c, 1) != 1 || c != 'x') return fail("pipe read back");
        close(pfd[0]);
        close(pfd[1]);
        printf("DIFF_NONBLOCK_OK\n");
    }

    /* O_APPEND writes land at end even after lseek. */
    {
        int fd = open("/tmp/slop-append", O_CREAT | O_WRONLY | O_TRUNC, 0644);
        if (fd < 0) return fail("append create");
        if (write(fd, "one", 3) != 3) return fail("append write1");
        close(fd);
        fd = open("/tmp/slop-append", O_WRONLY | O_APPEND);
        if (fd < 0) return fail("append reopen");
        if ((fcntl(fd, F_GETFL) & O_APPEND) == 0) return fail("getfl append");
        if (write(fd, "two", 3) != 3) return fail("append write2");
        if (lseek(fd, 0, SEEK_SET) != 0) return fail("append seek");
        if (write(fd, "three", 5) != 5) return fail("append write3");
        close(fd);
        fd = open("/tmp/slop-append", O_RDONLY);
        if (fd < 0) return fail("append read open");
        char buf[32];
        ssize_t n = read(fd, buf, sizeof buf);
        if (n != 11 || memcmp(buf, "onetwothree", 11) != 0) return fail("append content");
        close(fd);
        unlink("/tmp/slop-append");
        printf("DIFF_APPEND_OK\n");
    }

    /* FD_CLOEXEC descriptors must vanish across execve. */
    {
        char self[512];
        ssize_t sn = readlink("/proc/self/exe", self, sizeof self - 1);
        if (sn <= 0 || (size_t)sn >= sizeof self - 1) return fail("exe readlink");
        self[sn] = 0;
        int fd = open("/tmp/slop-cloexec", O_CREAT | O_RDWR, 0644);
        if (fd < 0) return fail("cloexec open");
        if (dup2(fd, 9) != 9) return fail("cloexec dup2");
        if (fcntl(9, F_SETFD, FD_CLOEXEC) != 0) return fail("cloexec setfd");
        pid_t pid = fork();
        if (pid < 0) return fail("cloexec fork");
        if (pid == 0) {
            char *nargv[] = { self, "cloexec-child", NULL };
            execv(self, nargv);
            _exit(97);
        }
        int st = 0;
        if (waitpid(pid, &st, 0) != pid || !WIFEXITED(st) || WEXITSTATUS(st) != 42)
            return fail("cloexec exec");
        if (fcntl(9, F_GETFD) < 0) return fail("parent fd kept");
        close(9);
        close(fd);
        unlink("/tmp/slop-cloexec");
        printf("DIFF_CLOEXEC_OK\n");
    }


    /* POSIX timers: signal notification, si_value, overrun and lifetime. */
    {
        sigset_t set;
        sigemptyset(&set);
        sigaddset(&set, SIGUSR1);
        if (sigprocmask(SIG_BLOCK, &set, NULL) != 0) return fail("timer sigmask");
        struct sigevent sev;
        memset(&sev, 0, sizeof sev);
        sev.sigev_notify = SIGEV_SIGNAL;
        sev.sigev_signo = SIGUSR1;
        sev.sigev_value.sival_int = 0x12345678;
        timer_t tid = 0;
        if (timer_create(CLOCK_MONOTONIC, &sev, &tid) != 0) return fail("timer_create");
        struct itimerspec its;
        memset(&its, 0, sizeof its);
        its.it_value.tv_nsec = 20 * 1000 * 1000;
        if (timer_settime(tid, 0, &its, NULL) != 0) return fail("timer_settime");
        siginfo_t si;
        memset(&si, 0, sizeof si);
        int got = sigwaitinfo(&set, &si);
        if (got != SIGUSR1) return fail("sigwaitinfo");
        if (si.si_code != SI_TIMER) return fail("si_code timer");
        if (si.si_value.sival_int != 0x12345678) return fail("si_value timer");
        if (timer_getoverrun(tid) < 0) return fail("timer_getoverrun");
        struct itimerspec cur;
        memset(&cur, 0, sizeof cur);
        if (timer_gettime(tid, &cur) != 0) return fail("timer_gettime");
        if (cur.it_value.tv_sec != 0 || cur.it_value.tv_nsec != 0) return fail("timer one-shot");
        if (timer_delete(tid) != 0) return fail("timer_delete");
        if (timer_delete(tid) == 0) return fail("timer_delete twice");
        errno = 0;
        if (timer_create((clockid_t)0x7fffffff, &sev, &tid) != -1 || errno != EINVAL)
            return fail("timer bad clock");
        sigemptyset(&set);
        if (sigprocmask(SIG_SETMASK, &set, NULL) != 0) return fail("timer unmask");
        printf("DIFF_TIMER_OK\n");
    }

    /* setitimer/alarm on ITIMER_REAL share one per-process timer. */
    {
        sigset_t set;
        sigemptyset(&set);
        sigaddset(&set, SIGALRM);
        if (sigprocmask(SIG_BLOCK, &set, NULL) != 0) return fail("itimer sigmask");
        struct itimerval itv;
        memset(&itv, 0, sizeof itv);
        itv.it_value.tv_usec = 30 * 1000;
        struct itimerval old;
        memset(&old, 0xff, sizeof old);
        if (setitimer(ITIMER_REAL, &itv, &old) != 0) return fail("setitimer");
        if (old.it_value.tv_sec != 0 || old.it_value.tv_usec != 0) return fail("setitimer old");
        siginfo_t si;
        memset(&si, 0, sizeof si);
        int got = sigwaitinfo(&set, &si);
        if (got != SIGALRM) return fail("itimer sigwait");
        struct itimerval cur_itv;
        memset(&cur_itv, 0, sizeof cur_itv);
        if (getitimer(ITIMER_REAL, &cur_itv) != 0) return fail("getitimer");
        if (cur_itv.it_value.tv_sec != 0 || cur_itv.it_value.tv_usec != 0) return fail("itimer remaining");
        unsigned remain = alarm(5);
        if (remain != 0) return fail("alarm start");
        remain = alarm(0);
        if (remain != 5) return fail("alarm cancel");
        sigemptyset(&set);
        if (sigprocmask(SIG_SETMASK, &set, NULL) != 0) return fail("itimer unmask");
        printf("DIFF_ITIMER_OK\n");
    }

    /* Clock surface: resolutions, readable realtime, monotonic not settable. */
    {
        struct timespec res, ts;
        if (clock_getres(CLOCK_REALTIME, &res) != 0) return fail("getres realtime");
        if (res.tv_sec < 0 || res.tv_nsec < 0 || res.tv_nsec >= 1000000000L) return fail("getres range");
        if (clock_getres((clockid_t)0x7fffffff, &res) != -1 || errno != EINVAL)
            return fail("getres bad clock");
        if (clock_gettime(CLOCK_REALTIME, &ts) != 0) return fail("gettime realtime");
        if (ts.tv_sec < 100000 || ts.tv_nsec < 0 || ts.tv_nsec >= 1000000000L)
            return fail("realtime value");
        memset(&ts, 0, sizeof ts);
        if (clock_settime(CLOCK_MONOTONIC, &ts) != -1 || errno != EINVAL)
            return fail("settime monotonic");
        printf("DIFF_CLOCK_OK\n");
    }

    /* Robust list round trip through the raw syscalls. */
    {
        unsigned long head = 0, len = 0;
        if (syscall(SYS_get_robust_list, 0, &head, &len) != 0) return fail("get_robust_list");
        if (len != 24) return fail("robust len");
        unsigned long fake[3] = { 0, 0, 0 };
        if (syscall(SYS_set_robust_list, fake, 24) != 0) return fail("set_robust_list");
        head = 0;
        len = 0;
        if (syscall(SYS_get_robust_list, 0, &head, &len) != 0) return fail("get_robust_list 2");
        if (head != (unsigned long)fake || len != 24) return fail("robust roundtrip");
        if (syscall(SYS_set_robust_list, fake, 17) != -1 || errno != EINVAL)
            return fail("set_robust_list bad len");
        printf("DIFF_ROBUST_OK\n");
    }

    /* Pending-mask introspection and zero-timeout waits. */
    {
        sigset_t set;
        sigemptyset(&set);
        sigaddset(&set, SIGUSR2);
        if (sigprocmask(SIG_BLOCK, &set, NULL) != 0) return fail("sigpend block");
        if (raise(SIGUSR2) != 0) return fail("sigpend raise");
        sigset_t pend;
        if (sigpending(&pend) != 0) return fail("sigpending");
        if (!sigismember(&pend, SIGUSR2)) return fail("sigpend member");
        struct timespec zero;
        memset(&zero, 0, sizeof zero);
        int got = sigtimedwait(&set, NULL, &zero);
        if (got != SIGUSR2) return fail("sigtimedwait zero");
        errno = 0;
        got = sigtimedwait(&set, NULL, &zero);
        if (got != -1 || errno != EAGAIN) return fail("sigtimedwait empty");
        sigemptyset(&set);
        if (sigprocmask(SIG_SETMASK, &set, NULL) != 0) return fail("sigpend unmask");
        printf("DIFF_SIGPEND_OK\n");
    }

    int memfd = memfd_create("diff", 0);
    if (memfd < 0) return fail("memfd_create");
    close(memfd);

    printf("DIFF_DONE\n");
    return 0;
}
