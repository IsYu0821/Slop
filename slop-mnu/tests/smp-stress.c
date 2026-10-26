/* SMP parallel-execution stress: fork CPU-bound children that record the
   CPU they run on (getcpu), exchange pipe messages, and do file I/O.
   A start barrier ensures all children become runnable at once so every
   online CPU must participate; the parent requires at least two distinct
   CPUs when SMP is available. */
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <errno.h>

#ifndef SYS_getcpu
#define SYS_getcpu 309
#endif

#define NCHILD 8

static int my_cpu(void) {
    unsigned cpu = 0xdeadbeef;
    long r = syscall(SYS_getcpu, &cpu, 0, 0);
    if (r != 0) return -1;
    return (int)cpu;
}

static void burn(int iters) {
    volatile unsigned x = 0x12345678u;
    for (int i = 0; i < iters; i++) {
        x = x * 1103515245u + 12345u;
        x ^= x >> 13;
    }
    if (x == 0xabcdefu) write(2, "impossible\n", 11);
}

int main(void) {
    int pipes[NCHILD][2];
    int startp[2];
    pid_t pids[NCHILD];
    if (pipe(startp) != 0) { printf("smp: start pipe failed\n"); return 1; }
    for (int i = 0; i < NCHILD; i++) {
        if (pipe(pipes[i]) != 0) { printf("smp: pipe failed\n"); return 1; }
    }
    for (int i = 0; i < NCHILD; i++) {
        pid_t p = fork();
        if (p < 0) { printf("smp: fork failed\n"); return 1; }
        if (p == 0) {
            for (int j = 0; j < NCHILD; j++) {
                if (j != i) { close(pipes[j][0]); close(pipes[j][1]); }
                else { close(pipes[j][0]); }
            }
            /* Wait for the start gun (blocks until parent releases all). */
            char go;
            close(startp[1]);
            if (read(startp[0], &go, 1) != 1) _exit(20);
            close(startp[0]);
            burn(30000000);
            int c0 = my_cpu();
            burn(30000000);
            int c1 = my_cpu();
            /* File I/O from this CPU. */
            char path[64];
            snprintf(path, sizeof path, "/tmp/smp%d", i);
            int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0600);
            if (fd < 0) { printf("smp: child open failed\n"); _exit(10); }
            char buf[128];
            int n = snprintf(buf, sizeof buf, "child=%d cpu0=%d cpu1=%d\n", i, c0, c1);
            if (write(fd, buf, (size_t)n) != n) { printf("smp: child write failed\n"); _exit(11); }
            close(fd);
            fd = open(path, O_RDONLY);
            if (fd < 0) { printf("smp: child reopen failed\n"); _exit(12); }
            char rbuf[128];
            int rn = (int)read(fd, rbuf, sizeof rbuf - 1);
            close(fd);
            unlink(path);
            if (rn != n || memcmp(rbuf, buf, (size_t)n) != 0) {
                printf("smp: child reread mismatch\n");
                _exit(13);
            }
            /* Report cpus to parent. */
            char msg[64];
            int m = snprintf(msg, sizeof msg, "%d %d %d\n", i, c0, c1);
            if (write(pipes[i][1], msg, (size_t)m) != m) _exit(14);
            close(pipes[i][1]);
            _exit(0);
        }
        pids[i] = p;
        close(pipes[i][1]);
    }
    close(startp[0]);
    /* Release all children at once, then stay busy so BSP cannot
       sequentially grab every child before APs wake. */
    for (int i = 0; i < NCHILD; i++) {
        if (write(startp[1], "G", 1) != 1) { printf("smp: start write failed\n"); return 1; }
    }
    close(startp[1]);
    burn(20000000);
    int cpus[NCHILD][2];
    int ok = 1;
    for (int i = 0; i < NCHILD; i++) {
        char msg[64];
        int got = 0;
        while (got < 6) {
            int r = (int)read(pipes[i][0], msg + got, sizeof(msg) - 1 - (size_t)got);
            if (r <= 0) break;
            got += r;
            if (got > 0 && msg[got - 1] == '\n') break;
        }
        close(pipes[i][0]);
        msg[got] = 0;
        int id, c0, c1;
        if (sscanf(msg, "%d %d %d", &id, &c0, &c1) != 3) {
            printf("smp: parent bad msg from %d: %s\n", i, msg);
            ok = 0;
            c0 = c1 = -9;
        }
        cpus[i][0] = c0;
        cpus[i][1] = c1;
        printf("smp: child %d cpu0=%d cpu1=%d\n", i, c0, c1);
    }
    for (int i = 0; i < NCHILD; i++) {
        int st;
        if (waitpid(pids[i], &st, 0) < 0) { printf("smp: waitpid failed\n"); ok = 0; continue; }
        if (!WIFEXITED(st) || WEXITSTATUS(st) != 0) {
            printf("smp: child %d status bad\n", i);
            ok = 0;
        }
    }
    int distinct0 = 0, seen[16];
    int nseen = 0;
    for (int i = 0; i < NCHILD; i++) {
        int c = cpus[i][0];
        if (c < 0) { ok = 0; continue; }
        int f = 0;
        for (int j = 0; j < nseen; j++) if (seen[j] == c) f = 1;
        if (!f && nseen < 16) { seen[nseen++] = c; distinct0++; }
    }
    printf("smp: distinct cpus=%d\n", distinct0);
    if (distinct0 < 1) ok = 0;
    const char *env = getenv("SMP_EXPECT_PARALLEL");
    if (env && env[0] == '1' && distinct0 < 2) {
        printf("smp: expected parallel execution, got 1 cpu\n");
        ok = 0;
    }
    if (ok) printf("SMP_OK\n");
    else printf("SMP_FAIL\n");
    return ok ? 0 : 1;
}
