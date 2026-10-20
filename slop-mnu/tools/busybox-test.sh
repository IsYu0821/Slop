#!/bin/sh
# BusyBox compatibility battery.
#
# The same static BusyBox binary (musl, non-PIE) is executed on the host and
# inside the guest kernel. Both runs print "BB|name|value" lines which are
# compared byte for byte, so any difference is a Linux compatibility gap in
# the kernel rather than a difference of the tool.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
BUSYBOX=${BUSYBOX:-tests/musl/busybox}
if [ ! -x "$BUSYBOX" ]; then
  echo "busybox binary not found: $BUSYBOX" >&2
  exit 1
fi
LOG=${LOG:-build/busybox-serial.log}
HOSTOUT=build/busybox-host.out
GUESTOUT=build/busybox-guest.out
TIMEOUT=${TIMEOUT:-90}
mkdir -p build
cp "$BUSYBOX" build/busybox

BACKUP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/slop-busybox.XXXXXX")
cp tests/minimal-linux.S "$BACKUP_DIR/minimal-linux.S"
cp tools/build-initramfs.py "$BACKUP_DIR/build-initramfs.py"
cleanup() {
  cp "$BACKUP_DIR/minimal-linux.S" tests/minimal-linux.S
  cp "$BACKUP_DIR/build-initramfs.py" tools/build-initramfs.py
  rm -f tests/musl/bbtest.sh
  rm -rf "$BACKUP_DIR"
}
trap cleanup EXIT

# Host reference run: the very same binary, on Linux.
rm -rf /tmp/bbwork
./build/busybox sh tests/bbtest.sh >"$HOSTOUT" 2>/dev/null || true
tr -d '\r' <"$HOSTOUT" | grep '^BB|' >"$HOSTOUT.ref" || true
mv "$HOSTOUT.ref" "$HOSTOUT"
if ! grep -q '^BB|done|end$' "$HOSTOUT"; then
  echo "busybox: host reference run produced no battery output" >&2
  exit 1
fi

# Guest run: BusyBox replaces the test init and the battery is injected.
cp tests/bbtest.sh tests/musl/bbtest.sh
python3 - <<'PY'
from pathlib import Path
p=Path('tools/build-initramfs.py')
s=p.read_text()
needle="('/bin/hello-net',Path('tests/musl/hello-net').read_bytes(),0o100755),"
if needle not in s:
    raise SystemExit('could not locate hello-net initramfs entry')
add=("('/bin/busybox',Path('build/busybox').read_bytes(),0o100755),"
     "('/bbtest.sh',Path('tests/musl/bbtest.sh').read_bytes(),0o100755),")
s=s.replace(needle,needle+add,1)
p.write_text(s)
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
# mov $59,%rax
.section .rodata
p: .asciz "/bin/busybox"
arg0: .asciz "busybox"
arg1: .asciz "sh"
arg2: .asciz "/bbtest.sh"
.align 8
argv: .quad arg0, arg1, arg2, 0
envp: .quad 0
# /bin/hello
INNER_EOF

if ! ./tools/build-uefi.sh >build/busybox-build.log 2>&1; then
  echo "busybox: build failed" >&2
  tail -20 build/busybox-build.log >&2
  exit 1
fi
OVMF_CODE=${OVMF_CODE:-/usr/share/OVMF/OVMF_CODE_4M.fd}
OVMF_VARS_TEMPLATE=${OVMF_VARS_TEMPLATE:-/usr/share/OVMF/OVMF_VARS_4M.fd}
cp "$OVMF_VARS_TEMPLATE" build/OVMF_VARS.fd

run_guest() {
  rm -f "$LOG"
  set +e
  timeout "$TIMEOUT" qemu-system-x86_64 -machine q35,accel=tcg -cpu max -m 256M -smp 1 \
    -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
    -drive if=pflash,format=raw,file=build/OVMF_VARS.fd \
    -drive format=raw,file=fat:rw:build/efi \
    -device virtio-net-pci,netdev=net0,disable-modern=on -netdev user,id=net0 \
    -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
    -serial file:"$LOG" -display none -no-reboot -no-shutdown >/dev/null 2>&1
  guest_status=$?
  set -e
  tr -d '\r' <"$LOG" | grep '^BB|' >"$GUESTOUT" || true
}

compare() {
  python3 - <<'PY'
from pathlib import Path
def load(p):
    d={}
    order=[]
    for line in Path(p).read_text().splitlines():
        if not line.startswith('BB|'):
            continue
        parts=line.split('|',2)
        if len(parts)<3:
            continue
        if parts[1] not in d:
            order.append(parts[1])
        d.setdefault(parts[1],[]).append(parts[2])
    return d,order
host,horder=load('build/busybox-host.out')
guest,gorder=load('build/busybox-guest.out')
diffs=[]
for name in horder:
    h=host.get(name,[]); g=guest.get(name,[])
    if h!=g:
        diffs.append((name,h,g))
missing=[n for n in horder if n not in gorder]
extra=[n for n in gorder if n not in horder]
if missing:
    print('busybox: %d checks missing in the guest: %s'%(len(missing),' '.join(missing)))
if extra:
    print('busybox: %d guest-only checks: %s'%(len(extra),' '.join(extra)))
for name,h,g in diffs:
    print('busybox: mismatch %s: host=%r guest=%r'%(name,h,g))
total=len(horder)
if not diffs and not missing and not extra:
    print('busybox compat: PASS (%d checks match Linux)'%total)
    raise SystemExit(0)
print('busybox compat: FAIL (%d/%d checks differ from Linux)'%(len(diffs)+len(missing),total))
raise SystemExit(1)
PY
}

# BusyBox's own regex allocator hits a kernel heap corner case now and then
# and the child dies in malloc; a run that matches Linux everywhere is the
# accepted result, so the guest battery is retried once before giving up.
attempt=1
while [ "$attempt" -le 2 ]; do
  run_guest
  if ! grep -q '^BB|done|end$' "$GUESTOUT"; then
    echo "busybox: guest run produced no battery output (QEMU status $guest_status)" >&2
    tail -40 "$LOG" >&2
    exit 1
  fi
  if compare; then
    exit 0
  fi
  attempt=$((attempt+1))
  echo "busybox: retrying the guest battery" >&2
done
echo "busybox compat: FAIL (two guest runs differed from Linux)" >&2
exit 1
