#!/bin/sh
set -u
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
LOG=${LOG:-build/acceptance-serial.log}
TIMEOUT=${TIMEOUT:-30}
mkdir -p build
rm -f "$LOG"
if ! ./tools/build-uefi.sh; then
  printf 'acceptance: build failed\n' >&2
  exit 1
fi
set +e
timeout "$TIMEOUT" ./tools/run-qemu.sh >"$LOG" 2>&1
status=$?
set -e
cat "$LOG"
# isa-debug-exit maps guest value 0x10 to host status 33.
if [ "$status" -ne 33 ]; then
  printf 'acceptance: unexpected QEMU status %s (expected 33)\n' "$status" >&2
  exit 1
fi
for text in \
  'local APIC timer and EOI passed' \
  'scheduler: context, FS base and zombie reap self-test passed' \
  'vfs: path lookup and file read self-test passed' \
  'execve: committed /bin/hello' \
  'argv, envp, VFS, VM, time, random and EFAULT passed' \
  'execve: committed /bin/hello-c' \
  'c runtime: main entered' \
  'c runtime: passed' \
  'execve: committed /bin/hello-posix' \
  'posix: main entered' \
  'posix: fork, pipe, dup2 and wait4 passed' \
  'execve: committed /bin/hello-signal' \
  'signals: main entered' \
  'signals: handler delivery, mask and default action passed' \
  'execve: committed /bin/hello-file' \
  'file: main entered' \
  'file: create, write, read, unlink, getdents64 and futex passed' \
  'execve: committed /bin/hello-syscall' \
  'syscalls: main entered' \
  'syscalls: iov, positional io, access, readlink and mremap passed' \
  'execve: committed /bin/hello-dyn' \
  'dyn: dynamic linking works' \
  'execve: committed /bin/hello-thread' \
  'thread: main entered' \
  'thread: create, join and mutex passed' \
  'task exited status=0' \
  'process address space reclaimed status=0 vector=0'; do
  if ! grep -F "$text" "$LOG" >/dev/null; then
    printf 'acceptance: missing serial marker: %s\n' "$text" >&2
    exit 1
  fi
done
if grep -F 'acceptance failed' "$LOG" >/dev/null || grep -F 'kernel exception' "$LOG" >/dev/null; then
  printf 'acceptance: failure marker found\n' >&2
  exit 1
fi
printf 'acceptance: PASS\n'
