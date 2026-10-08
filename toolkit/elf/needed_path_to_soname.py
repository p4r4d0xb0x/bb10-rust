#!/usr/bin/env python3
"""Rewrite DT_NEEDED strings that carry a build-host path to bare SONAMEs.

rust-lld records each command-line shared library under the path given to it
(or its SONAME). QNX's loader searches LD_LIBRARY_PATH by name, so
'/Volumes/.../libSDL12.so' must become 'libSDL12.so'. In-place shortening is
safe (null-padded tail); a later entry ordering never depends on length.
"""
import struct, sys
from pathlib import Path

path = Path(sys.argv[1])
wanted = set(sys.argv[2:])  # sonames we expect to (re)write; empty = all dotty
d = bytearray(path.read_bytes())
assert d[:4] == b'\x7fELF' and d[4] == 1, 'not ELF32'
e_phoff, = struct.unpack_from('<I', d, 0x1C)
e_phentsize, e_phnum = struct.unpack_from('<HH', d, 0x2A)
segs, dyn = [], None
for i in range(e_phnum):
    off = e_phoff + i * e_phentsize
    pt, = struct.unpack_from('<I', d, off)
    if pt == 1:
        poff, pvaddr, _, pfilesz = struct.unpack_from('<IIII', d, off + 4)
        segs.append((pvaddr, poff, pfilesz))
    elif pt == 2:
        dyn = (struct.unpack_from('<I', d, off + 4)[0],
               struct.unpack_from('<I', d, off + 16)[0])
assert dyn, 'no PT_DYNAMIC'

def v2o(v):
    for pa, po, fs in segs:
        if pa <= v < pa + fs:
            return v - pa + po
    raise ValueError(f'vaddr {v:#x} unmapped')

doff, dsz = dyn
i = 0
strtab = None
needs = []
while i + 8 <= dsz:
    t, v = struct.unpack_from('<II', d, doff + i)
    if t == 0:
        break
    if t == 5:
        strtab = v
    if t == 1:
        needs.append(v)
    i += 8
assert strtab, 'no DT_STRTAB'
sto = v2o(strtab)
fixed = []
for rel in needs:
    end = d.index(b'\x00', sto + rel)
    s = d[sto + rel:end].decode()
    if '/' in s and (not wanted or Path(s).name in wanted):
        new = Path(s).name.encode() + b'\x00'
        assert len(new) <= end - (sto + rel) + 1, 'cannot shorten in place'
        d[sto + rel:sto + rel + len(new)] = new
        d[sto + rel + len(new):end + 1] = b'\x00' * (end - (sto + rel + len(new)) + 1)
        fixed.append(f'{s} -> {Path(s).name}')
path.write_bytes(bytes(d))
print('needed_path_to_soname:', '; '.join(fixed) if fixed else 'no path-form NEEDED found')
