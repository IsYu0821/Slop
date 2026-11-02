#!/bin/sh
# Python smoke: unmodified host python3.12 binary + libs in initramfs.
# Phase 1: --version only (minimal init, no stdlib needed).
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
BACKUP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/slop-py.XXXXXX")
cp tests/minimal-linux.S "$BACKUP_DIR/minimal-linux.S"
cp tools/build-initramfs.py "$BACKUP_DIR/build-initramfs.py"
cleanup() {
  cp "$BACKUP_DIR/minimal-linux.S" tests/minimal-linux.S
  cp "$BACKUP_DIR/build-initramfs.py" tools/build-initramfs.py
  rm -rf "$BACKUP_DIR"
}
trap cleanup EXIT
mkdir -p tests/glibc/lib64 tests/glibc/lib-x86_64
cp -L /usr/bin/python3.12 tests/glibc/python3.12 2>/dev/null || cp -L /usr/bin/python3 tests/glibc/python3.12
cp -L /lib64/ld-linux-x86-64.so.2 tests/glibc/lib64/ld-linux-x86-64.so.2
cp /lib/x86_64-linux-gnu/libc.so.6 tests/glibc/lib-x86_64/libc.so.6
cp -L /lib/x86_64-linux-gnu/libm.so.6 tests/glibc/libm.so.6
cp -L /lib/x86_64-linux-gnu/libz.so.1 tests/glibc/libz.so.1
cp -L /lib/x86_64-linux-gnu/libexpat.so.1 tests/glibc/libexpat.so.1 2>/dev/null || true
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
p: .asciz "/bin/python3"
arg0: .asciz "python3"
arg1: .asciz "--version"
.align 8
argv: .quad arg0, arg1, 0
envp: .quad 0
# /bin/hello
INNER_EOF
python3 - <<'PY'
from pathlib import Path
p=Path('tools/build-initramfs.py')
s=p.read_text()
needle="('/bin/hello-net',Path('tests/musl/hello-net').read_bytes(),0o100755),"
assert needle in s
entries={
 "'/bin/python3'": "('/bin/python3',Path('tests/glibc/python3.12').read_bytes(),0o100755),",
 "'/lib64/ld-linux-x86-64.so.2'": "('/lib64/ld-linux-x86-64.so.2',Path('tests/glibc/lib64/ld-linux-x86-64.so.2').read_bytes(),0o755),",
 "'/lib/x86_64-linux-gnu/libc.so.6'": "('/lib/x86_64-linux-gnu/libc.so.6',Path('tests/glibc/lib-x86_64/libc.so.6').read_bytes(),0o755),",
 "'/lib/x86_64-linux-gnu/libm.so.6'": "('/lib/x86_64-linux-gnu/libm.so.6',Path('tests/glibc/libm.so.6').read_bytes(),0o755),",
 "'/lib/x86_64-linux-gnu/libz.so.1'": "('/lib/x86_64-linux-gnu/libz.so.1',Path('tests/glibc/libz.so.1').read_bytes(),0o755),",
 "'/lib/x86_64-linux-gnu/libexpat.so.1'": "('/lib/x86_64-linux-gnu/libexpat.so.1',Path('tests/glibc/libexpat.so.1').read_bytes(),0o755),",
}
add="".join(v for k,v in entries.items() if k not in s)
s=s.replace(needle,needle+add,1)
p.write_text(s)
print("injected entries:", sum(1 for k in entries if k in s))
PY
./tools/build-uefi.sh >/dev/null 2>&1
SERLOG=build/python-serial.log
rm -f "$SERLOG"
SERIAL_LOG="$SERLOG" timeout 180 ./tools/qemu-profile.sh B >/dev/null 2>&1 || true
grep -a "Python 3" "$SERLOG" | head -3 || true
grep -q "Python 3" "$SERLOG" && echo "python test: PASS (profile B)" || { echo "python test: FAIL"; tail -n 12 "$SERLOG"; exit 1; }
