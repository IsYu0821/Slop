#!/bin/sh
set -u
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
LOG=${LOG:-build/acceptance-serial.log}
TIMEOUT=${TIMEOUT:-30}
mkdir -p build
rm -f "$LOG"
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
