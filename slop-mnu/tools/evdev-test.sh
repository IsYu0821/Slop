#!/bin/sh
# evdev end-to-end: guest boots Profile B, opens /dev/input/event0 (HID kbd),
# blocks in read; host injects 'a' via QMP send-key; guest must report
# KEY_A (30) press (value 1) + release + SYN. Proves evdev ABI on real HW path.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
PROFILE=${PROFILE:-B}
BACKUP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/slop-evdev.XXXXXX")
cp tests/minimal-linux.S "$BACKUP_DIR/minimal-linux.S"
cp tools/build-initramfs.py "$BACKUP_DIR/build-initramfs.py"
cleanup() {
  cp "$BACKUP_DIR/minimal-linux.S" tests/minimal-linux.S
  cp "$BACKUP_DIR/build-initramfs.py" tools/build-initramfs.py
  rm -f tests/musl/evdev-test
  rm -rf "$BACKUP_DIR"
}
trap cleanup EXIT
gcc -O1 -o build/evdev-test tests/evdev-test.c
cp build/evdev-test tests/musl/evdev-test
mkdir -p tests/glibc/lib64 tests/glibc/lib-x86_64
cp -L /lib64/ld-linux-x86-64.so.2 tests/glibc/lib64/ld-linux-x86-64.so.2
cp /lib/x86_64-linux-gnu/libc.so.6 tests/glibc/lib-x86_64/libc.so.6
cat > tests/minimal-linux.S <<'INNER_EOF'
.global _start
.section .text
_start:
    mov $59, %rax
    lea p(%rip), %rdi
    lea argv(%rip), %rsi
    lea envp(%rip), %rdx
    syscall
1:  jmp 1b
# mov $59,%rax
.section .rodata
p: .asciz "/bin/evdev-test"
arg0: .asciz "evdev-test"
.align 8
argv: .quad arg0, 0
envp: .quad 0
# /bin/hello
INNER_EOF
python3 - tools/build-initramfs.py <<'PYEOF'
import pathlib
p = pathlib.Path("tools/build-initramfs.py")
s = p.read_text()
needle = "('/bin/hello-net',Path('tests/musl/hello-net').read_bytes(),0o100755),"
assert needle in s
s = s.replace(needle, needle + """
('/bin/evdev-test',Path('tests/musl/evdev-test').read_bytes(),0o100755),
('/lib64/ld-linux-x86-64.so.2',Path('tests/glibc/lib64/ld-linux-x86-64.so.2').read_bytes(),0o755),
('/lib/x86_64-linux-gnu/libc.so.6',Path('tests/glibc/lib-x86_64/libc.so.6').read_bytes(),0o755),""")
p.write_text(s)
print("injected")
PYEOF
SERLOG=build/evdev-serial.log
QSOCK=/tmp/opencode/slop-evdev-qmp.sock
rm -f "$QSOCK" "$SERLOG"
./tools/build-uefi.sh >/dev/null 2>&1
SERIAL_LOG="$SERLOG" timeout 120 ./tools/qemu-profile.sh "$PROFILE" -qmp "unix:$QSOCK,server,nowait" >/dev/null 2>&1 &
QEMU_PID=$!
qcleanup() { kill $QEMU_PID 2>/dev/null || true; wait $QEMU_PID 2>/dev/null || true; }
trap 'qcleanup; cleanup' EXIT INT TERM
# wait for evdev node ready (up to 60s)
for i in $(seq 1 60); do
  if grep -aq "evdev /dev/input/event0 ready" "$SERLOG" 2>/dev/null; then break; fi
  sleep 1
done
grep -aq "evdev /dev/input/event0 ready" "$SERLOG" || { echo "evdev test: FAIL (no evdev node)"; exit 1; }
# wait for blocking read to start (test prints dev name), then send key
for i in $(seq 1 30); do
  if grep -aq "EVDEV_NOTE dev" "$SERLOG" 2>/dev/null; then break; fi
  sleep 1
done
python3 - "$QSOCK" <<'PYEOF'
import socket, json, sys
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect(sys.argv[1])
f = s.makefile('r')
f.readline()
def cmd(o):
    s.sendall((json.dumps(o) + '\n').encode())
    while True:
        line = f.readline()
        if not line: raise SystemExit("qmp eof")
        m = json.loads(line)
        if 'return' in m or 'error' in m: return m
r = cmd({"execute": "qmp_capabilities"})
assert 'return' in r, r
r = cmd({"execute": "send-key", "arguments": {"keys": [{"type": "qcode", "data": "a"}]}})
assert 'return' in r, r
print("qmp send-key ok")
PYEOF
# wait for EVDEV_OK (up to 30s)
for i in $(seq 1 30); do
  if grep -aq "EVDEV_OK" "$SERLOG" 2>/dev/null; then
    echo "evdev test: PASS (profile $PROFILE)"
    grep -a "EVDEV_NOTE ev t=1" "$SERLOG" | head -3
    exit 0
  fi
  if grep -aq "EVDEV_FAIL" "$SERLOG" 2>/dev/null; then
    echo "evdev test: FAIL (guest reported failure)"
    grep -a "EVDEV_FAIL" "$SERLOG" | head -3
    exit 1
  fi
  sleep 1
done
echo "evdev test: FAIL (timeout, no EVDEV_OK)"
grep -a "EVDEV_NOTE\|hid " "$SERLOG" | head -10 || true
exit 1
