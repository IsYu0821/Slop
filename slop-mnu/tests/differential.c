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
#include <sys/un.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <sys/timerfd.h>
#include <sys/signalfd.h>
#include <sys/sysinfo.h>
#include <sys/syscall.h>
#include <pthread.h>
#include <poll.h>
#include <signal.h>
#include <dirent.h>
#include <errno.h>
#include <sys/file.h>
#include <time.h>
#include <stdint.h>

/* Local statx layout (published UAPI shape) for toolchains whose
   headers predate it; the kernel fills mask/blksize/mode/ino/size. */
struct slop_statx_ts { int64_t tv_sec; uint32_t tv_nsec; uint32_t __reserved; };
struct slop_statx {
    uint32_t stx_mask, stx_blksize, stx_attributes;
    uint32_t stx_nlink, stx_uid, stx_gid;
    uint16_t stx_mode; uint16_t __spare0;
    uint64_t stx_ino, stx_size, stx_blocks, stx_attributes_mask;
    struct slop_statx_ts stx_atime, stx_btime, stx_ctime, stx_mtime;
    uint32_t stx_rdev_major, stx_rdev_minor, stx_dev_major, stx_dev_minor;
    uint64_t __spare2[14];
};

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

/* pthread section helpers. */
static volatile long pt_counter;
static pthread_mutex_t pt_mtx = PTHREAD_MUTEX_INITIALIZER;
static pthread_mutex_t pt_done_mtx = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t pt_cond = PTHREAD_COND_INITIALIZER;
static int pt_ready;
static volatile sig_atomic_t pt_got_usr1, pt_got_tid;
static __thread int pt_tls_slot;

/* A handler that records the value of a real-time signal. */
static volatile int rt_hits;
static volatile int rt_value = -1;
static void rt_handler(int sig, siginfo_t *si, void *uc) {
    (void)sig; (void)uc;
    rt_hits++;
    /* The value of a queued signal lives in the _rt member of the
       information, thirty two bytes into the union: the two time fields,
       then the value. */
    rt_value = si ? si->si_value.sival_int : -1;
}

/* A handler that records where it ran. */
static volatile int alt_hits;
static volatile char *alt_sp;
static char altstack[64 * 1024] __attribute__((aligned(16)));
static void alt_handler(int sig) {
    (void)sig;
    alt_sp = (char *)__builtin_frame_address(0);
    alt_hits++;
}

static void *pt_worker(void *arg) {
    long n = (long)arg;
    for (long i = 0; i < n; i++) {
        pthread_mutex_lock(&pt_mtx);
        pt_counter++;
        pthread_mutex_unlock(&pt_mtx);
    }
    return (void *)(n * 2);
}

static void *pt_waiter(void *arg) {
    (void)arg;
    pthread_mutex_lock(&pt_done_mtx);
    while (!pt_ready) pthread_cond_wait(&pt_cond, &pt_done_mtx);
    pthread_mutex_unlock(&pt_done_mtx);
    return (void *)42;
}

static void pt_usr1(int sig) {
    (void)sig;
    pt_got_usr1 = 1;
    pt_got_tid = (sig_atomic_t)syscall(SYS_gettid);
}

static void *pt_sleeper(void *arg) {
    (void)arg;
    while (!pt_got_usr1) usleep(10000);
    return (void *)(long)pt_got_tid;
}

static void *pt_tls_worker(void *arg) {
    long n = (long)arg;
    pt_tls_slot = (int)(n * 7);
    for (int i = 0; i < 20; i++) usleep(2000);
    return (void *)(long)pt_tls_slot;
}

/* Stopped/continued job control and the ucontext floating-point state. */
static volatile sig_atomic_t fp_hits;
static volatile int fp_fpregs_ok;
static volatile int fp_mxcsr_ok;
static void fp_handler(int sig, siginfo_t *si, void *ctx) {
    (void)sig; (void)si;
    ucontext_t *uc = (ucontext_t *)ctx;
    if (!uc) return;
    /* The machine context carries the floating-point area: the kernel
       publishes the FXSAVE layout with the SSE registers at offset 160. */
    void *fp = (void *)uc->uc_mcontext.fpregs;
    unsigned char *mem = 0;
    if (fp) mem = (unsigned char *)fp;
    else mem = uc->__fpregs_mem;
    if (!mem) return;
    /* MXCSR reset value 0x1f80 lives at offset 24. */
    if (mem[24] == 0x80 && mem[25] == 0x1f) fp_mxcsr_ok = 1;
    fp_fpregs_ok = (fp != 0);
    /* Publish a known SSE pattern for the interrupted context: the kernel
       restores it through rt_sigreturn, so the pattern survives the call. */
    static const unsigned char pat[16] __attribute__((aligned(16))) =
        {0x22,0x22,0x22,0x22,0x22,0x22,0x22,0x22,0x22,0x22,0x22,0x22,0x22,0x22,0x22,0x22};
    __builtin_memcpy(mem + 160, pat, 16);
    fp_hits++;
}

static int fail(const char *what) {
    printf("DIFF_FAIL: %s (errno=%d)\n", what, errno);
    return 1;
}

