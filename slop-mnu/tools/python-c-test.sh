#!/bin/sh
# Python smoke: unmodified host python3.12 binary + libs in initramfs.
# Phase 2: -c print + file write. Minimal stdlib (encodings package) is staged
# into the initramfs at its canonical /usr/lib/python3.12 path; the rest of
# the core (abc/io/os/...) is frozen into the binary, so no full stdlib needed.
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
# Minimal stdlib for -c: only the 5 encodings files CPython needs at startup
# (initramfs has a 64-entry kernel ceiling; full stdlib lives on ext4 instead).
mkdir -p tests/glibc/stdlib/encodings
for f in __init__ aliases utf_8 ascii latin_1; do cp "/usr/lib/python3.12/encodings/$f.py" tests/glibc/stdlib/encodings/; done
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
arg1: .asciz "-c"
arg2: .asciz "print('PYC_OK'); open('/tmp/pyc-out.txt','w').write('hi from python'); print('PYC_FILE_OK')"
.align 8
argv: .quad arg0, arg1, arg2, 0
envp: .quad 0
# /bin/hello
INNER_EOF
python3 - <<'PY'
from pathlib import Path
import os
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
 # initramfs lookup is a flat exact-path match: parent dir nodes are required
 # for open()/listdir() of the stdlib tree (readdir itself is synthesized).
 "'/usr'": "('/usr', b'', 0o040755),",
 "'/usr/lib'": "('/usr/lib', b'', 0o040755),",
 "'/usr/lib/python3.12'": "('/usr/lib/python3.12', b'', 0o040755),",
 "'/usr/lib/python3.12/encodings'": "('/usr/lib/python3.12/encodings', b'', 0o040755),",
}
for fn in sorted(os.listdir('tests/glibc/stdlib/encodings')):
    if not fn.endswith('.py'):
        continue
    key="'/usr/lib/python3.12/encodings/%s'" % fn
    entries[key]="('%s',Path('tests/glibc/stdlib/encodings/%s').read_bytes(),0o644)," % (key.strip("'"), fn)
add="".join(v for k,v in entries.items() if k not in s)
s=s.replace(needle,needle+add,1)
p.write_text(s)
print("injected entries:", sum(1 for k in entries if k in s))
PY
./tools/build-uefi.sh >/dev/null 2>&1
SERLOG=build/python-c-serial.log
rm -f "$SERLOG"
SERIAL_LOG="$SERLOG" timeout 240 ./tools/qemu-profile.sh B >/dev/null 2>&1 || true
grep -a "PYC_OK" "$SERLOG" | head -3 || true
grep -q "PYC_FILE_OK" "$SERLOG" && echo "python-c test: PASS (profile B)" || { echo "python test: FAIL"; tail -n 12 "$SERLOG"; exit 1; }
