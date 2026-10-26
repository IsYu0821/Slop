/* Minimal DNS A-record client: queries a DNS server over UDP and prints
   the first A answer. Proves the full DNS request/response path. */
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <errno.h>

static unsigned short txid = 0x1234;

static int build_query(unsigned char *out, const char *name) {
    unsigned char *p = out;
    /* Header: ID, flags=0x0100 (RD), QD=1, AN=0, NS=0, AR=0 */
    p[0] = txid >> 8; p[1] = txid & 0xff;
    p[2] = 0x01; p[3] = 0x00;
    p[4] = 0x00; p[5] = 0x01;
    p[6] = 0x00; p[7] = 0x00;
    p[8] = 0x00; p[9] = 0x00;
    p[10] = 0x00; p[11] = 0x00;
    p += 12;
    /* QNAME */
    const char *s = name;
    while (*s) {
        const char *dot = strchr(s, '.');
        int len = dot ? (int)(dot - s) : (int)strlen(s);
        if (len <= 0 || len > 63) return -1;
        *p++ = (unsigned char)len;
        memcpy(p, s, (size_t)len);
        p += len;
        s += len;
        if (*s == '.') s++;
    }
    *p++ = 0;
    /* QTYPE=A, QCLASS=IN */
    *p++ = 0; *p++ = 1;
    *p++ = 0; *p++ = 1;
    return (int)(p - out);
}

static int skip_name(unsigned char *msg, int len, int off) {
    while (off < len) {
        unsigned char c = msg[off];
        if (c == 0) return off + 1;
        if ((c & 0xc0) == 0xc0) return off + 2;
        off += 1 + c;
    }
    return -1;
}

int main(int argc, char **argv) {
    const char *server = (argc > 1) ? argv[1] : "10.0.2.2";
    int port = (argc > 2) ? atoi(argv[2]) : 5353;
    const char *name = (argc > 3) ? argv[3] : "test.local";
    int fd = socket(AF_INET, SOCK_DGRAM, 0);
    if (fd < 0) { printf("dns: socket failed\n"); return 1; }
    struct sockaddr_in srv;
    memset(&srv, 0, sizeof srv);
    srv.sin_family = AF_INET;
    srv.sin_port = htons((unsigned short)port);
    if (inet_pton(AF_INET, server, &srv.sin_addr) != 1) {
        printf("dns: bad server\n");
        return 1;
    }
    unsigned char q[512];
    int qlen = build_query(q, name);
    if (qlen < 0) { printf("dns: bad name\n"); return 1; }
    if (sendto(fd, q, (size_t)qlen, 0, (struct sockaddr *)&srv, sizeof srv) != qlen) {
        printf("dns: sendto failed\n");
        return 1;
    }
    printf("dns: query sent for %s\n", name);
    /* Wait up to ~5s, polling. */
    for (int i = 0; i < 50; i++) {
        usleep(100000);
        unsigned char r[512];
        struct sockaddr_in from;
        socklen_t fl = sizeof from;
        int n = (int)recvfrom(fd, r, sizeof r, MSG_DONTWAIT,
                              (struct sockaddr *)&from, &fl);
        if (n < 12) continue;
        if (r[0] != (txid >> 8) || r[1] != (txid & 0xff)) continue;
        int rcode = r[3] & 0x0f;
        int qd = (r[4] << 8) | r[5];
        int an = (r[6] << 8) | r[7];
        printf("dns: response rcode=%d qd=%d an=%d\n", rcode, qd, an);
        if (rcode != 0) { printf("dns: server error\n"); return 2; }
        if (an < 1) { printf("dns: no answers\n"); return 3; }
        int off = 12;
        for (int q = 0; q < qd; q++) {
            off = skip_name(r, n, off);
            if (off < 0) return 4;
            off += 4;
        }
        for (int a = 0; a < an; a++) {
            off = skip_name(r, n, off);
            if (off < 0 || off + 10 > n) return 5;
            int type = (r[off] << 8) | r[off + 1];
            int rdlen = (r[off + 8] << 8) | r[off + 9];
            off += 10;
            if (type == 1 && rdlen == 4 && off + 4 <= n) {
                printf("dns: A %d.%d.%d.%d\n", r[off], r[off+1], r[off+2], r[off+3]);
                printf("DNS_OK\n");
                return 0;
            }
            off += rdlen;
        }
        printf("dns: no A record\n");
        return 6;
    }
    printf("dns: timeout\n");
    return 7;
}
