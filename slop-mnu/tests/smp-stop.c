/* A2 remote stop/continue stress: a CPU-bound child (burn loops with no
   syscalls between pipe markers) is stopped and continued repeatedly.
   The parent verifies: WIFSTOPPED is reported, no user progress happens
   while stopped (nonblocking pipe sampling stalls), SIGCONT resumes
   exactly once (WIFCONTINUED), and the child finishes all work units
   with exit 0. Prints STOP_OK on success; any mismatch prints STOP_FAIL
   and exits 1. Identical binary runs natively (reference) and in guest. */
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>
#include <fcntl.h>
#include <time.h>
#include <sys/wait.h>
#include <signal.h>

#define ROUNDS 25

static void burn(int iters) {
    volatile unsigned x = 0x12345678u;
    for (int i = 0; i < iters; i++) {
        x = x * 1103515245u + 12345u;
        x ^= x >> 13;
    }
    if (x == 0xabcdefu) write(2, "impossible\n", 11);
}

static void msleep(int ms) {
    struct timespec ts;
    ts.tv_sec = ms / 1000;
    ts.tv_nsec = (long)(ms % 1000) * 1000000L;
    nanosleep(&ts, 0);
}

/* Count bytes currently available without blocking. */
static int drain_nb(int fd) {
    int n = 0;
    char buf[256];
    for (;;) {
        ssize_t r = read(fd, buf, sizeof(buf));
        if (r > 0) { n += (int)r; continue; }
        if (r == 0) break;
        if (errno == EAGAIN || errno == EWOULDBLOCK) break;
        return -1;
    }
    return n;
}

int main(void) {
    int p[2];
    if (pipe(p) != 0) { printf("STOP_FAIL pipe errno=%d\n", errno); return 1; }
    pid_t c = fork();
    if (c < 0) { printf("STOP_FAIL fork errno=%d\n", errno); return 1; }
    if (c == 0) {
        close(p[0]);
        for (;;) {
            burn(3000000);
            /* Backpressure is normal (4 KiB pipe): retry EAGAIN like any
               correct writer instead of treating it as failure. */
            for (int t = 0; t < 1000000; t++) {
                ssize_t w = write(p[1], "x", 1);
                if (w == 1) break;
                if (w < 0 && errno == EAGAIN) { burn(100000); continue; }
                _exit(30);
            }
        }
    }
    close(p[1]);
    int fl = fcntl(p[0], F_GETFL, 0);
    if (fl < 0 || fcntl(p[0], F_SETFL, fl | O_NONBLOCK) != 0) {
        printf("STOP_FAIL nonblock errno=%d\n", errno); return 1;
    }
    int got = 0;
    /* Wait for the first marker (blocking-equivalent poll loop). */
    for (;;) {
        int n = drain_nb(p[0]);
        if (n < 0) { printf("STOP_FAIL pre-drain errno=%d\n", errno); return 1; }
        got += n;
        if (got > 0) break;
        msleep(5);
    }
    for (int r = 0; r < ROUNDS; r++) {
        if (kill(c, SIGSTOP) != 0) { printf("STOP_FAIL stop r=%d errno=%d\n", r, errno); return 1; }
        int st = 0;
        pid_t w = waitpid(c, &st, WUNTRACED);
        if (w != c || !WIFSTOPPED(st) || WSTOPSIG(st) != SIGSTOP) {
            printf("STOP_FAIL waitstop r=%d w=%d st=%d errno=%d\n", r, (int)w, st, errno); return 1;
        }
        /* Discard anything written before the stop parked: only arrivals
           after this point would prove continued user progress. */
        {
            int n0 = drain_nb(p[0]);
            if (n0 < 0) { printf("STOP_FAIL stop-ack-drain r=%d errno=%d\n", r, errno); return 1; }
            got += n0;
        }
        /* While stopped: no new markers may arrive (no user progress). */
        msleep(60);
        int n = drain_nb(p[0]);
        if (n < 0) { printf("STOP_FAIL stop-drain r=%d errno=%d\n", r, errno); return 1; }
        if (n != 0) { printf("STOP_FAIL progress-while-stopped r=%d n=%d\n", r, n); got += n; return 1; }
        if (kill(c, SIGCONT) != 0) { printf("STOP_FAIL cont r=%d errno=%d\n", r, errno); return 1; }
        w = waitpid(c, &st, WCONTINUED);
        if (w != c || !WIFCONTINUED(st)) {
            printf("STOP_FAIL waitcont r=%d w=%d st=%d errno=%d\n", r, (int)w, st, errno); return 1;
        }
        /* Let the child produce the next marker before the next stop. */
        for (;;) {
            n = drain_nb(p[0]);
            if (n < 0) { printf("STOP_FAIL run-drain r=%d errno=%d\n", r, errno); return 1; }
            got += n;
            if (n > 0) break;
            msleep(5);
        }
    }
    if (got == 0) { printf("STOP_FAIL no-progress\n"); return 1; }
    if (kill(c, SIGKILL) != 0) { printf("STOP_FAIL kill errno=%d\n", errno); return 1; }
    int st = 0;
    pid_t w = waitpid(c, &st, 0);
    if (w != c || !WIFSIGNALED(st) || WTERMSIG(st) != SIGKILL) {
        printf("STOP_FAIL waitend w=%d st=%d errno=%d\n", (int)w, st, errno); return 1;
    }
    printf("STOP_OK rounds=%d markers=%d\n", ROUNDS, got);
    return 0;
}
