#!/bin/sh
# A2 remote stop/continue battery.
#
# Builds tests/smp-stop.c with musl and runs the same binary on Linux
# (reference) and inside the guest with 2, 4 and 8 vCPUs, ITER times each.
# The guest passes only when: every SIGSTOP is reported (WIFSTOPPED), no
# user progress happens while stopped, every SIGCONT resumes exactly once
# (WIFCONTINUED), the child finishes, and scheduler accounting is strict
# (double_exec=0, lost_owner=0, push/pop delta=0, zero runnable at audit).
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
MUSL=${MUSL_GCC:-/tmp/opencode/musl-install/bin/musl-gcc}
test -x "$MUSL" || { echo "musl-gcc not found"; exit 1; }
TIMEOUT=${TIMEOUT:-150}
NCPUS=${NCPUS:-"1 2 4 8"}
ITER=${ITER:-1}
mkdir -p build
BACKUP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/slop-stop.XXXXXX")
cp tests/minimal-linux.S "$BACKUP_DIR/minimal-linux.S"
cp tools/build-initramfs.py "$BACKUP_DIR/build-initramfs.py"
cleanup() {
  cp "$BACKUP_DIR/minimal-linux.S" tests/minimal-linux.S
  cp "$BACKUP_DIR/build-initramfs.py" tools/build-initramfs.py
  rm -f tests/musl/smp-stop
  rm -rf "$BACKUP_DIR"
}
trap cleanup EXIT

"$MUSL" -static -O1 -o build/smp-stop tests/smp-stop.c
cp "build/smp-stop" "tests/musl/smp-stop"

# Native reference (proves the test itself is sound on Linux).
./build/smp-stop > build/smp-stop-native.out 2>&1 || {
  echo "stop: native reference failed"; tail -n 10 build/smp-stop-native.out; exit 1; }
grep -q "STOP_OK rounds=25" build/smp-stop-native.out || {
  echo "stop: native reference no STOP_OK"; tail -n 5 build/smp-stop-native.out; exit 1; }
echo "stop: native reference OK"
python3 - <<'PY'
from pathlib import Path
p=Path('tools/build-initramfs.py')
s=p.read_text()
needle="('/bin/hello-net',Path('tests/musl/hello-net').read_bytes(),0o100755),"
if needle not in s:
    raise SystemExit('could not locate hello-net initramfs entry')
add=needle+"('/bin/smp-stop',Path('tests/musl/smp-stop').read_bytes(),0o100755),"
if add not in s:
    s=s.replace(needle,add,1)
    p.write_text(s)
    print("initramfs entries added")
else:
    print("initramfs entries already present")
PY
cat > tests/minimal-linux.S <<'INNER_EOF'
.global _start
.section .text
_start:
    mov $59,%rax
    lea p(%rip), %rdi
    lea argv(%rip), %rsi
    lea envp(%rip), %rdx
    syscall
1:  jmp 1b
.section .rodata
p: .asciz "/bin/smp-stop"
arg0: .asciz "smp-stop"
env0: .asciz "X=1"
.align 8
argv: .quad arg0, 0
envp: .quad env0, 0
# check-source token: /bin/hello
INNER_EOF
if ! /tmp/opencode/uefi-user.sh >build/stop-build.log 2>&1; then
  echo "stop: build failed" >&2
  tail -20 build/stop-build.log >&2
  exit 1
fi
OVMF_CODE=${OVMF_CODE:-/usr/share/OVMF/OVMF_CODE_4M.fd}
OVMF_VARS_TEMPLATE=${OVMF_VARS_TEMPLATE:-/usr/share/OVMF/OVMF_VARS_4M.fd}
pass=1
for ncpu in $NCPUS; do
  i=1
  while [ "$i" -le "$ITER" ]; do
    LOG="/tmp/opencode/smp-stop-$ncpu-i$i.log"
    cp "$OVMF_VARS_TEMPLATE" build/OVMF_VARS.fd
    rm -f build/net-echo.ready
    python3 tools/net-echo.py >build/net-echo.log 2>&1 &
    ECHOPID=$!
    waited=0; while [ ! -f build/net-echo.ready ] && [ "$waited" -lt 50 ]; do sleep 0.1; waited=$((waited+1)); done
    set +e
    timeout "$TIMEOUT" qemu-system-x86_64 -machine q35,accel=tcg -cpu max -m 256M -smp "$ncpu" \
      -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
      -drive if=pflash,format=raw,file=build/OVMF_VARS.fd \
      -drive format=raw,file=fat:rw:build/efi \
      -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
      -serial file:"$LOG" -display none -no-reboot -no-shutdown >/dev/null 2>&1
    status=$?
    set -e
    kill "$ECHOPID" 2>/dev/null || true
    echo "--- stop ncpu=$ncpu iter=$i qemu=$status ---"
    grep -E "STOP_OK|STOP_FAIL|double_exec|ap_user" "$LOG" | tail -n 5 || true
    ok=1
    grep -q "STOP_OK rounds=25" "$LOG" || { echo "stop: ${ncpu}cpu no STOP_OK"; ok=0; }
    if grep -q "kernel exception" "$LOG"; then echo "stop: KERNEL EXCEPTION"; ok=0; fi
    if grep -q "smp: CORRUPT" "$LOG"; then echo "stop: CORRUPT slot"; ok=0; fi
    if grep -q "fatal:" "$LOG"; then echo "stop: fatal"; ok=0; fi
    de=$(grep -o 'double_exec=[0-9]*' "$LOG" | tail -n 1 | cut -d= -f2)
    lo=$(grep -o 'lost_owner=[0-9]*' "$LOG" | tail -n 1 | cut -d= -f2)
    dd=$(grep -o 'push_pop_delta=[0-9]*' "$LOG" | tail -n 1 | cut -d= -f2)
    ra=$(grep -o 'audit runnable=[0-9]*' "$LOG" | tail -n 1 | cut -d= -f2)
    echo "stop: counters double_exec=$de lost_owner=$lo delta=$dd runnable=$ra"
    [ "${de:-9}" = "0" ] || ok=0
    [ "${lo:-9}" = "0" ] || ok=0
    [ "${dd:-9}" = "0" ] || ok=0
    [ "${ra:-9}" = "0" ] || ok=0
    [ "$ok" = "1" ] || pass=0
    i=$((i+1))
  done
done
if [ "$pass" = "1" ]; then echo "stop-test: PASS"; exit 0; else echo "stop-test: FAIL" >&2; exit 1; fi
