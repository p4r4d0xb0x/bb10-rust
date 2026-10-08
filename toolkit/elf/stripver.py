#!/usr/bin/env python3
"""Strip DT_VERNEED/DT_VERNEEDNUM/DT_VERSYM from ELF32 .dynamic (QNX ldqnx rejects versioned deps)."""
import struct, sys

path = sys.argv[1]
data = bytearray(open(path, 'rb').read())
assert data[:4] == b'\x7fELF'
e_phoff, = struct.unpack_from('<I', data, 0x1C)
e_phentsize, = struct.unpack_from('<H', data, 0x2A)
e_phnum, = struct.unpack_from('<H', data, 0x2C)
dyn_off = dyn_sz = None
for i in range(e_phnum):
    off = e_phoff + i * e_phentsize
    p_type, p_off, p_vaddr, p_paddr, p_filesz, p_memsz, p_flags, p_align = struct.unpack_from('<8I', data, off)
    if p_type == 2:  # PT_DYNAMIC
        dyn_off, dyn_sz = p_off, p_filesz
assert dyn_off is not None, 'no PT_DYNAMIC'

DT_NULL, DT_VERNEED, DT_VERNEEDNUM, DT_VERSYM = 0, 0x6FFFFFFE, 0x6FFFFFFF, 0x6FFFFFF0
DT_RUNPATH, DT_RPATH, DT_HASH = 0x1D, 0x0F, 0x4
DT_INIT_ARRAY, DT_INIT_ARRAYSZ, DT_FINI_ARRAY, DT_FINI_ARRAYSZ = 0x19, 0x1A, 0x1B, 0x1C
DT_RELCOUNT = 0x6FFFFFFA  # REQUIRED: tells old ldqnx which leading .rel entries are symbolless RELATIVEs
DROP = (DT_VERNEED, DT_VERNEEDNUM, DT_VERSYM, DT_HASH,
        DT_INIT_ARRAY, DT_INIT_ARRAYSZ, DT_FINI_ARRAY, DT_FINI_ARRAYSZ)
entries = []
o = dyn_off
while o < dyn_off + dyn_sz:
    tag, val = struct.unpack_from('<II', data, o)
    if tag == DT_NULL:
        break
    if tag == DT_RUNPATH:
        tag = DT_RPATH  # old ldqnx only understands DT_RPATH
    if tag not in DROP:
        entries.append((tag, val))
    o += 8

o = dyn_off
for tag, val in entries:
    struct.pack_into('<II', data, o, tag, val)
    o += 8
struct.pack_into('<II', data, o, 0, 0)

# wipe leftover bytes up to old end
for k in range(o + 8, dyn_off + dyn_sz):
    data[k] = 0

# --- extend first PT_LOAD down to file offset 0 (stock QNX binaries map ehdr+phdrs;
#     QNX procnto rejects ET_DYN whose first LOAD starts above 0 with EINVAL) ---
for i in range(e_phnum):
    off = e_phoff + i * e_phentsize
    p_type, p_off, p_vaddr, p_paddr, p_filesz, p_memsz, p_flags, p_align = struct.unpack_from('<8I', data, off)
    if p_type == 1:  # first PT_LOAD
        delta = p_off  # shrink-to-zero offset
        struct.pack_into('<8I', data, off,
                         p_type, 0, p_vaddr - delta, 0,
                         p_filesz + delta, p_memsz + delta, p_flags, p_align)
        break

open(path, 'wb').write(bytes(data))
print(f'stripped; {len(entries)} dynamic entries remain; LOAD0 extended to off 0')
