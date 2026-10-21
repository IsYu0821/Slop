#!/usr/bin/env python3
"""Regenerate SOURCE_SNAPSHOT.txt from the working tree.

The snapshot records the size and the SHA-256 digest of every source file and
embeds the file contents, so that a release can be reconstructed from a single
archive. The layout is the one the earlier snapshots use, so the file can be
compared with them directly.
"""
from pathlib import Path
import hashlib

ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / 'SOURCE_SNAPSHOT.txt'
SKIP_DIRS = {'.git', 'build', 'target'}
SKIP_FILES = {'SOURCE_SNAPSHOT.txt'}
SEPARATOR = '=' * 80
RULE = '-' * 80


def version() -> str:
    for line in (ROOT / 'Cargo.toml').read_text(encoding='utf-8').splitlines():
        if line.startswith('version = '):
            return line.split('"')[1]
    raise SystemExit('Cargo.toml carries no version field')


def collect() -> list:
    files = []
    for path in ROOT.rglob('*'):
        if not path.is_file():
            continue
        rel = path.relative_to(ROOT)
        if rel.parts[0] in SKIP_DIRS:
            continue
        if rel.as_posix() in SKIP_FILES:
            continue
        try:
            (ROOT / rel.as_posix()).read_text(encoding='utf-8')
        except (UnicodeDecodeError, ValueError):
            # Binary artefacts (test programs, libraries) are not sources.
            continue
        files.append(rel.as_posix())
    return sorted(files)


def read_source(rel: str) -> bytes:
    """Reads a source file with the line endings of the repository.

    The repository stores the sources with Unix line endings, so the snapshot
    is taken over the normalized content. That keeps the digests independent
    of the line ending convention of the machine that runs the tool.
    """
    text = (ROOT / rel).read_text(encoding='utf-8')
    return text.replace('\r\n', '\n').encode('utf-8')


def main() -> None:
    files = collect()
    lines = []
    lines.append('SLOP/MNU - COMPLETE SOURCE SNAPSHOT')
    lines.append('Version: %s' % version())
    lines.append('SLOP: Synthetic Low-quality Operating Platform')
    lines.append('MNU: Maybe Not Unix')
    lines.append('Encoding: UTF-8')
    lines.append('')
    lines.append('FILE MANIFEST')
    for rel in files:
        raw = read_source(rel)
        lines.append('%s%10d  %s' % (hashlib.sha256(raw).hexdigest(), len(raw), rel))
    lines.append('')
    lines.append('TOTAL FILES: %d' % len(files))
    lines.append('')
    for rel in files:
        raw = read_source(rel)
        text = raw.decode('utf-8')
        if text.endswith('\n'):
            text = text[:-1]
        lines.append(SEPARATOR)
        lines.append('FILE: %s' % rel)
        lines.append('SIZE: %d bytes' % len(raw))
        lines.append('SHA-256: %s' % hashlib.sha256(raw).hexdigest())
        lines.append(SEPARATOR)
        lines.append(text)
        lines.append(RULE)
        lines.append('END FILE: %s' % rel)
        lines.append('')
    with OUTPUT.open('w', encoding='utf-8', newline='\n') as handle:
        handle.write('\n'.join(lines) + '\n')
    print('created %s with %d files' % (OUTPUT.name, len(files)))


if __name__ == '__main__':
    main()
