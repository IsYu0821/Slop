#!/bin/sh
# SMP parallel-execution battery (A1 stress).
#
# Builds tests/smp-stress.c and tests/smp-exec.c with musl and runs the same
# binaries on Linux (reference) and inside the guest with 1, 2, 4 and 8
# vCPUs, ITER times each. The guest passes only when children run on at
# least two different CPUs (multi-vCPU) and all pipe/file/exec/exit checks
# match the native reference behavior. Strict scheduler accounting applies:
# double_exec=0, lost_owner=0, push/pop delta=0, zero runnable at audit.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
MUSL=${MUSL_GCC:-/tmp/opencode/musl-install/bin/musl-gcc}
test -x "$MUSL" || { echo "musl-gcc not found"; exit 1; }
TIMEOUT=${TIMEOUT:-120}
NCPUS=${NCPUS:-"1 2 4 8"}
ITER=${ITER:-1}
mkdir -p build
BACKUP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/slop-smp.XXXXXX")
cp tests/minimal-linux.S "$BACKUP_DIR/minimal-linux.S"
cp tools/build-initramfs.py "$BACKUP_DIR/build-initramfs.py"
cleanup() {
  cp "$BACKUP_DIR/minimal-linux.S" tests/minimal-linux.S
  cp "$BACKUP_DIR/build-initramfs.py" tools/build-initramfs.py
  rm -f tests/musl/smp-stress tests/musl/smp-exec
  rm -rf "$BACKUP_DIR"
}
trap cleanup EXIT

"$MUSL" -static -O1 -o build/smp-stress tests/smp-stress.c
cp "build/smp-stress" "tests/musl/smp-stress"
"$MUSL" -static -O1 -o build/smp-exec tests/smp-exec.c
cp "build/smp-exec" "tests/musl/smp-exec"
# Silent raw-syscall exec target (no env/ppid assumptions); runs natively too.
clang -target x86_64-unknown-none-elf -ffreestanding -mno-red-zone -c tests/exec-probe.S -o build/exec-probe.o
ld.lld -nostdlib -static -T tests/minimal-linux.ld build/exec-probe.o -o build/exec-probe.elf

# Native reference (proves the tests themselves are sound on Linux).
SMP_EXPECT_PARALLEL=0 ./build/smp-stress > build/smp-native.out 2>&1 || {
  echo "smp: native reference failed"; tail -n 10 build/smp-native.out; exit 1; }
grep -q SMP_OK build/smp-native.out || { echo "smp: native reference no SMP_OK"; exit 1; }
SLOB_HELLO=build/exec-probe.elf ./build/smp-exec > build/smp-exec-native.out 2>&1 || {
  echo "exec: native reference failed"; tail -n 10 build/smp-exec-native.out; exit 1; }
grep -q "EXEC_OK exec=160 fork=364 sock=32" build/smp-exec-native.out || {
  echo "exec: native reference no EXEC_OK"; tail -n 5 build/smp-exec-native.out; exit 1; }
echo "smp: native reference OK"
python3 - <<'PY'
from pathlib import Path
p=Path('tools/build-initramfs.py')
s=p.read_text()
needle="('/bin/hello-net',Path('tests/musl/hello-net').read_bytes(),0o100755),"
if needle not in s:
    raise SystemExit('could not locate hello-net initramfs entry')
add=needle+"('/bin/exec-probe',Path('build/exec-probe.elf').read_bytes(),0o100755),('/bin/smp-stress',Path('tests/musl/smp-stress').read_bytes(),0o100755),('/bin/smp-exec',Path('tests/musl/smp-exec').read_bytes(),0o100755),"
if add not in s:
    s=s.replace(needle,add,1)
    p.write_text(s)
    print("initramfs entries added")
else:
    print("initramfs entries already present")
