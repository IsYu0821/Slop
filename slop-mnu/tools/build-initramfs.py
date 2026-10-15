#!/usr/bin/env python3
from pathlib import Path
import struct
out=bytearray(b'NSAR0001')
entries=[('/init',Path('build/minimal-linux.elf').read_bytes(),0o100755),('/bin/hello',Path('build/hello-linux.elf').read_bytes(),0o100755),('/etc/motd',b'Slop/MNU initramfs\n',0o100444)]
out += struct.pack('<I',len(entries))
for name,data,mode in entries:
    b=name.encode(); out += struct.pack('<HHI',len(b),mode,len(data)); out += b; out += bytes((-len(out))&7); out += data; out += bytes((-len(out))&7)
Path('build/initramfs.nsar').write_bytes(out)
print(f'created build/initramfs.nsar with {len(entries)} entries, {len(out)} bytes')
