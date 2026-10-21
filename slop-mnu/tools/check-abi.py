#!/usr/bin/env python3
"""Check the Linux ABI layouts that the kernel publishes.

The script reads the kernel structures and the libc header and compares the
computed offsets with the published Linux x86-64 layouts. It needs no
compiler, so a layout drift is caught before a build.

Provenance for every expected value is recorded in docs/compat-ledger.md.
"""
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent

SIZES = {
    'u8': 1, 'i8': 1, 'u16': 2, 'i16': 2, 'u32': 4, 'i32': 4, 'u64': 8, 'i64': 8,
    'usize': 8, 'isize': 8, 'bool': 1,
}


class Types:
    """Resolves the primitive and the named structure sizes."""

    def __init__(self, structs):
        self.structs = structs
        self.sizes = {}
        self.aligns = {}

    def size_of(self, rust_type):
        rust_type = rust_type.strip()
        if rust_type in self.sizes:
            return self.sizes[rust_type]
        array = re.fullmatch(r'\[([^\];]+);\s*(\d+)\]', rust_type)
        if array:
            value = self.size_of(array.group(1)) * int(array.group(2))
            self.sizes[rust_type] = value
            self.aligns[rust_type] = self.align_of(array.group(1))
            return value
        if rust_type in SIZES:
            return SIZES[rust_type]
        if rust_type in self.structs:
            size, align = self.layout(rust_type)
            return size
        raise SystemExit('unknown type: %s' % rust_type)

    def align_of(self, rust_type):
        rust_type = rust_type.strip()
        if rust_type in self.aligns:
            return self.aligns[rust_type]
        array = re.fullmatch(r'\[([^\];]+);\s*(\d+)\]', rust_type)
        if array:
            return self.align_of(array.group(1))
        if rust_type in SIZES:
            return min(SIZES[rust_type], 8)
        if rust_type in self.structs:
            return self.layout(rust_type)[1]
        raise SystemExit('unknown type: %s' % rust_type)

    def layout(self, name):
        offset = 0
        offsets = {}
        for field, rust_type in self.structs[name]:
            align = self.align_of(rust_type)
            size = self.size_of(rust_type)
            offset = (offset + align - 1) & ~(align - 1)
            offsets[field] = offset
            offset += size
        size = (offset + 7) & ~7
        self.sizes[name] = size
        self.aligns[name] = min(self.align_of_of_field(name), 8)
        return offsets, size

    def align_of_of_field(self, name):
        align = 1
        for _, rust_type in self.structs[name]:
            align = max(align, self.align_of(rust_type))
        return align


def rust_structs(text):
    """Collects the plain field lists of the repr(C) structures."""
    found = {}
    for match in re.finditer(r'pub struct (\w+)\s*\{([^}]*)\}', text):
        name = match.group(1)
        fields = []
        for raw in match.group(2).split(','):
            raw = raw.strip()
            if not raw or raw.startswith('/*') or raw.startswith('#'):
                continue
            field = re.fullmatch(r'(?:pub )?(\w+)\s*:\s*([\w\[\]; ]+)', raw)
            if not field:
                continue
            fields.append((field.group(1), field.group(2).strip()))
        found[name] = fields
    return found


