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
#include <dirent.h>
#include <errno.h>
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

int main(void) {
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

    int memfd = memfd_create("diff", 0);
    if (memfd < 0) return fail("memfd_create");
    close(memfd);

    printf("DIFF_DONE\n");
    return 0;
}
