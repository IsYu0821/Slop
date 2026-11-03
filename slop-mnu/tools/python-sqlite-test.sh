#!/bin/sh
# Python sqlite3 module on ext4: extension load plus DB create/insert/query.
# Requires the stat_ino file-mmap fix (dlopen of lib-dynload extensions).
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
MUSL=${MUSL_GCC:-/usr/bin/musl-gcc}
"$MUSL" -static -O1 -o build/pyrun tests/pyrun.c
cp build/pyrun tests/musl/pyrun
mkdir -p tests/glibc/lib64 tests/glibc/lib-x86_64
cp -L /usr/bin/python3.12 tests/glibc/python3.12
cp -L /lib64/ld-linux-x86-64.so.2 tests/glibc/lib64/
cp /lib/x86_64-linux-gnu/libc.so.6 tests/glibc/lib-x86_64/
cp -L /lib/x86_64-linux-gnu/libm.so.6 tests/glibc/ 2>/dev/null || true
cp -L /lib/x86_64-linux-gnu/libz.so.1 tests/glibc/ 2>/dev/null || true
cp -L /lib/x86_64-linux-gnu/libexpat.so.1 tests/glibc/ 2>/dev/null || true
cp -L /lib/x86_64-linux-gnu/libsqlite3.so.0 tests/glibc/ 2>/dev/null || true
BACKUP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/slop-py.XXXXXX")
cp tests/minimal-linux.S "$BACKUP_DIR/minimal-linux.S"
cp tools/build-initramfs.py "$BACKUP_DIR/build-initramfs.py"
cp build/ext2.img "$BACKUP_DIR/ext2.img"
cleanup() {
  cp "$BACKUP_DIR/minimal-linux.S" tests/minimal-linux.S
  cp "$BACKUP_DIR/build-initramfs.py" tools/build-initramfs.py
  cp "$BACKUP_DIR/ext2.img" build/ext2.img
  rm -rf "$BACKUP_DIR"
}
trap cleanup EXIT
SEED=/tmp/opencode/pysqlite-seed
IMG=/tmp/opencode/pysqlite.img
rm -rf "$SEED"
mkdir -p "$SEED/lib"
(cd /usr/lib/python3.12 && find . -type f ! -path "*/__pycache__/*") | while read f; do
  mkdir -p "$SEED/lib/$(dirname "$f")"
  cp "/usr/lib/python3.12/$f" "$SEED/lib/$f"
done
cat > "$SEED/test.py" <<'PYEOF'
import sqlite3
print("SQLMOD_OK", sqlite3.sqlite_version, flush=True)
con = sqlite3.connect("/mntpy/t.db")
cur = con.cursor()
cur.execute("CREATE TABLE t(a INTEGER, b TEXT)")
cur.executemany("INSERT INTO t VALUES(?,?)", [(i, "x%d" % i) for i in range(90)])
con.commit()
n = cur.execute("SELECT COUNT(*), SUM(a) FROM t").fetchone()
assert (n[0], n[1]) == (90, sum(range(90))), n
print("PYSQLITE_OK", n[0], n[1], flush=True)
con.close()
import os
assert os.path.getsize("/mntpy/t.db") > 0
print("PYSQLITE_FILE_OK", flush=True)
PYEOF
echo "seed files: $(find "$SEED" -type f | wc -l)"
rm -f "$IMG"
mke2fs -t ext4 -b 4096 -N 8192 -O extent,^64bit,^metadata_csum,^dir_index,^huge_file,^flex_bg,^uninit_bg -d "$SEED" -q "$IMG" 65536
cp "$IMG" build/ext2.img
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
p: .asciz "/bin/pyrun"
arg0: .asciz "pyrun"
.align 8
argv: .quad arg0, 0
envp: .quad 0
# /bin/hello
INNER_EOF
python3 - <<'PYEOF2'
from pathlib import Path
p=Path('tools/build-initramfs.py')
s=p.read_text()
needle="('/bin/hello-net',Path('tests/musl/hello-net').read_bytes(),0o100755),"
assert needle in s
entries={
 "'/bin/pyrun'": "('/bin/pyrun',Path('tests/musl/pyrun').read_bytes(),0o100755),",
 "'/bin/python3'": "('/bin/python3',Path('tests/glibc/python3.12').read_bytes(),0o100755),",
 "'/lib64/ld-linux-x86-64.so.2'": "('/lib64/ld-linux-x86-64.so.2',Path('tests/glibc/lib64/ld-linux-x86-64.so.2').read_bytes(),0o755),",
 "'/lib/x86_64-linux-gnu/libc.so.6'": "('/lib/x86_64-linux-gnu/libc.so.6',Path('tests/glibc/lib-x86_64/libc.so.6').read_bytes(),0o755),",
 "'/lib/x86_64-linux-gnu/libm.so.6'": "('/lib/x86_64-linux-gnu/libm.so.6',Path('tests/glibc/libm.so.6').read_bytes(),0o755),",
 "'/lib/x86_64-linux-gnu/libz.so.1'": "('/lib/x86_64-linux-gnu/libz.so.1',Path('tests/glibc/libz.so.1').read_bytes(),0o755),",
 "'/lib/x86_64-linux-gnu/libexpat.so.1'": "('/lib/x86_64-linux-gnu/libexpat.so.1',Path('tests/glibc/libexpat.so.1').read_bytes(),0o755),",
 "'/lib/x86_64-linux-gnu/libsqlite3.so.0'": "('/lib/x86_64-linux-gnu/libsqlite3.so.0',Path('tests/glibc/libsqlite3.so.0').read_bytes(),0o755),",
}
add="".join(v for k,v in entries.items() if k not in s)
s=s.replace(needle,needle+add,1)
p.write_text(s)
print("injected entries:", sum(1 for k in entries if k in s))
PYEOF2
./tools/build-uefi.sh >/dev/null 2>&1
SERLOG=build/pysqlite-serial.log
rm -f "$SERLOG"
set +e
SERIAL_LOG="$SERLOG" timeout 300 ./tools/qemu-profile.sh A >/dev/null 2>&1
status=$?
set -e
echo "qemu status $status"
grep -a "SQLMOD_OK\|PYSQLITE" "$SERLOG" | head -5 || true
grep -q "PYSQLITE_FILE_OK" "$SERLOG" && echo "python-sqlite test: PASS (profile A)" || { echo "python-sqlite test: FAIL"; tail -n 8 "$SERLOG"; exit 1; }