def main():
    kernel = (ROOT / 'kernel' / 'signal.rs').read_text(encoding='utf-8')
    header = (ROOT / 'libc' / 'include' / 'signal.h').read_text(encoding='utf-8')
    types = Types(rust_structs(kernel))
    failed = 0

    def expect(label, actual, expected):
        nonlocal failed
        if actual != expected:
            print('ABI MISMATCH %s: got %s, expected %s' % (label, actual, expected))
            failed += 1

    # struct k_sigaction: handler, flags, mask, restorer.
    offsets, size = types.layout('Action')
    expect('k_sigaction.handler', offsets['handler'], 0)
    expect('k_sigaction.flags', offsets['flags'], 8)
    expect('k_sigaction.mask', offsets['mask'], 16)
    expect('k_sigaction.restorer', offsets['restorer'], 24)
    expect('k_sigaction.size', size, 32)

    # struct sigcontext: the register order named by the REG_* index list.
    expected_context = {
        'r8': 0, 'r9': 8, 'r10': 16, 'r11': 24, 'r12': 32, 'r13': 40,
        'r14': 48, 'r15': 56, 'rdi': 64, 'rsi': 72, 'rbp': 80, 'rbx': 88,
        'rdx': 96, 'rax': 104, 'rcx': 112, 'rsp': 120, 'rip': 128,
        'eflags': 136, 'cs': 144, 'gs': 146, 'fs': 148, 'ss': 150,
        'err': 152, 'trapno': 160, 'oldmask': 168, 'cr2': 176,
        'fpstate': 184, 'reserved1': 192,
    }
    offsets, size = types.layout('SigContext')
    for name, expected in expected_context.items():
        expect('sigcontext.%s' % name, offsets[name], expected)
    expect('sigcontext.size', size, 256)

    # stack_t and the userspace ucontext_t.
    offsets, size = types.layout('StackArea')
    expect('stack_t.ss_sp', offsets['ss_sp'], 0)
    expect('stack_t.ss_flags', offsets['ss_flags'], 8)
    expect('stack_t.ss_size', offsets['ss_size'], 16)
    expect('stack_t.size', size, 24)

    offsets, size = types.layout('UContext')
    expect('ucontext.uc_flags', offsets['uc_flags'], 0)
    expect('ucontext.uc_link', offsets['uc_link'], 8)
    expect('ucontext.uc_stack', offsets['uc_stack'], 16)
    expect('ucontext.uc_mcontext', offsets['uc_mcontext'], 40)
    expect('ucontext.uc_sigmask', offsets['uc_sigmask'], 296)
    expect('ucontext.uc_fpstate', offsets['uc_fpstate'], 304)
    expect('ucontext.size', size, 816)

    # siginfo_t: the header fields and the _kill member inside the union.
    # The union starts right after the three header fields, so the _kill
    # member begins at offset 12.
    offsets, size = types.layout('SigInfo')
    expect('siginfo.si_signo', offsets['si_signo'], 0)
    expect('siginfo.si_errno', offsets['si_errno'], 4)
    expect('siginfo.si_code', offsets['si_code'], 8)
    expect('siginfo.si_pid', offsets['si_pid'], 12)
    expect('siginfo.si_uid', offsets['si_uid'], 16)
    expect('siginfo.size', size, 128)

    # The signal frame: the restorer, then the ucontext, then the siginfo.
    expect('frame.ucontext', 8, 8)
    expect('frame.siginfo', 8 + types.size_of('UContext'), 8 + 816)

    # The published REG_* index list has to match the register order above.
    registers = ['R8', 'R9', 'R10', 'R11', 'R12', 'R13', 'R14', 'R15',
                 'RDI', 'RSI', 'RBP', 'RBX', 'RDX', 'RAX', 'RCX', 'RSP',
                 'RIP', 'EFL', 'CSGSFS', 'ERR', 'TRAPNO', 'OLDMASK', 'CR2']
    for index, name in enumerate(registers):
        if ('#define REG_%s %d' % (name, index)) not in header:
            print('ABI MISMATCH libc signal.h: REG_%s is missing or has another index' % name)
            failed += 1

    # The libc header has to present the same layouts to the programs.
    ordered = [
        ('ucontext_t', ['uc_flags', 'uc_link', 'uc_stack', 'uc_mcontext',
                        'uc_sigmask', '__fpregs_mem']),
        ('siginfo_t', ['si_signo', 'si_errno', 'si_code', '_sifields']),
        ('stack_t', ['ss_sp', 'ss_flags', 'ss_size']),
    ]
    for name, fields in ordered:
        position = -1
        for field in fields:
            found = header.find(field, position + 1)
            if found < 0:
                print('ABI MISMATCH libc signal.h: %s misses %s' % (name, field))
                failed += 1
                continue
            position = found

    if failed:
        print('ABI layout checks FAILED (%d problems)' % failed)
        return 1
    print('ABI layout checks passed')
    return 0


if __name__ == '__main__':
    sys.exit(main())
