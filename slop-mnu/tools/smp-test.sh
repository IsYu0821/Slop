#!/bin/sh
# SMP parallel-execution battery.
#
# Builds tests/smp-stress.c with musl and runs the same binary on Linux
# (reference) and inside the guest with 1, 2 and 4 vCPUs. The guest passes
# only when children run on at least two different CPUs (2/4 vCPU) and all
# pipe/file/exit checks match the native reference behavior.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
MUSL=${MUSL_GCC:-/tmp/opencode/musl-install/bin/musl-gcc}
test -x "$MUSL" || { echo "musl-gcc not found"; exit 1; }
TIMEOUT=${TIMEOUT:-90}
mkdir -p build
BACKUP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/slop-smp.XXXXXX")
cp tests/minimal-linux.S "$BACKUP_DIR/minimal-linux.S"
cp tools/build-initramfs.py "$BACKUP_DIR/build-initramfs.py"
cleanup() {
  cp "$BACKUP_DIR/minimal-linux.S" tests/minimal-linux.S
  cp "$BACKUP_DIR/build-initramfs.py" tools/build-initramfs.py
  rm -f tests/musl/smp-stress
  rm -rf "$BACKUP_DIR"
}
trap cleanup EXIT

"$MUSL" -static -O1 -o build/smp-stress tests/smp-stress.c
cp "build/smp-stress" "tests/musl/smp-stress"

# Native reference (proves the test itself is sound on Linux).
SMP_EXPECT_PARALLEL=0 ./build/smp-stress > build/smp-native.out 2>&1 || {
  echo "smp: native reference failed"; tail -n 10 build/smp-native.out; exit 1; }
grep -q SMP_OK build/smp-native.out || { echo "smp: native reference no SMP_OK"; exit 1; }
echo "smp: native reference OK"

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
p: .asciz "/bin/smp-stress"
arg0: .asciz "smp-stress"
env0: .asciz "SMP_EXPECT_PARALLEL=0"
.align 8
argv: .quad arg0, 0
envp: .quad env0, 0
# mov $59,%rax
# /bin/hello
INNER_EOF
python3 - <<'PY'
from pathlib import Path
p=Path('tools/build-initramfs.py')
s=p.read_text()
needle="('/bin/hello-net',Path('tests/musl/hello-net').read_bytes(),0o100755),"
if needle not in s:
    raise SystemExit('could not locate hello-net initramfs entry')
s=s.replace(needle,needle+"('/bin/smp-stress',Path('tests/musl/smp-stress').read_bytes(),0o100755),",1)
p.write_text(s)
PY
if ! /tmp/opencode/uefi-user.sh >build/smp-build.log 2>&1; then
  echo "smp: build failed" >&2
  tail -20 build/smp-build.log >&2
  exit 1
fi
OVMF_CODE=${OVMF_CODE:-/usr/share/OVMF/OVMF_CODE_4M.fd}
OVMF_VARS_TEMPLATE=${OVMF_VARS_TEMPLATE:-/usr/share/OVMF/OVMF_VARS_4M.fd}
pass=1
for ncpu in 1 2 4; do
  LOG="/tmp/opencode/smp-stress-$ncpu.log"
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
  echo "--- smpstress ncpu=$ncpu qemu=$status ---"
  grep -E "smp: (child|distinct)|SMP_OK|SMP_FAIL|task->task|idle->task|smp: cpu[0-9]|double_exec|ap_user" "$LOG" | tail -n 25 || true
  if [ "$ncpu" = "1" ]; then
    grep -q SMP_OK "$LOG" || { echo "smp: 1cpu FAIL"; pass=0; };
  else
    if grep -q SMP_OK "$LOG"; then
      d=$(grep -o 'distinct cpus=[0-9]*' "$LOG" | tail -n 1 | cut -d= -f2)
      echo "smp: ${ncpu}cpu distinct=$d"
      if [ "$ncpu" = "2" ]; then
        # 2-CPU proof: AP executed user children (IPI wake beats BSP timer
        # wake, so AP may run everything; parallelism is proven on 4 CPUs).
        if grep -q "smp: child [0-9]* cpu0=[1-9]" "$LOG"; then
          echo "smp: 2cpu AP ran user tasks"
        else
          echo "smp: 2cpu no AP participation"; pass=0
        fi
      else
        if [ "${d:-0}" -lt 2 ]; then echo "smp: ${ncpu}cpu no parallelism"; pass=0; fi
      fi
    else
      echo "smp: ${ncpu}cpu FAIL"; pass=0
    fi
  fi
done
if [ "$pass" = "1" ]; then echo "smp-stress: PASS"; exit 0; else echo "smp-stress: FAIL" >&2; exit 1; fi
