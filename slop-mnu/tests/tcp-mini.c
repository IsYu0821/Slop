/* Minimal TCP loopback: listen, fork, connect, accept, ping-pong, close.
   Prints TCPMINI_OK or TCPMINI_FAIL <where>. */
#define _GNU_SOURCE
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>
#include <sys/wait.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>

int main(void) {
    int ls = socket(AF_INET, SOCK_STREAM, 0);
    if (ls < 0) { printf("TCPMINI_FAIL socket %d\n", errno); return 1; }
    struct sockaddr_in a;
    memset(&a, 0, sizeof a);
    a.sin_family = AF_INET;
    a.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    a.sin_port = htons(45311);
    if (bind(ls, (struct sockaddr *)&a, sizeof a) != 0) { printf("TCPMINI_FAIL bind %d\n", errno); return 1; }
    if (listen(ls, 4) != 0) { printf("TCPMINI_FAIL listen %d\n", errno); return 1; }
    pid_t c = fork();
    if (c < 0) { printf("TCPMINI_FAIL fork %d\n", errno); return 1; }
    if (c == 0) {
        int s = socket(AF_INET, SOCK_STREAM, 0);
        if (s < 0) _exit(10);
        struct sockaddr_in sa;
        memset(&sa, 0, sizeof sa);
        sa.sin_family = AF_INET;
        sa.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        sa.sin_port = htons(45311);
        if (connect(s, (struct sockaddr *)&sa, sizeof sa) != 0) _exit(11);
        if (send(s, "Hi", 2, 0) != 2) _exit(12);
        char rb[8] = {0};
        int got = 0;
        while (got < 2) { int n = recv(s, rb + got, 2 - got, 0); if (n <= 0) _exit(13); got += n; }
        if (memcmp(rb, "Yo", 2) != 0) _exit(14);
        close(s);
        _exit(0);
    }
    int s = accept(ls, 0, 0);
    if (s < 0) { printf("TCPMINI_FAIL accept %d\n", errno); return 1; }
    char rb[8] = {0};
    int got = 0;
    while (got < 2) { int n = recv(s, rb + got, 2 - got, 0); if (n <= 0) { printf("TCPMINI_FAIL recv %d\n", errno); return 1; } got += n; }
    if (memcmp(rb, "Hi", 2) != 0) { printf("TCPMINI_FAIL content\n"); return 1; }
    if (send(s, "Yo", 2, 0) != 2) { printf("TCPMINI_FAIL send %d\n", errno); return 1; }
    close(s);
    close(ls);
    int st = 0;
    if (waitpid(c, &st, 0) != c || !WIFEXITED(st) || WEXITSTATUS(st) != 0) {
        printf("TCPMINI_FAIL child st=%d errno=%d\n", st, errno); return 1;
    }
    printf("TCPMINI_OK\n");
    return 0;
}