int main(int argc, char **argv) {
    static char g_bindir[128];
    {
        /* Remember where the test binary lives: later sections chdir("/"),
           so a relative helper directory would stop resolving. */
        char startcwd[128] = {0};
        if (getcwd(startcwd, sizeof startcwd) == NULL) startcwd[0] = 0;
        const char *a0 = (argc > 0 && argv[0]) ? argv[0] : "";
        const char *slash = strrchr(a0, '/');
        if (slash && slash != a0 && (size_t)(slash - a0) < sizeof g_bindir - 8) {
            size_t dl = (size_t)(slash - a0);
            if (a0[0] == '/') snprintf(g_bindir, sizeof g_bindir, "%.*s", (int)dl, a0);
            else if (startcwd[0]) snprintf(g_bindir, sizeof g_bindir, "%s/%.*s", startcwd, (int)dl, a0);
        } else if (startcwd[0]) {
            snprintf(g_bindir, sizeof g_bindir, "%s/build", startcwd);
        }
    }
    sigset_t rt_set;
    sigemptyset(&rt_set);
    sigaddset(&rt_set, SIGRTMIN);
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

    /* The real-time signals: a signal of the range 32 to 64, a value that
       travels with it, and more than one instance of it waiting. */
    {
        struct sigaction sa;
        memset(&sa, 0, sizeof sa);
        sa.sa_sigaction = rt_handler;
        sa.sa_flags = SA_SIGINFO;
        sigemptyset(&sa.sa_mask);
        if (sigaction(SIGRTMIN, &sa, 0) != 0) return fail("rt sigaction");
        rt_hits = 0;
        rt_value = -1;
        /* The value travels with the signal. */
        union sigval sv;
        sv.sival_int = 0x1234;
        /* The signal is blocked, so it is unblocked for delivery. */
        sigset_t old;
        sigemptyset(&old);
        sigprocmask(SIG_UNBLOCK, &rt_set, &old);
        /* The signal is unblocked, so it is delivered by the kernel. */
        errno = 0;
        errno = 0;
        siginfo_t si;
        memset(&si, 0, sizeof si);
        si.si_signo = SIGRTMIN;
        si.si_code = SI_QUEUE;
        si.si_value.sival_int = 0x1234;
        int rq = syscall(129, getpid(), SIGRTMIN, &si);
        printf("sigqueue ret=%d errno=%d\n", rq, errno);
        if (rq != 0) return fail("rt sigqueue");
        /* The signal is sent to the current process, so the target is
           the current thread group. */
        struct timespec ts;
        memset(&ts, 0, sizeof ts);
        ts.tv_sec = 2;
        /* The signal was delivered by the handler. */
        if (rt_hits != 1) return fail("rt hits");
        if (rt_value != 0x1234) return fail("rt value");
        /* The signal has been delivered, so the mask is restored. */
        sigprocmask(SIG_SETMASK, &old, NULL);
        /* The signal is blocked again, so nothing else is delivered. */
        sigprocmask(SIG_BLOCK, &rt_set, NULL);
        sigprocmask(SIG_SETMASK, &old, NULL);
        if (rt_hits != 1) return fail("rt hits");
        if (rt_value != 0x1234) return fail("rt value");
        /* Two instances of the same signal wait at the same time. */
        rt_hits = 0;
        sv.sival_int = 0x11;
        if (sigqueue(getpid(), SIGRTMIN, sv) != 0) return fail("rt sigqueue 2");
        sv.sival_int = 0x22;
        if (sigqueue(getpid(), SIGRTMIN, sv) != 0) return fail("rt sigqueue 3");
        memset(&ts, 0, sizeof ts);
        ts.tv_sec = 2;
        /* The second instance was already consumed by the handler. */
        if (rt_hits != 2) return fail("rt hits 2");
        /* A signal outside the range is refused. */
        errno = 0;
        if (sigqueue(getpid(), 65, sv) != -1 || errno != EINVAL)
            return fail("rt range");
        printf("DIFF_RTSIG_OK\n");
    }

    /* The alternate stack: a handler that asked for one runs on it. */
    {
        stack_t ss, old;
        memset(&ss, 0, sizeof ss);
        ss.ss_sp = altstack;
        ss.ss_size = sizeof altstack;
        ss.ss_flags = 0;
        if (sigaltstack(&ss, NULL) != 0) return fail("sigaltstack");
        if (old.ss_flags & SS_DISABLE) return fail("sigaltstack old");
        struct sigaction sa;
        memset(&sa, 0, sizeof sa);
        sa.sa_handler = alt_handler;
        sa.sa_flags = SA_ONSTACK;
        sigemptyset(&sa.sa_mask);
        if (sigaction(SIGUSR1, &sa, 0) != 0) return fail("altstack sigaction");
        alt_hits = 0;
        if (raise(SIGUSR1) != 0) return fail("altstack raise");
        if (alt_hits != 1) return fail("altstack hits");
        if (alt_sp < (char *)altstack || alt_sp >= (char *)altstack + sizeof altstack)
            return fail("altstack position");
        /* A stack that is too small is refused. */
        stack_t small;
        memset(&small, 0, sizeof small);
        small.ss_sp = altstack;
        small.ss_size = 16;
        errno = 0;
        if (sigaltstack(&small, 0) != -1 || errno != ENOMEM) return fail("altstack small");
        printf("DIFF_ALTSTACK_OK\n");
    }

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

    /* POSIX record locks: cross-process conflict, F_GETLK owner reporting,
       blocking F_SETLKW wakeup, release on child exit and on any close. */
    {
        int fd = open("/tmp/slop-rlock", O_CREAT | O_RDWR | O_TRUNC, 0644);
        if (fd < 0) return fail("rlock create");
        char fill[128];
        memset(fill, 'a', sizeof fill);
        if (write(fd, fill, sizeof fill) != (ssize_t)sizeof fill) return fail("rlock fill");

        struct flock fl;
        memset(&fl, 0, sizeof fl);
        fl.l_type = F_WRLCK;
        fl.l_whence = SEEK_SET;
        fl.l_start = 10;
        fl.l_len = 20;
        if (fcntl(fd, F_SETLK, &fl) != 0) return fail("rlock set");
        fl.l_type = F_WRLCK;
        if (fcntl(fd, F_GETLK, &fl) != 0) return fail("rlock getlk self");
        if (fl.l_type != F_UNLCK) return fail("rlock self conflict");

        int sync_pipe[2];
        if (pipe(sync_pipe) != 0) return fail("rlock pipe");
        pid_t ch = fork();
        if (ch < 0) return fail("rlock fork");
        if (ch == 0) {
            close(sync_pipe[0]);
            struct flock c;
            memset(&c, 0, sizeof c);
            c.l_type = F_WRLCK;
            c.l_whence = SEEK_SET;
            c.l_start = 20;
            c.l_len = 30;
            errno = 0;
            if (fcntl(fd, F_SETLK, &c) != -1 || errno != EAGAIN) _exit(1);
            memset(&c, 0, sizeof c);
            c.l_type = F_WRLCK;
            if (fcntl(fd, F_GETLK, &c) != 0) _exit(2);
            if (c.l_type != F_WRLCK || c.l_start != 10 || c.l_len != 20 ||
                c.l_pid != getppid()) _exit(3);
            /* Tell the parent we are about to park in F_SETLKW. */
            if (write(sync_pipe[1], "1", 1) != 1) _exit(9);
            memset(&c, 0, sizeof c);
            c.l_type = F_WRLCK;
            c.l_whence = SEEK_SET;
            c.l_start = 10;
            c.l_len = 20;
            if (fcntl(fd, F_SETLKW, &c) != 0) _exit(4);
            memset(&c, 0, sizeof c);
            c.l_type = F_WRLCK;
            if (fcntl(fd, F_SETLK, &c) != 0) _exit(5);
            _exit(0);
        }
        close(sync_pipe[1]);
        char sc;
        if (read(sync_pipe[0], &sc, 1) != 1) return fail("rlock sync");
        close(sync_pipe[0]);
        usleep(100000);
        memset(&fl, 0, sizeof fl);
        fl.l_type = F_UNLCK;
        fl.l_whence = SEEK_SET;
        fl.l_start = 10;
        fl.l_len = 20;
        if (fcntl(fd, F_SETLK, &fl) != 0) return fail("rlock unlock");
        int st;
        if (waitpid(ch, &st, 0) != ch || !WIFEXITED(st) || WEXITSTATUS(st) != 0)
            return fail("rlock child");

        memset(&fl, 0, sizeof fl);
        fl.l_type = F_WRLCK;
        if (fcntl(fd, F_SETLK, &fl) != 0) return fail("rlock child release");
        int fd2 = open("/tmp/slop-rlock", O_RDWR);
        if (fd2 < 0) return fail("rlock reopen");
        close(fd2);
        pid_t ch2 = fork();
        if (ch2 < 0) return fail("rlock fork2");
        if (ch2 == 0) {
            struct flock c;
            memset(&c, 0, sizeof c);
            c.l_type = F_WRLCK;
            errno = 0;
            if (fcntl(fd, F_SETLK, &c) != 0) _exit(6);
            _exit(0);
        }
        if (waitpid(ch2, &st, 0) != ch2 || !WIFEXITED(st) || WEXITSTATUS(st) != 0)
            return fail("rlock close release");
        close(fd);
        unlink("/tmp/slop-rlock");
        printf("DIFF_RECORDLOCK_OK\n");
    }

    /* OFD locks: independent descriptions conflict in one process, dup shares
       the owner, and the lock lives until the description's last close. */
    {
        int a = open("/tmp/slop-ofdlock", O_CREAT | O_RDWR | O_TRUNC, 0644);
        int b = open("/tmp/slop-ofdlock", O_RDWR);
        if (a < 0 || b < 0) return fail("ofd open");
        char fill[128];
        memset(fill, 'b', sizeof fill);
        if (write(a, fill, sizeof fill) != (ssize_t)sizeof fill) return fail("ofd fill");
        struct flock fl;
        memset(&fl, 0, sizeof fl);
        fl.l_type = F_WRLCK;
        fl.l_whence = SEEK_SET;
        fl.l_start = 0;
        fl.l_len = 50;
        if (fcntl(a, F_OFD_SETLK, &fl) != 0) return fail("ofd set");
        errno = 0;
        if (fcntl(b, F_OFD_SETLK, &fl) != -1 || errno != EAGAIN) return fail("ofd conflict");
        memset(&fl, 0, sizeof fl);
        fl.l_type = F_WRLCK;
        fl.l_start = 0;
        fl.l_len = 10;
        if (fcntl(b, F_OFD_GETLK, &fl) != 0) return fail("ofd getlk");
        if (fl.l_type != F_WRLCK || fl.l_start != 0 || fl.l_len != 50)
            return fail("ofd getlk body");
        int c = dup(a);
        if (c < 0) return fail("ofd dup");
        memset(&fl, 0, sizeof fl);
        fl.l_type = F_WRLCK;
        fl.l_start = 0;
        fl.l_len = 50;
        if (fcntl(c, F_OFD_SETLK, &fl) != 0) return fail("ofd shared owner");
        close(c);
        memset(&fl, 0, sizeof fl);
        fl.l_type = F_WRLCK;
        fl.l_start = 40;
        fl.l_len = 5;
        if (fcntl(b, F_OFD_GETLK, &fl) != 0) return fail("ofd getlk after dup close");
        if (fl.l_type != F_WRLCK) return fail("ofd lock lost");
        close(a);
        memset(&fl, 0, sizeof fl);
        fl.l_type = F_WRLCK;
        fl.l_start = 0;
        fl.l_len = 10;
        if (fcntl(b, F_OFD_GETLK, &fl) != 0) return fail("ofd getlk released");
        if (fl.l_type != F_UNLCK) return fail("ofd last close release");
        memset(&fl, 0, sizeof fl);
        fl.l_type = F_WRLCK;
        fl.l_start = 0;
        fl.l_len = 10;
        if (fcntl(b, F_OFD_SETLK, &fl) != 0) return fail("ofd reacquire");
        close(b);
        unlink("/tmp/slop-ofdlock");
        printf("DIFF_OFDLOCK_OK\n");
    }

    /* Robust futexes: an exiting task marks still-owned words FUTEX_OWNER_DIED
       and wakes a waiter blocked on the word. */
    {
        struct robust_list { struct robust_list *next; };
        struct robust_list_head { struct robust_list list; long futex_offset; struct robust_list *list_op_pending; };
        struct robust_area { struct robust_list elem; int word; int holder; } *sh;
        sh = mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_SHARED | MAP_ANONYMOUS, -1, 0);
        if (sh == MAP_FAILED) return fail("robust mmap");
        memset(sh, 0, sizeof *sh);
        int ready[2];
        if (pipe(ready) != 0) return fail("robust pipe");
        pid_t ch = fork();
        if (ch < 0) return fail("robust fork");
        if (ch == 0) {
            close(ready[0]);
            struct robust_list_head head;
            long tid = syscall(SYS_gettid);
            sh->holder = (int)tid;
            sh->word = (int)tid;
            sh->elem.next = &head.list;
            head.list.next = &sh->elem;
            head.futex_offset = (long)((char *)&sh->word - (char *)&sh->elem);
            head.list_op_pending = NULL;
            if (syscall(SYS_set_robust_list, &head, sizeof head) != 0) _exit(7);
            char c = 'x';
            if (write(ready[1], &c, 1) != 1) _exit(8);
            usleep(150000);
            _exit(0);
        }
        close(ready[1]);
        char c;
        if (read(ready[0], &c, 1) != 1) return fail("robust handshake");
        int tid = sh->holder;
        /* Contended robust waiters set FUTEX_WAITERS, as glibc does; the
           kernel only wakes such waiters when marking OWNER_DIED. */
        int expect = tid | 0x80000000;
        sh->word = expect;
        struct timespec to;
        to.tv_sec = 2;
        to.tv_nsec = 0;
        errno = 0;
        long r = syscall(SYS_futex, &sh->word, 0, expect, &to, NULL, 0);
        if (r != 0 && errno == ETIMEDOUT) return fail("robust wake");
        if (r != 0 && errno != EAGAIN) return fail("robust wait");
        int st;
        if (waitpid(ch, &st, 0) != ch || !WIFEXITED(st) || WEXITSTATUS(st) != 0)
            return fail("robust child");
        if ((sh->word & 0x3fffffff) != 0 || (sh->word & 0x40000000) == 0)
            return fail("robust owner died");
        munmap(sh, 4096);
        printf("DIFF_ROBUSTFUTEX_OK\n");
    }

    /* CPU-time accounting: process and thread CPU clocks track consumed CPU. */
    {
        struct timespec w0, c0, ct0;
        if (clock_gettime(CLOCK_MONOTONIC, &w0) != 0) return fail("cputime mono0");
        if (clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &c0) != 0) return fail("cputime proc0");
        if (clock_gettime(CLOCK_THREAD_CPUTIME_ID, &ct0) != 0) return fail("cputime thr0");
        volatile unsigned long spin = 0;
        for (;;) {
            struct timespec n;
            spin++;
            clock_gettime(CLOCK_MONOTONIC, &n);
            if ((n.tv_sec - w0.tv_sec) * 1000 + (n.tv_nsec - w0.tv_nsec) / 1000000 >= 150)
                break;
        }
        struct timespec w1, c1, ct1;
        if (clock_gettime(CLOCK_MONOTONIC, &w1) != 0) return fail("cputime mono1");
        if (clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &c1) != 0) return fail("cputime proc1");
        if (clock_gettime(CLOCK_THREAD_CPUTIME_ID, &ct1) != 0) return fail("cputime thr1");
        long long cms = (c1.tv_sec - c0.tv_sec) * 1000 + (c1.tv_nsec - c0.tv_nsec) / 1000000;
        long long wms = (w1.tv_sec - w0.tv_sec) * 1000 + (w1.tv_nsec - w0.tv_nsec) / 1000000;
        long long tms = (ct1.tv_sec - ct0.tv_sec) * 1000 + (ct1.tv_nsec - ct0.tv_nsec) / 1000000;
        if (cms < 30) return fail("cputime process clock");
        if (cms > wms + 50) return fail("cputime exceeds wall");
        if (tms < 30) return fail("cputime thread clock");
        struct rusage ru;
        memset(&ru, 0, sizeof ru);
        if (getrusage(RUSAGE_SELF, &ru) != 0) return fail("cputime rusage self");
        long long rut = (long long)ru.ru_utime.tv_sec * 1000 + ru.ru_utime.tv_usec / 1000;
        if (rut < 30) return fail("cputime rusage utime");
        if (getrusage(RUSAGE_THREAD, &ru) != 0) return fail("cputime rusage thread");
        /* /proc/self/stat fields 14/15 are utime/stime in USER_HZ ticks. */
        int fd = open("/proc/self/stat", O_RDONLY);
        if (fd < 0) return fail("cputime stat open");
        char sbuf[1024];
        ssize_t sn = read(fd, sbuf, sizeof(sbuf) - 1);
        close(fd);
        if (sn <= 0) return fail("cputime stat read");
        sbuf[sn] = 0;
        char *sp = strchr(sbuf, ')');
        if (!sp) return fail("cputime stat close");
        sp++;
        while (*sp == ' ') sp++;
        while (*sp && *sp != ' ') sp++;
        long utime_ticks = 0, stime_ticks = 0;
        for (int i = 0; i < 12; i++) {
            while (*sp == ' ') sp++;
            long v = strtol(sp, &sp, 10);
            if (i == 10) utime_ticks = v;
            if (i == 11) stime_ticks = v;
        }
        if (utime_ticks <= 0) return fail("cputime stat utime");
        if (stime_ticks < 0) return fail("cputime stat stime");
        printf("DIFF_CPUTIME_OK\n");
    }

    /* wait4 reports the child's CPU usage; RUSAGE_CHILDREN accumulates it. */
    {
        int pid = fork();
        if (pid == 0) {
            struct timespec s0;
            clock_gettime(CLOCK_MONOTONIC, &s0);
            volatile unsigned long x = 0;
            for (;;) {
                struct timespec n;
                x++;
                clock_gettime(CLOCK_MONOTONIC, &n);
                if ((n.tv_sec - s0.tv_sec) * 1000 + (n.tv_nsec - s0.tv_nsec) / 1000000 >= 120)
                    break;
            }
            _exit(0);
        }
        if (pid < 0) return fail("rusage fork");
        struct rusage cru, chr;
        memset(&cru, 0, sizeof cru);
        int status = 0;
        if (wait4(pid, &status, 0, &cru) != pid) return fail("rusage wait4");
        if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) return fail("rusage child status");
        long long cut = (long long)cru.ru_utime.tv_sec * 1000 + cru.ru_utime.tv_usec / 1000;
        if (cut < 20) return fail("rusage child utime");
        memset(&chr, 0, sizeof chr);
        if (getrusage(RUSAGE_CHILDREN, &chr) != 0) return fail("rusage children");
        long long ct = (long long)chr.ru_utime.tv_sec * 1000 + chr.ru_utime.tv_usec / 1000;
        if (ct < 20) return fail("rusage children utime");
        printf("DIFF_RUSAGE_OK\n");
    }

    /* ITIMER_VIRTUAL/ITIMER_PROF fire SIGVTALRM/SIGPROF on consumed CPU. */
    {
        sigset_t set, oldset;
        sigemptyset(&set);
        sigaddset(&set, SIGVTALRM);
        sigaddset(&set, SIGPROF);
        if (sigprocmask(SIG_BLOCK, &set, &oldset) != 0) return fail("vtimer block");
        struct itimerval it;
        memset(&it, 0, sizeof it);
        it.it_value.tv_usec = 30000;
        if (setitimer(ITIMER_VIRTUAL, &it, NULL) != 0) return fail("vtimer set");
        if (setitimer(3, &it, NULL) != -1 || errno != EINVAL) return fail("vtimer which");
        struct timespec s0;
        clock_gettime(CLOCK_MONOTONIC, &s0);
        volatile unsigned long x = 0;
        for (;;) {
            struct timespec n;
            x++;
            clock_gettime(CLOCK_MONOTONIC, &n);
            if ((n.tv_sec - s0.tv_sec) * 1000 + (n.tv_nsec - s0.tv_nsec) / 1000000 >= 120)
                break;
        }
        struct timespec tw;
        tw.tv_sec = 3;
        tw.tv_nsec = 0;
        if (sigtimedwait(&set, NULL, &tw) != SIGVTALRM) return fail("vtimer fire");
        memset(&it, 0, sizeof it);
        if (setitimer(ITIMER_VIRTUAL, &it, NULL) != 0) return fail("vtimer clear");
        memset(&it, 0, sizeof it);
        it.it_value.tv_usec = 30000;
        if (setitimer(ITIMER_PROF, &it, NULL) != 0) return fail("prof set");
        clock_gettime(CLOCK_MONOTONIC, &s0);
        for (;;) {
            struct timespec n;
            x++;
            clock_gettime(CLOCK_MONOTONIC, &n);
            if ((n.tv_sec - s0.tv_sec) * 1000 + (n.tv_nsec - s0.tv_nsec) / 1000000 >= 120)
                break;
        }
        if (sigtimedwait(&set, NULL, &tw) != SIGPROF) return fail("prof fire");
        memset(&it, 0, sizeof it);
        if (setitimer(ITIMER_PROF, &it, NULL) != 0) return fail("prof clear");
        if (sigprocmask(SIG_SETMASK, &oldset, NULL) != 0) return fail("vtimer unblock");
        printf("DIFF_VTIMER_OK\n");
    }

    /* pthreads: create/join, contended mutex, condvar, pthread_kill, TLS. */
    {
        pthread_t t[4];
        long sums[4];
        for (int i = 0; i < 4; i++) {
            sums[i] = 100000 + i;
            if (pthread_create(&t[i], 0, pt_worker, (void *)sums[i])) return fail("pthread create");
        }
        long total = 0, expect = 0;
        for (int i = 0; i < 4; i++) {
            void *r;
            if (pthread_join(t[i], &r)) return fail("pthread join");
            total += (long)r;
            expect += sums[i] * 2;
        }
        if (total != expect) return fail("pthread retval");
        if (pt_counter != 400006) return fail("pthread mutex");
        printf("DIFF_PTHREAD_OK\n");

        pthread_t w;
        if (pthread_create(&w, 0, pt_waiter, 0)) return fail("cond create");
        usleep(50000);
        pthread_mutex_lock(&pt_done_mtx);
        pt_ready = 1;
        pthread_cond_signal(&pt_cond);
        pthread_mutex_unlock(&pt_done_mtx);
        void *r;
        if (pthread_join(w, &r)) return fail("cond join");
        if ((long)r != 42) return fail("cond retval");
        printf("DIFF_COND_OK\n");

        struct sigaction sa;
        memset(&sa, 0, sizeof sa);
        sa.sa_handler = pt_usr1;
        sa.sa_flags = SA_RESTART;
        if (sigaction(SIGUSR1, &sa, 0)) return fail("kill sigaction");
        pthread_t sl;
        if (pthread_create(&sl, 0, pt_sleeper, 0)) return fail("kill create");
        usleep(50000);
        if (pthread_kill(sl, SIGUSR1)) return fail("kill send");
        if (pthread_join(sl, &r)) return fail("kill join");
        if (!pt_got_usr1 || (long)r <= 0) return fail("kill delivery");
        printf("DIFF_KILL_OK\n");

        pthread_t tt[3];
        for (int i = 0; i < 3; i++)
            if (pthread_create(&tt[i], 0, pt_tls_worker, (void *)(long)(i + 1))) return fail("tls create");
        for (int i = 0; i < 3; i++) {
            if (pthread_join(tt[i], &r)) return fail("tls join");
            if ((long)r != (i + 1) * 7) return fail("tls value");
        }
        printf("DIFF_TLS_OK\n");
    }

    /* Stopped-child wait status: a child that stops itself reports
       WIFSTOPPED with WUNTRACED, then WIFCONTINUED with WCONTINUED. */
    {
        pid_t c = fork();
        if (c < 0) return fail("stop fork");
        if (c == 0) {
            raise(SIGSTOP);
            sleep(1);
            _exit(43);
        }
        int st;
        if (waitpid(c, &st, WUNTRACED) != c) return fail("stop wait");
        if (!WIFSTOPPED(st) || WSTOPSIG(st) != SIGSTOP) return fail("stop status");
        if (kill(c, SIGCONT) != 0) return fail("cont send");
        if (waitpid(c, &st, WCONTINUED) != c) return fail("cont wait");
        if (!WIFCONTINUED(st)) return fail("cont status");
        if (waitpid(c, &st, 0) != c) return fail("stop reap");
        if (!WIFEXITED(st) || WEXITSTATUS(st) != 43) return fail("stop exit");
        printf("DIFF_STOP_OK\n");
    }

    /* Floating-point state in the ucontext: the handler sees the FXSAVE
       area through fpregs and a pattern it writes there survives. */
    {
        struct sigaction sa;
        memset(&sa, 0, sizeof sa);
        sa.sa_sigaction = fp_handler;
        sa.sa_flags = SA_SIGINFO | SA_RESTART;
        sigemptyset(&sa.sa_mask);
        if (sigaction(SIGUSR2, &sa, 0) != 0) return fail("fp sigaction");
        fp_hits = 0; fp_fpregs_ok = 0; fp_mxcsr_ok = 0;
        if (raise(SIGUSR2) != 0) return fail("fp raise");
        if (fp_hits != 1) return fail("fp hits");
        if (!fp_fpregs_ok) return fail("fp fpregs");
        if (!fp_mxcsr_ok) return fail("fp mxcsr");
        /* The handler wrote 0x22 across XMM0 into the FXSAVE area: the
           interrupted context resumes with it. */
        unsigned char out[16] __attribute__((aligned(16))) = {0};
        __asm__ volatile("movups %%xmm0, %0" : "=m"(out) :: "memory");
        for (int i = 0; i < 16; i++) if (out[i] != 0x22) return fail("fp roundtrip");
        printf("DIFF_FP_OK\n");
        memset(&sa, 0, sizeof sa);
        sa.sa_handler = SIG_DFL;
        sigemptyset(&sa.sa_mask);
        sigaction(SIGUSR2, &sa, 0);
    }

    /* copy_file_range: explicit and current offsets, flags and type errors.
       PID-unique paths so parallel runs never collide on stale files. */
    {
        char cfr_src[64], cfr_dst[64];
        snprintf(cfr_src, sizeof cfr_src, "/tmp/cfr-src-%d", (int)getpid());
        snprintf(cfr_dst, sizeof cfr_dst, "/tmp/cfr-dst-%d", (int)getpid());
        int f = open(cfr_src, O_RDWR | O_CREAT | O_TRUNC, 0644);
        int g = open(cfr_dst, O_RDWR | O_CREAT | O_TRUNC, 0644);
        if (f < 0 || g < 0) return fail("cfr open");
        if (write(f, "0123456789abcdef", 16) != 16) return fail("cfr seed");
        loff_t oi = 4, oo = 0;
        if (copy_file_range(f, &oi, g, &oo, 8, 0) != 8) return fail("cfr basic");
        if (oi != 12 || oo != 8) return fail("cfr offsets");
        char buf[16] = {0};
        lseek(g, 0, SEEK_SET);
        if (read(g, buf, 8) != 8 || memcmp(buf, "456789ab", 8) != 0) return fail("cfr content");
        lseek(f, 0, SEEK_SET); lseek(g, 0, SEEK_SET);
        if (copy_file_range(f, NULL, g, NULL, 4, 0) != 4) return fail("cfr nulloff");
        if (lseek(f, 0, SEEK_CUR) != 4 || lseek(g, 0, SEEK_CUR) != 4) return fail("cfr cur off");
        if (copy_file_range(f, NULL, g, NULL, 4, 1) != -1 || errno != EINVAL) return fail("cfr flags");
        if (copy_file_range(f, NULL, g, NULL, 0, 0) != 0) return fail("cfr len0");
        int pp[2];
        if (pipe(pp) != 0) return fail("cfr pipe");
        if (write(pp[1], "ZZZZ", 4) != 4) return fail("cfr pipe seed");
        if (copy_file_range(pp[0], NULL, g, NULL, 4, 0) != -1 || errno != EINVAL) return fail("cfr from pipe");
        if (copy_file_range(f, NULL, pp[1], NULL, 4, 0) != -1 || errno != EINVAL) return fail("cfr onto pipe");
        int dd = open("/tmp", O_RDONLY);
        if (dd < 0) return fail("cfr dir open");
        if (copy_file_range(dd, NULL, g, NULL, 4, 0) != -1 || errno != EISDIR) return fail("cfr from dir");
        close(dd); close(pp[0]); close(pp[1]); close(f); close(g);
        printf("DIFF_CFR_OK\n");
    }

    /* splice: file<->pipe moves, pipe offsets, type and flag errors.
       PID-unique paths so parallel runs never collide on stale files. */
    {
        char spl_src[64], spl_dst[64];
        snprintf(spl_src, sizeof spl_src, "/tmp/spl-src-%d", (int)getpid());
        snprintf(spl_dst, sizeof spl_dst, "/tmp/spl-dst-%d", (int)getpid());
        int f = open(spl_src, O_RDWR | O_CREAT | O_TRUNC, 0644);
        int g = open(spl_dst, O_RDWR | O_CREAT | O_TRUNC, 0644);
        if (f < 0 || g < 0) return fail("splice open");
        if (write(f, "0123456789abcdef", 16) != 16) return fail("splice seed");
        int sp[2];
        if (pipe(sp) != 0) return fail("splice pipe");
        lseek(f, 0, SEEK_SET);
        if (splice(f, NULL, sp[1], NULL, 6, 0) != 6) return fail("splice file pipe");
        if (lseek(f, 0, SEEK_CUR) != 6) return fail("splice offset");
        char buf[8] = {0};
        if (read(sp[0], buf, 6) != 6 || memcmp(buf, "012345", 6) != 0) return fail("splice content");
        loff_t z = 0;
        if (splice(f, NULL, sp[1], &z, 6, 0) != -1 || errno != ESPIPE) return fail("splice pipe off");
        if (splice(f, NULL, g, NULL, 6, 0) != -1 || errno != EINVAL) return fail("splice file file");
        if (splice(f, NULL, sp[1], NULL, 6, 0x100) != -1 || errno != EINVAL) return fail("splice flags");
        if (splice(f, NULL, sp[1], NULL, 0, 0) != 0) return fail("splice len0");
        /* pipe -> file with an explicit file offset. */
        if (write(sp[1], "PIPEPIPE", 8) != 8) return fail("splice refill");
        loff_t at = 16;
        if (splice(sp[0], NULL, g, &at, 8, 0) != 8) return fail("splice pipe file");
        if (at != 24) return fail("splice file off update");
        lseek(g, 16, SEEK_SET);
        memset(buf, 0, sizeof buf);
        if (read(g, buf, 8) != 8 || memcmp(buf, "PIPEPIPE", 8) != 0) return fail("splice pipe file content");
        close(sp[0]); close(sp[1]); close(f); close(g);
        printf("DIFF_SPLICE_OK\n");
    }

    /* tee: duplicates pipe content without consuming it. */
    {
        int ta[2], tb[2];
        if (pipe(ta) != 0 || pipe(tb) != 0) return fail("tee pipe");
        if (write(ta[1], "HelloW", 6) != 6) return fail("tee seed");
        if (tee(ta[0], tb[1], 6, 0) != 6) return fail("tee basic");
        char a1[8] = {0}, b1[8] = {0};
        if (read(ta[0], a1, 6) != 6 || memcmp(a1, "HelloW", 6) != 0) return fail("tee kept");
        if (read(tb[0], b1, 6) != 6 || memcmp(b1, "HelloW", 6) != 0) return fail("tee copy");
        char tee_path[64];
        snprintf(tee_path, sizeof tee_path, "/tmp/tee-src-%d", (int)getpid());
        int f = open(tee_path, O_RDWR | O_CREAT | O_TRUNC, 0644);
        if (f < 0) return fail("tee open");
        if (write(f, "data", 4) != 4) return fail("tee file seed");
        if (tee(f, tb[1], 4, 0) != -1 || errno != EINVAL) return fail("tee file in");
        if (tee(ta[0], f, 4, 0) != -1 || errno != EINVAL) return fail("tee file out");
        if (tee(ta[0], tb[1], 0, 0) != 0) return fail("tee len0");
        if (write(ta[1], "AB", 2) != 2) return fail("tee refill");
        /* Flags are ignored by Linux. */
        if (tee(ta[0], tb[1], 2, 1) != 2) return fail("tee flags");
        close(ta[0]); close(ta[1]); close(tb[0]); close(tb[1]); close(f);
        printf("DIFF_TEE_OK\n");
    }

    /* fallocate: extend with zeros, KEEP_SIZE, PUNCH_HOLE, bad modes. */
    {
        char falloc_path[64];
        snprintf(falloc_path, sizeof falloc_path, "/tmp/falloc-%d", (int)getpid());
        int h = open(falloc_path, O_RDWR | O_CREAT | O_TRUNC, 0644);
        if (h < 0) return fail("falloc open");
        if (fallocate(h, 0, 0, 8192) != 0) return fail("falloc extend");
        struct stat st;
        if (fstat(h, &st) != 0 || st.st_size != 8192) return fail("falloc size");
        char z8[8] = {1,1,1,1,1,1,1,1};
        if (pread(h, z8, 8, 4096) != 8) return fail("falloc read");
        for (int i = 0; i < 8; i++) if (z8[i] != 0) return fail("falloc zero");
        if (write(h, "0123456789abcdef", 16) != 16) return fail("falloc seed");
        if (fallocate(h, FALLOC_FL_KEEP_SIZE, 0, 16384) != 0) return fail("falloc keep");
        if (fstat(h, &st) != 0 || st.st_size != 8192) return fail("falloc keep size");
        if (fallocate(h, FALLOC_FL_KEEP_SIZE | FALLOC_FL_PUNCH_HOLE, 4, 4) != 0) return fail("falloc punch");
        char p8[8] = {0};
        lseek(h, 0, SEEK_SET);
        if (read(h, p8, 8) != 8 || memcmp(p8, "0123\0\0\0\0", 8) != 0) return fail("falloc punched");
        if (fallocate(h, 0x100, 0, 100) != -1 || errno != EOPNOTSUPP) return fail("falloc mode");
        if (fallocate(h, 0, 0, 0) != -1 || errno != EINVAL) return fail("falloc len0");
        close(h);
        printf("DIFF_FALLOC_OK\n");
    }

    /* TCP passive open over loopback: bind conflict, refused connect,
       listener peername, accept with peer address, echo, orderly EOF. */
    {
        int ls = socket(AF_INET, SOCK_STREAM, 0);
        if (ls < 0) return fail("tcpsrv socket");
        struct sockaddr_in a;
        memset(&a, 0, sizeof a);
        a.sin_family = AF_INET;
        a.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        a.sin_port = htons(45311);
        if (bind(ls, (struct sockaddr *)&a, sizeof a) != 0) return fail("tcpsrv bind");
        int dup = socket(AF_INET, SOCK_STREAM, 0);
        if (dup < 0) return fail("tcpsrv dup socket");
        if (bind(dup, (struct sockaddr *)&a, sizeof a) != -1 || errno != EADDRINUSE) return fail("tcpsrv dupbind");
        close(dup);
        if (listen(ls, 4) != 0) return fail("tcpsrv listen");
        int bad = socket(AF_INET, SOCK_STREAM, 0);
        struct sockaddr_in z;
        memset(&z, 0, sizeof z);
        z.sin_family = AF_INET;
        z.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        z.sin_port = htons(45312);
        if (connect(bad, (struct sockaddr *)&z, sizeof z) != -1 || errno != ECONNREFUSED) return fail("tcpsrv refused");
        close(bad);
        struct sockaddr_in p;
        socklen_t pl = sizeof p;
        if (getpeername(ls, (struct sockaddr *)&p, &pl) != -1 || errno != ENOTCONN) return fail("tcpsrv peername");
        pid_t c = fork();
        if (c < 0) return fail("tcpsrv fork");
        if (c == 0) {
            int s = socket(AF_INET, SOCK_STREAM, 0);
            if (s < 0) _exit(10);
            struct sockaddr_in sa;
            memset(&sa, 0, sizeof sa);
            sa.sin_family = AF_INET;
            sa.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
            sa.sin_port = htons(45311);
            if (connect(s, (struct sockaddr *)&sa, sizeof sa) != 0) _exit(11);
            struct sockaddr_in pp;
            socklen_t ppl = sizeof pp;
            if (getpeername(s, (struct sockaddr *)&pp, &ppl) != 0) _exit(12);
            if (ntohs(pp.sin_port) != 45311) _exit(13);
            if (pp.sin_addr.s_addr != htonl(INADDR_LOOPBACK)) _exit(14);
            if (send(s, "HelloSrv", 8, 0) != 8) _exit(15);
            char rb[8] = {0};
            int got = 0;
            while (got < 8) { int n = recv(s, rb + got, 8 - got, 0); if (n <= 0) _exit(16); got += n; }
            if (memcmp(rb, "HiClient", 8) != 0) _exit(17);
            close(s);
            _exit(0);
        }
        struct sockaddr_in peer;
        socklen_t peerlen = sizeof peer;
        int conn = accept(ls, (struct sockaddr *)&peer, &peerlen);
        if (conn < 0) return fail("tcpsrv accept");
        if (peer.sin_addr.s_addr != htonl(INADDR_LOOPBACK)) return fail("tcpsrv peer ip");
        if (ntohs(peer.sin_port) == 0) return fail("tcpsrv peer port");
        char rb[8] = {0};
        int got = 0;
        while (got < 8) { int n = recv(conn, rb + got, 8 - got, 0); if (n <= 0) return fail("tcpsrv recv"); got += n; }
        if (memcmp(rb, "HelloSrv", 8) != 0) return fail("tcpsrv content");
        if (send(conn, "HiClient", 8, 0) != 8) return fail("tcpsrv send");
        int st;
        if (waitpid(c, &st, 0) != c) return fail("tcpsrv wait");
        if (!WIFEXITED(st) || WEXITSTATUS(st) != 0) return fail("tcpsrv child");
        char eofb[4];
        if (recv(conn, eofb, sizeof eofb, 0) != 0) return fail("tcpsrv eof");
        close(conn);
        close(ls);
        printf("DIFF_TCPSRV_OK\n");
    }

    /* UDP loopback: sendto 127.0.0.1 is delivered locally with the address. */
    {
        int s = socket(AF_INET, SOCK_DGRAM, 0);
        int t = socket(AF_INET, SOCK_DGRAM, 0);
        if (s < 0 || t < 0) return fail("udploop socket");
        struct sockaddr_in a;
        memset(&a, 0, sizeof a);
        a.sin_family = AF_INET;
        a.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        a.sin_port = htons(45321);
        if (bind(s, (struct sockaddr *)&a, sizeof a) != 0) return fail("udploop bind");
        if (sendto(t, "ping", 4, 0, (struct sockaddr *)&a, sizeof a) != 4) return fail("udploop sendto");
        char buf[8] = {0};
        struct sockaddr_in from;
        socklen_t fl = sizeof from;
        memset(&from, 0, sizeof from);
        if (recvfrom(s, buf, sizeof buf, 0, (struct sockaddr *)&from, &fl) != 4) return fail("udploop recv");
        if (memcmp(buf, "ping", 4) != 0) return fail("udploop content");
        if (from.sin_addr.s_addr != htonl(INADDR_LOOPBACK)) return fail("udploop src ip");
        /* Connected datagrams report the peer too. */
        if (connect(t, (struct sockaddr *)&a, sizeof a) != 0) return fail("udploop connect");
        struct sockaddr_in pp;
        socklen_t ppl = sizeof pp;
        if (getpeername(t, (struct sockaddr *)&pp, &ppl) != 0) return fail("udploop peername");
        if (pp.sin_addr.s_addr != htonl(INADDR_LOOPBACK) || ntohs(pp.sin_port) != 45321) return fail("udploop peer");
        if (send(t, "pong", 4, 0) != 4) return fail("udploop send");
        memset(buf, 0, sizeof buf);
        fl = sizeof from;
        memset(&from, 0, sizeof from);
        if (recvfrom(s, buf, sizeof buf, 0, (struct sockaddr *)&from, &fl) != 4) return fail("udploop recv2");
        if (memcmp(buf, "pong", 4) != 0) return fail("udploop content2");
        close(s);
        close(t);
        printf("DIFF_UDPLOOP_OK\n");
    }

    /* signalfd observes stop/continue/exit as SIGCHLD with the Linux codes:
       CLD_STOPPED=5, CLD_CONTINUED=6, CLD_EXITED=1, CLD_KILLED=2. */
    {
        sigset_t chmask, chold;
        sigemptyset(&chmask);
        sigaddset(&chmask, SIGCHLD);
        if (sigprocmask(SIG_BLOCK, &chmask, &chold) != 0) return fail("sfd block");
        int sfd = signalfd(-1, &chmask, 0);
        if (sfd < 0) return fail("sfd create");
        /* Read semantics on an empty blocking set is covered by poll
           timeouts below; check the nonblocking and short-read errors. */
        {
            int nfsd = signalfd(-1, &chmask, SFD_NONBLOCK);
            if (nfsd < 0) return fail("sfd nonblock create");
            struct signalfd_siginfo tmp;
            ssize_t nn = read(nfsd, &tmp, sizeof tmp);
            if (nn != -1 || errno != EAGAIN) return fail("sfd nonblock empty");
            nn = read(nfsd, &tmp, 10);
            if (nn != -1 || errno != EINVAL) return fail("sfd short read");
            close(nfsd);
        }
        pid_t c = fork();
        if (c < 0) return fail("sfd stop fork");
        if (c == 0) {
            raise(SIGSTOP);
            sleep(1);
            _exit(43);
        }
        struct pollfd pfd = {sfd, POLLIN, 0};
        if (poll(&pfd, 1, 5000) <= 0) return fail("sfd stop poll");
        struct signalfd_siginfo si;
        if (read(sfd, &si, sizeof si) != sizeof si) return fail("sfd stop read");
        if (si.ssi_signo != SIGCHLD) return fail("sfd stop signo");
        if (si.ssi_code != 5) return fail("sfd stop code");
        if (si.ssi_pid != (uint32_t)c) return fail("sfd stop pid");
        if (si.ssi_status != SIGSTOP) return fail("sfd stop status");
        int st;
        if (waitpid(c, &st, WUNTRACED) != c) return fail("sfd stop wait");
        if (!WIFSTOPPED(st) || WSTOPSIG(st) != SIGSTOP) return fail("sfd stop wait status");
        if (kill(c, SIGCONT) != 0) return fail("sfd cont send");
        pfd.revents = 0;
        if (poll(&pfd, 1, 5000) <= 0) return fail("sfd cont poll");
        if (read(sfd, &si, sizeof si) != sizeof si) return fail("sfd cont read");
        if (si.ssi_signo != SIGCHLD) return fail("sfd cont signo");
        if (si.ssi_code != 6) return fail("sfd cont code");
        if (si.ssi_pid != (uint32_t)c) return fail("sfd cont pid");
        if (si.ssi_status != SIGCONT) return fail("sfd cont status");
        if (waitpid(c, &st, WCONTINUED) != c) return fail("sfd cont wait");
        if (!WIFCONTINUED(st)) return fail("sfd cont wait status");
        if (waitpid(c, &st, 0) != c) return fail("sfd exit wait");
        if (!WIFEXITED(st) || WEXITSTATUS(st) != 43) return fail("sfd exit wait status");
        pfd.revents = 0;
        if (poll(&pfd, 1, 5000) <= 0) return fail("sfd exit poll");
        if (read(sfd, &si, sizeof si) != sizeof si) return fail("sfd exit read");
        if (si.ssi_signo != SIGCHLD) return fail("sfd exit signo");
        if (si.ssi_code != 1) return fail("sfd exit code");
        if (si.ssi_pid != (uint32_t)c) return fail("sfd exit pid");
        if (si.ssi_status != 43) return fail("sfd exit status");
        close(sfd);
        sigprocmask(SIG_SETMASK, &chold, NULL);
        printf("DIFF_SIGNALFD_STOP_OK\n");
    }

    /* SA_NOCLDSTOP suppresses the stop/continue SIGCHLD on signalfd but
       waitpid still observes the states; the exit event still arrives.
       Suppression is checked with a nonblocking read after the state is
       confirmed via waitpid, so a later exit cannot fall into the window
       and look like a leak; every wait is bounded. */
    {
        struct sigaction sa, oldsa;
        memset(&sa, 0, sizeof sa);
        sa.sa_handler = SIG_DFL;
        sa.sa_flags = SA_NOCLDSTOP;
        sigemptyset(&sa.sa_mask);
        if (sigaction(SIGCHLD, &sa, &oldsa) != 0) return fail("nocld sigaction");
        sigset_t chmask, chold;
        sigemptyset(&chmask);
        sigaddset(&chmask, SIGCHLD);
        if (sigprocmask(SIG_BLOCK, &chmask, &chold) != 0) return fail("nocld block");
        int sfd = signalfd(-1, &chmask, 0);
        if (sfd < 0) return fail("nocld sfd create");
        int nbfd = signalfd(-1, &chmask, SFD_NONBLOCK);
        if (nbfd < 0) return fail("nocld nb create");
        pid_t c = fork();
        if (c < 0) return fail("nocld fork");
        if (c == 0) {
            raise(SIGSTOP);
            sleep(2);
            _exit(7);
        }
        /* Bounded wait for the stop; the exit cannot arrive first. */
        int st = 0, waited = 0;
        for (int i = 0; i < 100; i++) {
            pid_t r = waitpid(c, &st, WUNTRACED | WNOHANG);
            if (r == c) { waited = 1; break; }
            if (r != 0) return fail("nocld stop wait err");
            usleep(50000);
        }
        if (!waited) return fail("nocld stop wait timeout");
        if (!WIFSTOPPED(st) || WSTOPSIG(st) != SIGSTOP) return fail("nocld stop status");
        /* No stop SIGCHLD may be queued: nonblocking read must be empty. */
        {
            struct signalfd_siginfo dbg;
            ssize_t dbg_n = read(nbfd, &dbg, sizeof dbg);
            if (dbg_n != -1 || errno != EAGAIN) return fail("nocld stop signaled");
        }
        if (kill(c, SIGCONT) != 0) return fail("nocld cont send");
        st = 0; waited = 0;
        for (int i = 0; i < 100; i++) {
            pid_t r = waitpid(c, &st, WCONTINUED | WNOHANG);
            if (r == c) { waited = 1; break; }
            if (r != 0) return fail("nocld cont wait err");
            usleep(50000);
        }
        if (!waited) return fail("nocld cont wait timeout");
        if (!WIFCONTINUED(st)) return fail("nocld cont status");
        {
            struct signalfd_siginfo dbg;
            ssize_t dbg_n = read(nbfd, &dbg, sizeof dbg);
            if (dbg_n != -1 || errno != EAGAIN) {
                if (dbg_n == (ssize_t)sizeof dbg)
                    printf("nocld cont unexpected: code=%u pid=%u status=%d\n",
                        dbg.ssi_code, dbg.ssi_pid, dbg.ssi_status);
                return fail("nocld cont signaled");
            }
        }
        st = 0; waited = 0;
        for (int i = 0; i < 100; i++) {
            pid_t r = waitpid(c, &st, WNOHANG);
            if (r == c) { waited = 1; break; }
            if (r != 0) return fail("nocld exit wait err");
            usleep(50000);
        }
        if (!waited) return fail("nocld exit wait timeout");
        if (!WIFEXITED(st) || WEXITSTATUS(st) != 7) return fail("nocld exit status");
        struct pollfd pfd = {sfd, POLLIN, 0};
        if (poll(&pfd, 1, 5000) <= 0) return fail("nocld exit poll");
        struct signalfd_siginfo si;
        if (read(sfd, &si, sizeof si) != sizeof si) return fail("nocld exit read");
        if (si.ssi_signo != SIGCHLD) return fail("nocld exit signo");
        if (si.ssi_code != 1) return fail("nocld exit code");
        if (si.ssi_pid != (uint32_t)c) return fail("nocld exit pid");
        if (si.ssi_status != 7) return fail("nocld exit status value");
        close(nbfd);
        close(sfd);
        sigprocmask(SIG_SETMASK, &chold, NULL);
        sigaction(SIGCHLD, &oldsa, NULL);
        printf("DIFF_SIGNALFD_NOCLDSTOP_OK\n");
    }

    /* AF_UNIX datagrams: bind/sendto/recvfrom with source addresses,
       connected sends, unbound senders, abstract names and socketpair. */
    {
        char pa[108], pb[108];
        snprintf(pa, sizeof pa, "/tmp/slop-dg-a-%d", (int)getpid());
        snprintf(pb, sizeof pb, "/tmp/slop-dg-b-%d", (int)getpid());
        (void)unlink(pa); (void)unlink(pb);
        int a = socket(AF_UNIX, SOCK_DGRAM, 0);
        int b = socket(AF_UNIX, SOCK_DGRAM, 0);
        if (a < 0 || b < 0) return fail("unixdg socket");
        struct sockaddr_un au, bu;
        memset(&au, 0, sizeof au); memset(&bu, 0, sizeof bu);
        au.sun_family = AF_UNIX; bu.sun_family = AF_UNIX;
        snprintf(au.sun_path, sizeof au.sun_path, "%s", pa);
        snprintf(bu.sun_path, sizeof bu.sun_path, "%s", pb);
        if (bind(a, (struct sockaddr *)&au, sizeof au) != 0) return fail("unixdg bind a");
        if (bind(b, (struct sockaddr *)&bu, sizeof bu) != 0) return fail("unixdg bind b");
        /* Duplicate bind must fail with EADDRINUSE. */
        {
            int d = socket(AF_UNIX, SOCK_DGRAM, 0);
            if (d < 0) return fail("unixdg dup socket");
            if (bind(d, (struct sockaddr *)&au, sizeof au) != -1 || errno != EADDRINUSE) return fail("unixdg dupbind");
            close(d);
        }
        /* getsockname reports the bound path. */
        {
            struct sockaddr_un gn; socklen_t gl = sizeof gn;
            memset(&gn, 0, sizeof gn);
            if (getsockname(b, (struct sockaddr *)&gn, &gl) != 0) return fail("unixdg getsockname");
            if (gn.sun_family != AF_UNIX) return fail("unixdg getsockname family");
            if (strcmp(gn.sun_path, pb) != 0) return fail("unixdg getsockname path");
        }
        /* Unconnected send must fail with ENOTCONN. */
        if (send(a, "x", 1, 0) != -1 || errno != ENOTCONN) return fail("unixdg unconn send");
        /* sendto with an explicit destination, recvfrom reports the source. */
        if (sendto(a, "hello", 5, 0, (struct sockaddr *)&bu, sizeof bu) != 5) return fail("unixdg sendto");
        {
            char rbuf[32] = {0};
            struct sockaddr_un from; socklen_t fl = sizeof from;
            memset(&from, 0, sizeof from);
            if (recvfrom(b, rbuf, sizeof rbuf, 0, (struct sockaddr *)&from, &fl) != 5) return fail("unixdg recvfrom");
            if (memcmp(rbuf, "hello", 5) != 0) return fail("unixdg content");
            if (from.sun_family != AF_UNIX) return fail("unixdg from family");
            if (strcmp(from.sun_path, pa) != 0) return fail("unixdg from path");
        }
        /* Connected datagrams use send/recv and report the peer. */
        if (connect(a, (struct sockaddr *)&bu, sizeof bu) != 0) return fail("unixdg connect");
        {
            struct sockaddr_un pp; socklen_t ppl = sizeof pp;
            memset(&pp, 0, sizeof pp);
            if (getpeername(a, (struct sockaddr *)&pp, &ppl) != 0) return fail("unixdg peername");
            if (strcmp(pp.sun_path, pb) != 0) return fail("unixdg peer path");
        }
        if (send(a, "world", 5, 0) != 5) return fail("unixdg send");
        {
            char rbuf[32] = {0};
            if (recv(b, rbuf, sizeof rbuf, 0) != 5) return fail("unixdg recv");
            if (memcmp(rbuf, "world", 5) != 0) return fail("unixdg recv content");
        }
        /* Message boundaries are preserved: two sends arrive as two reads. */
        if (send(a, "AB", 2, 0) != 2) return fail("unixdg msg1");
        if (send(a, "CDEF", 4, 0) != 4) return fail("unixdg msg2");
        {
            char r1[16] = {0}, r2[16] = {0};
            if (recv(b, r1, sizeof r1, 0) != 2) return fail("unixdg boundary1");
            if (memcmp(r1, "AB", 2) != 0) return fail("unixdg boundary1 content");
            if (recv(b, r2, sizeof r2, 0) != 4) return fail("unixdg boundary2");
            if (memcmp(r2, "CDEF", 4) != 0) return fail("unixdg boundary2 content");
        }
        /* Unbound sender: source is the empty address. */
        {
            int c = socket(AF_UNIX, SOCK_DGRAM, 0);
            if (c < 0) return fail("unixdg unbound socket");
            if (sendto(c, "anon", 4, 0, (struct sockaddr *)&bu, sizeof bu) != 4) return fail("unixdg anon sendto");
            char rbuf[16] = {0};
            struct sockaddr_un from; socklen_t fl = sizeof from;
            memset(&from, 0, sizeof from);
            if (recvfrom(b, rbuf, sizeof rbuf, 0, (struct sockaddr *)&from, &fl) != 4) return fail("unixdg anon recv");
            if (memcmp(rbuf, "anon", 4) != 0) return fail("unixdg anon content");
            if (from.sun_path[0] != '\0') return fail("unixdg anon source");
            close(c);
        }
        /* Unknown destination reports ENOENT. */
        {
            struct sockaddr_un nz;
            memset(&nz, 0, sizeof nz);
            nz.sun_family = AF_UNIX;
            snprintf(nz.sun_path, sizeof nz.sun_path, "/tmp/slop-dg-nope-%d", (int)getpid());
            if (sendto(a, "x", 1, 0, (struct sockaddr *)&nz, sizeof nz) != -1 || errno != ENOENT) return fail("unixdg nope");
        }
        /* Oversize datagrams report EMSGSIZE on both ends. Linux allows
           ~212 KiB; the guest caps at 4 KiB, so probe above the Linux
           limit so both ends agree. */
        {
            size_t huge = 300 * 1024;
            char *bigm = malloc(huge);
            if (!bigm) return fail("unixdg huge alloc");
            memset(bigm, 'q', huge);
            int rc = sendto(a, bigm, huge, 0, (struct sockaddr *)&bu, sizeof bu);
            free(bigm);
            if (rc != -1 || errno != EMSGSIZE) return fail("unixdg msgsize");
        }
        /* Abstract names (leading NUL) rendezvous without the filesystem. */
        {
            int x = socket(AF_UNIX, SOCK_DGRAM, 0);
            int y = socket(AF_UNIX, SOCK_DGRAM, 0);
            if (x < 0 || y < 0) return fail("unixdg abs socket");
            struct sockaddr_un xa, ya;
            memset(&xa, 0, sizeof xa); memset(&ya, 0, sizeof ya);
            xa.sun_family = AF_UNIX; ya.sun_family = AF_UNIX;
            char aname[32];
            snprintf(aname, sizeof aname, "slopdg-%d", (int)getpid());
            size_t anl = strlen(aname);
            xa.sun_path[0] = '\0';
            memcpy(&xa.sun_path[1], aname, anl);
            socklen_t xlen = (socklen_t)(2 + 1 + anl);
            if (bind(x, (struct sockaddr *)&xa, xlen) != 0) return fail("unixdg abs bind");
            if (sendto(y, "abs", 3, 0, (struct sockaddr *)&xa, xlen) != 3) return fail("unixdg abs sendto");
            char rbuf[16] = {0};
            struct sockaddr_un from; socklen_t fl = sizeof from;
            memset(&from, 0, sizeof from);
            if (recvfrom(x, rbuf, sizeof rbuf, 0, (struct sockaddr *)&from, &fl) != 3) return fail("unixdg abs recv");
            if (memcmp(rbuf, "abs", 3) != 0) return fail("unixdg abs content");
            close(x); close(y);
        }
        /* DGRAM socketpair behaves as a connected pair. */
        {
            int sv[2] = {-1, -1};
            if (socketpair(AF_UNIX, SOCK_DGRAM, 0, sv) != 0) return fail("unixdg pair");
            if (send(sv[0], "pq", 2, 0) != 2) return fail("unixdg pair send");
            char rbuf[8] = {0};
            if (recv(sv[1], rbuf, sizeof rbuf, 0) != 2) return fail("unixdg pair recv");
            if (memcmp(rbuf, "pq", 2) != 0) return fail("unixdg pair content");
            if (send(sv[1], "qp", 2, 0) != 2) return fail("unixdg pair send2");
            memset(rbuf, 0, sizeof rbuf);
            if (recv(sv[0], rbuf, sizeof rbuf, 0) != 2) return fail("unixdg pair recv2");
            if (memcmp(rbuf, "qp", 2) != 0) return fail("unixdg pair content2");
            close(sv[0]); close(sv[1]);
        }
        close(a); close(b);
        (void)unlink(pa); (void)unlink(pb);
        printf("DIFF_UNIXDG_OK\n");
    }

    /* dirfd-relative *at: openat/mkdirat/unlinkat/newfstatat/statx/
       faccessat/fchmodat/fchownat/utimensat/execveat and, where the
       filesystem allows, linkat/symlinkat/readlinkat/renameat. ext2
       stays read-only for link/symlink/rename, so those run on the
       root/tmpfs side, like real software hitting different mounts. */
    /* Probe the guest image flavor once: the ext4 seed tree lives at
       /mnt/xdiag4, which never exists on the ext2 image. The read-only
       ext4 mount cannot host scratch dirs, so those move to tmpfs. */
    int guest_ext4 = 0;
    if (getenv("SLOP_MNU_DIFF")) {
        int pf = open("/mnt/xdiag4/hello.txt", O_RDONLY);
        if (pf >= 0) { guest_ext4 = 1; close(pf); }
    }
    {
        char base[128], sub[128], f1[128], g1[128];
        char base2[128], lnk[128], hlnk[128], h2[128];
        const char *envbase = getenv("SLOP_MNU_DIFF");
        if (envbase && !guest_ext4) snprintf(base, sizeof base, "/mnt/atdiag");
        else if (envbase) snprintf(base, sizeof base, "/atdiag4");
        else snprintf(base, sizeof base, "/tmp/atdiag-%d", (int)getpid());
        if (envbase) snprintf(base2, sizeof base2, "/atfs2");
        else snprintf(base2, sizeof base2, "/tmp/atfs-%d", (int)getpid());
        snprintf(sub, sizeof sub, "%s/sub", base);
        snprintf(f1, sizeof f1, "%s/f.txt", base);
        snprintf(g1, sizeof g1, "%s/g.txt", base);
        snprintf(lnk, sizeof lnk, "%s/lnk", base2);
        snprintf(hlnk, sizeof hlnk, "%s/hlnk", base2);
        snprintf(h2, sizeof h2, "%s/hlnk2", base2);
        (void)unlink(f1); (void)unlink(g1);
        (void)rmdir(sub); (void)rmdir(base);
        (void)unlink(lnk); (void)unlink(hlnk); (void)unlink(h2);
        (void)rmdir(base2);
        if (mkdir(base, 0755) != 0) return fail("at mkdir base");
        if (mkdir(base2, 0755) != 0) return fail("at mkdir base2");
        int dfd = open(base, O_RDONLY | O_DIRECTORY);
        if (dfd < 0) return fail("at open dirfd");
        int fw = openat(dfd, "f.txt", O_WRONLY | O_CREAT | O_TRUNC, 0644);
        if (fw < 0) return fail("at openat create");
        if (write(fw, "at-data!", 8) != 8) return fail("at write");
        close(fw);
        int fr = openat(dfd, "f.txt", O_RDONLY);
        if (fr < 0) return fail("at openat read");
        char rb[8] = {0};
        if (read(fr, rb, sizeof rb) != 8 || memcmp(rb, "at-data!", 8) != 0) return fail("at content");
        close(fr);
        int fa = openat(dfd, f1, O_RDONLY);
        if (fa < 0) return fail("at openat absolute");
        close(fa);
        if (openat(-5, "x", O_RDONLY) != -1 || errno != EBADF) return fail("at badfd");
        int reg = openat(dfd, "f.txt", O_RDONLY);
        if (reg < 0) return fail("at open reg");
        if (openat(reg, "x", O_RDONLY) != -1 || errno != ENOTDIR) return fail("at regfd");
        close(reg);
        if (mkdirat(dfd, "sub", 0700) != 0) return fail("at mkdirat");
        if (mkdirat(dfd, "sub", 0700) != -1 || errno != EEXIST) return fail("at mkdirat eexist");
        /* Device-node creation stays read-only on the disk mounts, so the
           positive mknodat coverage runs on the tmpfs side (base2). */
        int dfd2 = open(base2, O_RDONLY | O_DIRECTORY);
        if (dfd2 < 0) return fail("at open base2 dirfd");
        if (mknodat(dfd2, "fifo1", S_IFIFO | 0600, 0) != 0) return fail("at mknodat tmpfs fifo");
        {
            struct stat fst;
            if (fstatat(dfd2, "fifo1", &fst, 0) != 0 || !S_ISFIFO(fst.st_mode)) return fail("at mknodat fifo stat");
        }
        if (mknodat(dfd2, "fifo1", S_IFIFO | 0600, 0) != -1 || errno != EEXIST) return fail("at mknodat eexist");
        if (mknodat(dfd2, "plain", S_IFREG | 0644, 0) != 0) return fail("at mknodat reg");
        {
            struct stat pst;
            if (fstatat(dfd2, "plain", &pst, 0) != 0 || !S_ISREG(pst.st_mode) || pst.st_size != 0) return fail("at mknodat reg stat");
        }
        if (mknodat(-5, "x", S_IFIFO | 0600, 0) != -1 || errno != EBADF) return fail("at mknodat badfd");
        if (unlinkat(dfd2, "fifo1", 0) != 0) return fail("at mknodat fifo unlink");
        if (unlinkat(dfd2, "plain", 0) != 0) return fail("at mknodat reg unlink");
        close(dfd2);
        int sfd = openat(dfd, "sub", O_RDONLY | O_DIRECTORY);
        if (sfd < 0) return fail("at open subdir");
        int fdot = openat(sfd, "../f.txt", O_RDONLY);
        if (fdot < 0) return fail("at dotdot");
        close(fdot);
        close(sfd);
        {
            struct stat st;
            if (fstatat(dfd, "f.txt", &st, 0) != 0 || st.st_size != 8) return fail("at fstatat");
            struct slop_statx sx;
            memset(&sx, 0, sizeof sx);
            if (syscall(SYS_statx, dfd, "f.txt", 0, 0xFFFu, &sx) != 0) return fail("at statx");
            if (sx.stx_size != 8) return fail("at statx size");
            int ofd = openat(dfd, "f.txt", O_RDONLY);
            if (ofd < 0) return fail("at open for empty");
            memset(&sx, 0, sizeof sx);
            if (syscall(SYS_statx, ofd, "", AT_EMPTY_PATH, 0xFFFu, &sx) != 0) return fail("at statx empty");
            if (sx.stx_size != 8) return fail("at statx empty size");
            close(ofd);
            struct stat es;
            if (fstatat(dfd, "f.txt", &es, AT_EMPTY_PATH) != 0) return fail("at fstatat nonempty empty-flag");
            int efd = openat(dfd, "f.txt", O_RDONLY);
            struct stat fs;
            if (fstatat(efd, "", &fs, AT_EMPTY_PATH) != 0) return fail("at fstatat empty");
            if (fs.st_size != 8) return fail("at fstatat empty size");
            struct stat cs;
            if (fstatat(AT_FDCWD, "", &cs, AT_EMPTY_PATH) != 0) return fail("at fstatat cwd");
            close(efd);
        }
        if (faccessat(dfd, "f.txt", R_OK, 0) != 0) return fail("at faccessat");
        if (faccessat(dfd, "nope", R_OK, 0) != -1 || errno != ENOENT) return fail("at faccessat missing");
        if (fchmodat(dfd, "f.txt", 0600, 0) != 0) return fail("at fchmodat");
        struct stat cm;
        if (stat(f1, &cm) != 0 || (cm.st_mode & 0777) != 0600) return fail("at chmod mode");
        if (fchownat(dfd, "f.txt", (uid_t)-1, (gid_t)-1, 0) != 0) return fail("at fchownat");
        if (utimensat(dfd, "f.txt", NULL, 0) != 0) return fail("at utimensat");
        if (utimensat(dfd, "f.txt", NULL, AT_SYMLINK_NOFOLLOW) != 0) return fail("at utimensat nofollow");
        close(dfd);
        int d2 = open(base2, O_RDONLY | O_DIRECTORY);
        if (d2 < 0) return fail("at open dirfd2");
        int w2 = openat(d2, "f.txt", O_WRONLY | O_CREAT | O_TRUNC, 0644);
        if (w2 < 0) return fail("at openat2 create");
        if (write(w2, "at-data!", 8) != 8) return fail("at write2");
        close(w2);
        if (symlinkat("f.txt", d2, "lnk") != 0) return fail("at symlinkat");
        char lb[16] = {0};
        if (readlinkat(d2, "lnk", lb, sizeof lb) != 5 || memcmp(lb, "f.txt", 5) != 0) return fail("at readlinkat");
        struct stat lst;
        if (fstatat(d2, "lnk", &lst, AT_SYMLINK_NOFOLLOW) != 0) return fail("at lstat");
        if (!S_ISLNK(lst.st_mode)) return fail("at lstat type");
        struct stat rst;
        if (fstatat(d2, "lnk", &rst, 0) != 0) return fail("at stat follow");
        if (!S_ISREG(rst.st_mode)) return fail("at stat type");
        if (utimensat(d2, "lnk", NULL, AT_SYMLINK_NOFOLLOW) != 0) return fail("at utimensat lnk nofollow");
        if (linkat(d2, "f.txt", d2, "hlnk", 0) != 0) return fail("at linkat");
        if (linkat(d2, "f.txt", d2, "hlnk", 0) != -1 || errno != EEXIST) return fail("at linkat eexist");
        if (linkat(d2, "lnk", d2, "hlnk2", AT_SYMLINK_FOLLOW) != 0) return fail("at linkat follow");
        if (unlinkat(d2, "hlnk2", 0) != 0) return fail("at unlink hlnk2");
        if (renameat(d2, "f.txt", d2, "g.txt") != 0) return fail("at renameat");
        struct stat gs;
        char g2[128];
        snprintf(g2, sizeof g2, "%s/g.txt", base2);
        if (stat(g2, &gs) != 0) return fail("at rename visible");
        long rn2 = syscall(SYS_renameat2, d2, "g.txt", d2, "hlnk", 1);
        if (rn2 != -1 || errno != EEXIST) return fail("at renameat2 noreplace");
        if (unlinkat(d2, "hlnk", 0) != 0) return fail("at unlink hlnk");
        rn2 = syscall(SYS_renameat2, d2, "g.txt", d2, "h.txt", 0);
        if (rn2 != 0) return fail("at renameat2 plain");
        if (renameat(d2, "h.txt", d2, "g.txt") != 0) return fail("at rename back");
        {
            const char *bindir = envbase ? "/bin" : g_bindir;
            int bfd = open(bindir, O_RDONLY | O_DIRECTORY);
            if (bfd < 0) return fail("at open bindir");
            pid_t c = fork();
            if (c < 0) return fail("at execveat fork");
            if (c == 0) {
                char *args[] = { "nopie-helper", "fixed", 0 };
                char *env[] = { "SLOP_MNU_DIFF=1", 0 };
                long r = syscall(SYS_execveat, bfd, "nopie-helper", args, env, 0);
                _exit(71);
                (void)r;
            }
            int st;
            if (waitpid(c, &st, 0) != c) return fail("at execveat wait");
            if (!WIFEXITED(st) || WEXITSTATUS(st) != 17) return fail("at execveat status");
            close(bfd);
        }
        if (unlinkat(d2, "g.txt", 0) != 0) return fail("at unlinkat g");
        if (unlinkat(d2, "lnk", 0) != 0) return fail("at unlinkat lnk");
        dfd = open(base, O_RDONLY | O_DIRECTORY);
        if (dfd < 0) return fail("at reopen dirfd");
        if (unlinkat(dfd, "f.txt", 0) != 0) return fail("at unlinkat");
        if (unlinkat(dfd, "sub", AT_REMOVEDIR) != 0) return fail("at unlinkat rmdir");
        close(dfd);
        close(d2);
        if (rmdir(base) != 0) return fail("at rmdir base");
        if (rmdir(base2) != 0) return fail("at rmdir base2");
        printf("DIFF_AT_OK\n");
    }

    /* Big files: a 3 MiB round trip through multi-level block addressing
       (direct, single-indirect and double-indirect pages), with hole
       zero-fill, shrink, rewrite and removal. ext2 already handles big
       files; this covers the page-based filesystems. */
    {
        char bbase[128], big[128];
        const char *envb = getenv("SLOP_MNU_DIFF");
        if (envb) snprintf(bbase, sizeof bbase, "/bigfs");
        else snprintf(bbase, sizeof bbase, "/tmp/bigfs-%d", (int)getpid());
        snprintf(big, sizeof big, "%s/big.bin", bbase);
        (void)unlink(big);
        (void)rmdir(bbase);
        if (mkdir(bbase, 0755) != 0) return fail("big mkdir");
        const size_t total = 3 * 1024 * 1024;
        int fw = open(big, O_WRONLY | O_CREAT | O_TRUNC, 0644);
        if (fw < 0) return fail("big create");
        {
            unsigned char chunk[8192];
            size_t off = 0;
            while (off < total) {
                size_t n = sizeof chunk;
                if (n > total - off) n = total - off;
                for (size_t i = 0; i < n; i++) chunk[i] = (unsigned char)(((off + i) * 131 + 7) & 0xff);
                if (write(fw, chunk, n) != (ssize_t)n) return fail("big write");
                off += n;
            }
        }
        close(fw);
        struct stat bst;
        if (stat(big, &bst) != 0 || bst.st_size != (off_t)total) return fail("big size");
        int fr = open(big, O_RDONLY);
        if (fr < 0) return fail("big reopen");
        {
            unsigned char chunk[8192];
            size_t off = 0;
            while (off < total) {
                size_t n = sizeof chunk;
                if (n > total - off) n = total - off;
                ssize_t got = 0;
                while ((size_t)got < n) {
                    ssize_t r = read(fr, chunk + got, n - got);
                    if (r <= 0) return fail("big read");
                    got += r;
                }
                for (size_t i = 0; i < n; i++)
                    if (chunk[i] != (unsigned char)(((off + i) * 131 + 7) & 0xff)) return fail("big content");
                off += n;
            }
        }
        close(fr);
        if (truncate(big, 100) != 0) return fail("big shrink");
        if (stat(big, &bst) != 0 || bst.st_size != 100) return fail("big shrink size");
        /* Sparse rewrite far past the end: the gap reads back as zeros. */
        {
            int sw = open(big, O_WRONLY);
            if (sw < 0) return fail("big sparse open");
            if (lseek(sw, 2 * 1024 * 1024, SEEK_SET) != 2 * 1024 * 1024) return fail("big sparse seek");
            if (write(sw, "TAIL", 4) != 4) return fail("big sparse write");
            close(sw);
            if (stat(big, &bst) != 0 || bst.st_size != 2 * 1024 * 1024 + 4) return fail("big sparse size");
            int sr = open(big, O_RDONLY);
            if (sr < 0) return fail("big sparse reopen");
            unsigned char zb[16] = {0};
            if (lseek(sr, 1024 * 1024, SEEK_SET) != 1024 * 1024) return fail("big sparse seek2");
            if (read(sr, zb, sizeof zb) != (ssize_t)sizeof zb) return fail("big sparse read");
            for (int i = 0; i < 16; i++) if (zb[i] != 0) return fail("big hole zero");
            char tail[4] = {0};
            if (lseek(sr, 2 * 1024 * 1024, SEEK_SET) != 2 * 1024 * 1024) return fail("big sparse seek3");
            if (read(sr, tail, sizeof tail) != 4 || memcmp(tail, "TAIL", 4) != 0) return fail("big tail");
            close(sr);
        }
        if (unlink(big) != 0) return fail("big unlink");
        if (stat(big, &bst) == 0 || errno != ENOENT) return fail("big gone");
        if (rmdir(bbase) != 0) return fail("big rmdir");
        printf("DIFF_BIGFILE_OK\n");
    }

    /* Persistent-style files on a real on-disk filesystem: natively /tmp,
       in the guest the ext2 mount. Exercises create/write/read/truncate,
       multi-block mappings, sparse holes, directories and removal. */
    if (!guest_ext4) {
        char basebuf[128];
        const char *base;
        if (getenv("SLOP_MNU_DIFF")) base = "/mnt/xdiag";
        else { snprintf(basebuf, sizeof basebuf, "/tmp/xdiag-%d", (int)getpid()); base = basebuf; }
        char sub[128], f1[128], f2[128], big[128], keep[128];
        snprintf(sub, sizeof sub, "%s/sub", base);
        snprintf(f1, sizeof f1, "%s/a.txt", base);
        snprintf(f2, sizeof f2, "%s/sub/b.txt", base);
        snprintf(big, sizeof big, "%s/big.bin", base);
        snprintf(keep, sizeof keep, "%s/keep.bin", base);
        /* Best-effort cleanup so reruns start from the same state. */
        (void)unlink(f1); (void)unlink(f2); (void)unlink(big); (void)unlink(keep);
        (void)rmdir(sub); (void)rmdir(base);
        if (mkdir(base, 0755) != 0) return fail("ext2 mkdir base");
        if (mkdir(base, 0755) != -1 || errno != EEXIST) return fail("ext2 mkdir eexist");
        if (mkdir(sub, 0700) != 0) return fail("ext2 mkdir sub");
        int f = open(f1, O_WRONLY | O_CREAT | O_TRUNC, 0644);
        if (f < 0) return fail("ext2 create");
        if (write(f, "0123456789abcdef", 16) != 16) return fail("ext2 seed");
        close(f);
        f = open(f1, O_RDONLY);
        if (f < 0) return fail("ext2 reopen");
        char head[16] = {0};
        if (read(f, head, sizeof head) != 16 || memcmp(head, "0123456789abcdef", 16) != 0) return fail("ext2 content");
        struct stat st;
        if (fstat(f, &st) != 0 || st.st_size != 16) return fail("ext2 fstat");
        close(f);
        /* A 20 KiB patterned file crosses the 12 direct blocks into the
           single-indirect range on 1 KiB blocks. */
        int bf = open(big, O_WRONLY | O_CREAT | O_TRUNC, 0644);
        if (bf < 0) return fail("ext2 big create");
        unsigned char chunk[1024];
        for (int c = 0; c < 20; c++) {
            for (int i = 0; i < 1024; i++) chunk[i] = (unsigned char)((c * 131 + i * 7 + 3) & 0xff);
            if (write(bf, chunk, sizeof chunk) != (ssize_t)sizeof chunk) return fail("ext2 big write");
        }
        close(bf);
        bf = open(big, O_RDONLY);
        if (bf < 0) return fail("ext2 big reopen");
        if (fstat(bf, &st) != 0 || st.st_size != 20480) return fail("ext2 big size");
        for (int c = 0; c < 20; c++) {
            unsigned char rb[1024];
            if (read(bf, rb, sizeof rb) != (ssize_t)sizeof rb) return fail("ext2 big read");
            for (int i = 0; i < 1024; i++)
                if (rb[i] != (unsigned char)((c * 131 + i * 7 + 3) & 0xff)) return fail("ext2 big content");
        }
        close(bf);
        /* Sparse growth reads back as zeros; an interior write sticks. */
        int sf = open(f2, O_WRONLY | O_CREAT | O_TRUNC, 0644);
        if (sf < 0) return fail("ext2 sparse create");
        if (ftruncate(sf, 8192) != 0) return fail("ext2 sparse grow");
        if (lseek(sf, 5000, SEEK_SET) != 5000) return fail("ext2 sparse seek");
        if (write(sf, "XYZ", 3) != 3) return fail("ext2 sparse write");
        close(sf);
        sf = open(f2, O_RDONLY);
        if (sf < 0) return fail("ext2 sparse reopen");
        if (fstat(sf, &st) != 0 || st.st_size != 8192) return fail("ext2 sparse size");
        unsigned char zb[5003] = {0};
        if (read(sf, zb, sizeof zb) != (ssize_t)sizeof zb) return fail("ext2 sparse read");
        for (int i = 0; i < 5000; i++) if (zb[i] != 0) return fail("ext2 hole zero");
        if (memcmp(zb + 5000, "XYZ", 3) != 0) return fail("ext2 sparse content");
        close(sf);
        /* Shrink, directory listing, error cases, removal. */
        if (truncate(big, 100) != 0) return fail("ext2 shrink");
        if (stat(big, &st) != 0 || st.st_size != 100) return fail("ext2 shrink size");
        /* Links share the inode, symlinks resolve, renames move names. */
        {
            char l1[128], sl[128], f1r[128], sub2[128], sub3[128];
            snprintf(l1, sizeof l1, "%s/a-link", base);
            snprintf(sl, sizeof sl, "%s/a-sym", base);
            snprintf(f1r, sizeof f1r, "%s/a-renamed", base);
            snprintf(sub2, sizeof sub2, "%s/sub2", base);
            snprintf(sub3, sizeof sub3, "%s/sub3", base);
            (void)unlink(l1); (void)unlink(sl); (void)unlink(f1r);
            (void)rmdir(sub2); (void)rmdir(sub3);
            if (link(f1, l1) != 0) return fail("ext2 link");
            struct stat la, lb;
            if (stat(f1, &la) != 0 || stat(l1, &lb) != 0) return fail("ext2 link stat");
            if (la.st_ino != lb.st_ino || la.st_nlink != 2) return fail("ext2 link shared");
            int lf = open(l1, O_RDONLY);
            if (lf < 0) return fail("ext2 link open");
            char lh[16] = {0};
            if (read(lf, lh, sizeof lh) != 16 || memcmp(lh, "0123456789abcdef", 16) != 0) return fail("ext2 link content");
            close(lf);
            if (unlink(l1) != 0) return fail("ext2 link unlink");
            if (stat(f1, &la) != 0 || la.st_nlink != 1) return fail("ext2 link nlink");
            if (symlink("a.txt", sl) != 0) return fail("ext2 symlink");
            char rl[16] = {0};
            if (readlink(sl, rl, sizeof rl) != 5 || memcmp(rl, "a.txt", 5) != 0) return fail("ext2 readlink");
            struct stat ls, fs;
            if (lstat(sl, &ls) != 0 || !S_ISLNK(ls.st_mode) || ls.st_size != 5) return fail("ext2 lstat link");
            if (stat(sl, &fs) != 0 || !S_ISREG(fs.st_mode) || fs.st_size != 16) return fail("ext2 stat through");
            int sfd = open(sl, O_RDONLY);
            if (sfd < 0) return fail("ext2 open through");
            if (read(sfd, lh, 5) != 5 || memcmp(lh, "01234", 5) != 0) return fail("ext2 link read");
            close(sfd);
            /* A long target exercises the block-backed symlink path. */
            {
                char tgt[101];
                memset(tgt, 'x', 100);
                tgt[100] = 0;
                char sl2[128];
                snprintf(sl2, sizeof sl2, "%s/b-sym", base);
                (void)unlink(sl2);
                if (symlink(tgt, sl2) != 0) return fail("ext2 symlink long");
                char rb[104] = {0};
                if (readlink(sl2, rb, sizeof rb) != 100 || memcmp(rb, tgt, 100) != 0) return fail("ext2 readlink long");
                if (unlink(sl2) != 0) return fail("ext2 symlink long unlink");
            }
            if (unlink(sl) != 0) return fail("ext2 symlink unlink");
            if (rename(f1, f1r) != 0) return fail("ext2 rename");
            if (stat(f1, &st) == 0 || errno != ENOENT) return fail("ext2 rename gone");
            if (stat(f1r, &st) != 0 || st.st_size != 16) return fail("ext2 rename target");
            if (rename(f1r, f1) != 0) return fail("ext2 rename back");
            if (mkdir(sub2, 0700) != 0) return fail("ext2 mkdir sub2");
            if (rename(sub2, sub3) != 0) return fail("ext2 rename dir");
            if (stat(sub2, &st) == 0 || errno != ENOENT) return fail("ext2 rename dir gone");
            if (stat(sub3, &st) != 0 || !S_ISDIR(st.st_mode)) return fail("ext2 rename dir target");
            if (rmdir(sub3) != 0) return fail("ext2 rmdir sub3");
        }
        DIR *dd = opendir(sub);
        if (!dd) return fail("ext2 opendir");
        int saw_b = 0, entries = 0;
        struct dirent *de;
        while ((de = readdir(dd)) != NULL) {
            if (!strcmp(de->d_name, ".") || !strcmp(de->d_name, "..")) continue;
            entries++;
            if (!strcmp(de->d_name, "b.txt")) saw_b = 1;
        }
        closedir(dd);
        if (!saw_b || entries != 1) return fail("ext2 readdir");
        if (unlink(sub) != -1 || errno != EISDIR) return fail("ext2 unlink dir");
        if (rmdir(sub) != -1 || errno != ENOTEMPTY) return fail("ext2 rmdir full");
        char missing[128];
        snprintf(missing, sizeof missing, "%s/nope", base);
        if (unlink(missing) != -1 || errno != ENOENT) return fail("ext2 unlink missing");
        if (rmdir(missing) != -1 || errno != ENOENT) return fail("ext2 rmdir missing");
        if (unlink(f1) != 0 || unlink(f2) != 0 || unlink(big) != 0) return fail("ext2 unlink");
        if (stat(f1, &st) == 0 || errno != ENOENT) return fail("ext2 gone");
        if (rmdir(sub) != 0) return fail("ext2 rmdir");
        /* keep.bin stays on purpose: the host verifies its bytes straight
           off the platter with debugfs after the guest run. */
        int kf = open(keep, O_WRONLY | O_CREAT | O_TRUNC, 0644);
        if (kf < 0) return fail("ext2 keep create");
        if (write(kf, "0123456789abcdef", 16) != 16) return fail("ext2 keep write");
        close(kf);
        printf("DIFF_EXT2_OK\n");
    } else {
        printf("DIFF_EXT2_SKIP\n");
    }

    /* Read-only ext4 on the same virtio-blk device: natively the seed tree,
       in the guest the ext4 mount. Exercises superblock detect, extent
       mapping, directories, both symlink forms and EROFS on mutation. */
    if (guest_ext4 || !getenv("SLOP_MNU_DIFF")) {
        const char *base = getenv("SLOP_MNU_DIFF") ? "/mnt/xdiag4" : "/tmp/xdiag4";
        int guest = getenv("SLOP_MNU_DIFF") != 0;
        char hello[128], nested[128], asym[128], bsym[128], big[128], sub[128];
        snprintf(hello, sizeof hello, "%s/hello.txt", base);
        snprintf(nested, sizeof nested, "%s/sub/nested.txt", base);
        snprintf(asym, sizeof asym, "%s/a-sym", base);
        snprintf(bsym, sizeof bsym, "%s/b-sym", base);
        snprintf(big, sizeof big, "%s/big.bin", base);
        snprintf(sub, sizeof sub, "%s/sub", base);
        int f = open(hello, O_RDONLY);
        if (f < 0) return fail("ext4 open hello");
        char hbuf[35] = {0};
        if (read(f, hbuf, sizeof hbuf) != 35) return fail("ext4 hello size");
        if (memcmp(hbuf, "Hello from ext4.\nSecond line here.\n", 35) != 0) return fail("ext4 hello content");
        struct stat st;
        if (fstat(f, &st) != 0 || st.st_size != 35 || !S_ISREG(st.st_mode)) return fail("ext4 hello stat");
        close(f);
        int nf = open(nested, O_RDONLY);
        if (nf < 0) return fail("ext4 open nested");
        char nbuf[25] = {0};
        if (read(nf, nbuf, sizeof nbuf) != 25 || memcmp(nbuf, "nested content 0123456789\n", 25) != 0) return fail("ext4 nested content");
        close(nf);
        char rl[16] = {0};
        if (readlink(asym, rl, sizeof rl) != 9 || memcmp(rl, "hello.txt", 9) != 0) return fail("ext4 readlink fast");
        int af = open(asym, O_RDONLY);
        if (af < 0) return fail("ext4 open through fast link");
        char ab[5] = {0};
        if (read(af, ab, sizeof ab) != 5 || memcmp(ab, "Hello", 5) != 0) return fail("ext4 read through");
        close(af);
        char rb[104] = {0};
        if (readlink(bsym, rb, sizeof rb) != 100) return fail("ext4 readlink long");
        for (int i = 0; i < 100; i++) if (rb[i] != 'x') return fail("ext4 readlink long content");
        int bf = open(big, O_RDONLY);
        if (bf < 0) return fail("ext4 big open");
        if (fstat(bf, &st) != 0 || st.st_size != 102400) return fail("ext4 big size");
        for (int c = 0; c < 25; c++) {
            unsigned char rb2[4096];
            if (read(bf, rb2, sizeof rb2) != (ssize_t)sizeof rb2) return fail("ext4 big read");
            for (int i = 0; i < 4096; i++)
                if (rb2[i] != (unsigned char)((c * 131 + i * 7 + 3) & 0xff)) return fail("ext4 big content");
        }
        close(bf);
        DIR *dd = opendir(sub);
        if (!dd) return fail("ext4 opendir");
        int saw_nested = 0, entries = 0;
        struct dirent *de;
        while ((de = readdir(dd)) != NULL) {
            if (!strcmp(de->d_name, ".") || !strcmp(de->d_name, "..")) continue;
            entries++;
            if (!strcmp(de->d_name, "nested.txt")) saw_nested = 1;
        }
        closedir(dd);
        if (!saw_nested || entries != 1) return fail("ext4 readdir");
        struct stat ls, fs;
        if (lstat(asym, &ls) != 0 || !S_ISLNK(ls.st_mode) || ls.st_size != 9) return fail("ext4 lstat link");
        if (stat(asym, &fs) != 0 || !S_ISREG(fs.st_mode) || fs.st_size != 35) return fail("ext4 stat through");
        if (guest) {
            char nb[128], nl[128];
            snprintf(nb, sizeof nb, "%s/nope.txt", base);
            snprintf(nl, sizeof nl, "%s/nodir", base);
            int wf = open(hello, O_WRONLY);
            if (wf >= 0) {
                char one = 'x';
                int w = (int)write(wf, &one, 1);
                close(wf);
                if (w >= 0) return fail("ext4 write allowed");
            }
            if (open(nb, O_WRONLY | O_CREAT | O_TRUNC, 0644) >= 0) return fail("ext4 create allowed");
            if (mkdir(nl, 0755) == 0) return fail("ext4 mkdir allowed");
            if (unlink(hello) == 0) return fail("ext4 unlink allowed");
            if (rmdir(sub) == 0) return fail("ext4 rmdir allowed");
            if (truncate(big, 100) == 0) return fail("ext4 truncate allowed");
            int ef = open(hello, O_WRONLY);
            if (ef >= 0) close(ef);
        }
        printf("DIFF_EXT4_OK\n");
    } else {
        printf("DIFF_EXT4_SKIP\n");
    }

    if (access("/bin/nopie-helper", X_OK) == 0) {
        pid_t pid = fork();
        if (pid < 0) return fail("nopie fork");
        if (pid == 0) {
            char *args[] = { "/bin/nopie-helper", "fixed", 0 };
            char *env[] = { "SLOP_MNU_NOPIE=1", 0 };
            execve("/bin/nopie-helper", args, env);
            _exit(71);
        }
        int st;
        if (waitpid(pid, &st, 0) != pid) return fail("nopie waitpid");
        if (!WIFEXITED(st) || WEXITSTATUS(st) != 17) return fail("nopie exec");
        printf("DIFF_NOPIE_OK\n");
    }

    int memfd = memfd_create("diff", 0);

    if (memfd < 0) return fail("memfd_create");
    close(memfd);

    printf("DIFF_DONE\n");
    return 0;
}
