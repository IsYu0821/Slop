#!/bin/sh
# Minimal CPU-time accounting probe (no block I/O): boots PROFILE with
# /init -> /bin/cputime-mini, checks CPUTIME_OK + guest exit 33.
# Knobs: PROFILE (default B), SMP, ACCEL, MEM (passed to qemu-profile.sh).
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
MUSL=${MUSL_GCC:-/tmp/opencode/musl-install/bin/musl-gcc}
test -x "$MUSL" || { echo "musl-gcc not found"; exit 1; }
PROFILE=${PROFILE:-B}
BACKUP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/slop-cputime.XXXXXX")
cp tests/minimal-linux.S "$BACKUP_DIR/minimal-linux.S"
cp tools/build-initramfs.py "$BACKUP_DIR/build-initramfs.py"
cleanup() {
  cp "$BACKUP_DIR/minimal-linux.S" tests/minimal-linux.S
  cp "$BACKUP_DIR/build-initramfs.py" tools/build-initramfs.py
  rm -f tests/musl/cputime-mini
  rm -rf "$BACKUP_DIR"
}
trap cleanup EXIT
"$MUSL" -static -O1 -o build/cputime-mini tests/cputime-mini.c
PIN=${PIN:-}
if [ -n "$PIN" ]; then
  ARGV2="arg1: .asciz \"$PIN\""
  ARGVQ="argv: .quad arg0, arg1, 0"
else
  ARGV2=""
  ARGVQ="argv: .quad arg0, 0"
fi
cat > tests/minimal-linux.S <<INNER_EOF
.global _start
.section .text
_start:
    mov \$59, %rax
    lea p(%rip), %rdi
    lea argv(%rip), %rsi
    lea envp(%rip), %rdx
    syscall
1:  jmp 1b
# mov \$59,%rax
.section .rodata
p: .asciz "/bin/cputime-mini"
arg0: .asciz "cputime-mini"
$ARGV2
.align 8
$ARGVQ
envp: .quad 0
# /bin/hello
INNER_EOF
cp build/cputime-mini tests/musl/cputime-mini
python3 - <<'PY'
from pathlib import Path
p=Path('tools/build-initramfs.py')
s=p.read_text()
needle="('/bin/hello-net',Path('tests/musl/hello-net').read_bytes(),0o100755),"
if needle not in s:
    raise SystemExit('could not locate hello-net initramfs entry')
s=s.replace(needle,needle+"('/bin/cputime-mini',Path('tests/musl/cputime-mini').read_bytes(),0o100755),",1)
p.write_text(s)
PY
./tools/build-uefi.sh >/tmp/scratchpad/cputime-build.log 2>&1
SERLOG=build/cputime-serial.log
rm -f "$SERLOG"
if SERIAL_LOG="$SERLOG" timeout 300 ./tools/qemu-profile.sh "$PROFILE" >/dev/null 2>&1; then
  guest_status=0
else
  guest_status=$?
fi
grep -a "CPUTIME" "$SERLOG" | head -5 || true
if ! grep -q "CPUTIME_OK" "$SERLOG"; then
  echo "cputime probe failed (qemu status $guest_status, expected 33)"
  exit 1
fi
if [ "$guest_status" -ne 33 ]; then
  echo "guest QEMU status $guest_status (expected 33)"
  exit 1
fi
echo "cputime test: PASS (profile $PROFILE)"
