#!/usr/bin/env python3
from pathlib import Path
import struct
out=bytearray(b'NSAR0001')
musl_lib=Path('tests/musl/lib/libc.so').read_bytes()
entries=[('/bin',b'',0o040755),('/etc',b'',0o040755),('/lib',b'',0o040755),('/init',Path('build/minimal-linux.elf').read_bytes(),0o100755),('/lib/libc.so',musl_lib,0o100755),('/lib/ld-musl-x86_64.so.1',musl_lib,0o100755),('/bin/hello-dyn',Path('tests/musl/hello-dyn').read_bytes(),0o100755),('/bin/hello-thread',Path('tests/musl/hello-thread').read_bytes(),0o100755),('/bin/hello',Path('build/hello-linux.elf').read_bytes(),0o100755),('/bin/hello-c',Path('build/hello-c.elf').read_bytes(),0o100755),('/bin/hello-posix',Path('build/hello-posix.elf').read_bytes(),0o100755),('/bin/hello-signal',Path('build/hello-signal.elf').read_bytes(),0o100755),('/bin/hello-file',Path('build/hello-file.elf').read_bytes(),0o100755),('/bin/hello-syscall',Path('build/hello-syscall.elf').read_bytes(),0o100755),('/etc/motd',b'Slop/MNU initramfs\n',0o100444)]
out += struct.pack('<I',len(entries))
for name,data,mode in entries:
    b=name.encode(); out += struct.pack('<HHI',len(b),mode,len(data)); out += b; out += bytes((-len(out))&7); out += data; out += bytes((-len(out))&7)
Path('build/initramfs.nsar').write_bytes(out)

def coff_rdata(data,symbols):
    # Minimal COFF-x86-64 relocatable object with one .rdata section.
    dataoff=64
    symoff=(dataoff+len(data)+3)&~3
    strtab=bytearray(struct.pack('<I',0))
    names=[]
    for name,_ in symbols:
        b=name.encode()
        if len(b)<=8:
            names.append((b.ljust(8,b'\0'),None))
        else:
            names.append((b'\0\0\0\0'+struct.pack('<I',len(strtab)),len(strtab)))
            strtab+=b+b'\0'
    struct.pack_into('<I',strtab,0,len(strtab))
    header=struct.pack('<HHIIIHH',0x8664,1,0,symoff,len(symbols),0,0)
    section=struct.pack('<8sIIIIIIHHI',b'.rdata',0,0,len(data),dataoff,0,0,0,0,0x40500040)
    obj=bytearray(header+section)
    obj+=bytes(dataoff-len(obj))+data
    obj+=bytes(symoff-len(obj))
    for (nm,_),(_,value) in zip(names,symbols):
        obj+=struct.pack('<8sIHHBB',nm,value,1,0,2,0)
    obj+=strtab
    return bytes(obj)

blob=bytes(out)
syms=[('_binary_build_initramfs_nsar_start',0),('_binary_build_initramfs_nsar_end',len(blob)),('_binary_build_initramfs_nsar_size',len(blob))]
Path('build/initramfs.obj').write_bytes(coff_rdata(blob,syms))
print(f'created build/initramfs.nsar with {len(entries)} entries, {len(out)} bytes')
print(f'created build/initramfs.obj, {len(blob)} bytes of .rdata')
