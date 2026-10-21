#!/usr/bin/env python3
"""Dual UDP/TCP echo server for the Slop/MNU acceptance test.

Slirp forwards guest traffic to 10.0.2.2:5555 as host 127.0.0.1:5555, so a
single port carries both protocols. Writes build/net-echo.ready once both
sockets are bound so the acceptance script can wait for readiness.
"""
import os
import socket
import sys
import threading

HOST = "127.0.0.1"
PORT = 5555
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
READY = os.path.join(ROOT, "build", "net-echo.ready")


def udp_loop(sock):
    while True:
        data, addr = sock.recvfrom(2048)
        sock.sendto(data, addr)


def tcp_loop(sock):
    while True:
        conn, _ = sock.accept()
        threading.Thread(target=tcp_conn, args=(conn,), daemon=True).start()


def tcp_conn(conn):
    try:
        while True:
            data = conn.recv(2048)
            if not data:
                break
            conn.sendall(data)
    finally:
        conn.close()


def main():
    udp = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    udp.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    udp.bind((HOST, PORT))
    listening = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listening.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listening.bind((HOST, PORT))
    listening.listen(4)
    threading.Thread(target=udp_loop, args=(udp,), daemon=True).start()
    threading.Thread(target=tcp_loop, args=(listening,), daemon=True).start()
    with open(READY, "w") as f:
        f.write("ready\n")
    sys.stdout.write("net-echo: listening on %s:%d (udp+tcp)\n" % (HOST, PORT))
    sys.stdout.flush()
    threading.Event().wait()


if __name__ == "__main__":
    main()