PY
write_stub() {
  # $1 = binary path in guest, $2 = argv0
  cat > tests/minimal-linux.S <<INNER_EOF
.global _start
.section .text
_start:
    mov \$59,%rax
    lea p(%rip), %rdi
    lea argv(%rip), %rsi
    lea envp(%rip), %rdx
    syscall
1:  jmp 1b
.section .rodata
p: .asciz "$1"
arg0: .asciz "$2"
env0: .asciz "SMP_EXPECT_PARALLEL=0"
env1: .asciz "SLOB_HELLO=/bin/exec-probe"
.align 8
argv: .quad arg0, 0
envp: .quad env0, env1, 0
# check-source token: /bin/hello
INNER_EOF
}
check_counters() {
  # $1 = log file. Strict A1 accounting.
  if grep -q "kernel exception" "$1"; then echo "smp: KERNEL EXCEPTION"; return 1; fi
  if grep -q "smp: CORRUPT" "$1"; then echo "smp: CORRUPT slot"; return 1; fi
  if grep -q "fatal:" "$1"; then echo "smp: fatal"; return 1; fi
  de=$(grep -o 'double_exec=[0-9]*' "$1" | tail -n 1 | cut -d= -f2)
  lo=$(grep -o 'lost_owner=[0-9]*' "$1" | tail -n 1 | cut -d= -f2)
  dd=$(grep -o 'push_pop_delta=[0-9]*' "$1" | tail -n 1 | cut -d= -f2)
  ra=$(grep -o 'audit runnable=[0-9]*' "$1" | tail -n 1 | cut -d= -f2)
  echo "smp: counters double_exec=$de lost_owner=$lo delta=$dd runnable=$ra"
  [ "${de:-9}" = "0" ] || return 1
  [ "${lo:-9}" = "0" ] || return 1
  [ "${dd:-9}" = "0" ] || return 1
  [ "${ra:-9}" = "0" ] || return 1
  return 0
}
OVMF_CODE=${OVMF_CODE:-/usr/share/OVMF/OVMF_CODE_4M.fd}
OVMF_VARS_TEMPLATE=${OVMF_VARS_TEMPLATE:-/usr/share/OVMF/OVMF_VARS_4M.fd}
pass=1
run_guest() {
  # $1 = test name (smp-stress|smp-exec), $2 = ncpu, $3 = iter
  t=$1; ncpu=$2; it=$3
  LOG="/tmp/opencode/smp-$t-$ncpu-i$it.log"
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
  echo "--- $t ncpu=$ncpu iter=$it qemu=$status ---"
  grep -E "SMP_OK|SMP_FAIL|EXEC_OK|EXEC_FAIL|double_exec|ap_user" "$LOG" | tail -n 8 || true
  if [ "$t" = "smp-stress" ]; then
    grep -q SMP_OK "$LOG" || { echo "smp: $t ${ncpu}cpu FAIL (no SMP_OK)"; return 1; }
    if [ "$ncpu" != "1" ]; then
      if [ "$ncpu" = "2" ]; then
        grep -q "smp: child [0-9]* cpu0=[1-9]" "$LOG" || { echo "smp: 2cpu no AP participation"; return 1; }
      else
        d=$(grep -o 'distinct cpus=[0-9]*' "$LOG" | tail -n 1 | cut -d= -f2)
        echo "smp: ${ncpu}cpu distinct=$d"
        [ "${d:-0}" -ge 2 ] || { echo "smp: ${ncpu}cpu no parallelism"; return 1; }
      fi
    fi
  else
    grep -q "EXEC_OK exec=160 fork=364 sock=32" "$LOG" || { echo "smp: $t ${ncpu}cpu FAIL (no EXEC_OK)"; return 1; }
  fi
  check_counters "$LOG" || { echo "smp: $t ${ncpu}cpu FAIL (counters)"; return 1; }
  return 0
}
for t in smp-stress smp-exec; do
  if [ "$t" = "smp-stress" ]; then
    write_stub "/bin/smp-stress" "smp-stress"
  else
    write_stub "/bin/smp-exec" "smp-exec"
  fi
  if ! /tmp/opencode/uefi-user.sh >build/smp-build.log 2>&1; then
    echo "smp: build failed" >&2
    tail -20 build/smp-build.log >&2
    exit 1
  fi
  for ncpu in $NCPUS; do
    i=1
    while [ "$i" -le "$ITER" ]; do
      run_guest "$t" "$ncpu" "$i" || pass=0
      i=$((i+1))
    done
  done
done
if [ "$pass" = "1" ]; then echo "smp-stress: PASS"; exit 0; else echo "smp-stress: FAIL" >&2; exit 1; fi
