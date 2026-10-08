#!/usr/bin/env python3
"""Normalize ET_DYN ELF32 for QNX procnto: ARM_EXIDX hdr -> PT_PHDR, stock header order."""
import struct, sys

path = sys.argv[1]
d = bytearray(open(path, 'rb').read())
phoff, = struct.unpack_from('<I', d, 0x1C)
phes, = struct.unpack_from('<H', d, 0x2A)
phn, = struct.unpack_from('<H', d, 0x2C)
hdrs = [list(struct.unpack_from('<8I', d, phoff + i * phes)) for i in range(phn)]

for h in hdrs:
    if h[0] == 0x70000001:  # PT_ARM_EXIDX -> PT_PHDR (procnto wants a PT_PHDR)
        h[:] = [6, phoff, phoff, phoff, phes * phn, phes * phn, 4, 4]

order = []
def take(t, pred=lambda h: True):
    for h in hdrs:
        if h[0] == t and pred(h):
            hdrs.remove(h)
            order.append(h)
            return
take(6)                          # PT_PHDR
take(3)                          # PT_INTERP
take(1, lambda h: h[6] & 1)      # PT_LOAD X
take(2)                          # PT_DYNAMIC
take(1, lambda h: h[6] & 2)      # PT_LOAD W
order += hdrs

# --- stock QNX invariant: PT_LOAD paddr == vaddr. ---
loads = [h for h in order if h[0] == 1]
for h in loads:
    h[3] = h[2]  # paddr = vaddr

newb = b''.join(struct.pack('<8I', *h) for h in order)
d[phoff:phoff + phes * len(order)] = newb
open(path, 'wb').write(bytes(d))
print(f'normalized {len(order)} phdrs')
