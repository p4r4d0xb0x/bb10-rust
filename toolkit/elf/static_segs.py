import struct, sys
def segs(path):
    d = open(path, 'rb').read()
    e_phoff, = struct.unpack_from('<I', d, 0x1c)
    e_phentsize, e_phnum = struct.unpack_from('<HH', d, 0x2a)
    etype, emach = struct.unpack_from('<HH', d, 0x10)
    eflags, = struct.unpack_from('<I', d, 0x28)
    print(f"== {path}: type={etype} flags={eflags:#x} phnum={e_phnum}")
    for i in range(e_phnum):
        o = e_phoff + i * e_phentsize
        p_type, off, va, pa, fsz, msz, fl, al = struct.unpack_from('<IIIIIIII', d, o)
        print(f"  {p_type:#010x} off={off:#8x} vaddr={va:#010x} paddr={pa:#010x} fsz={fsz:#x} msz={msz:#x} f={fl} a={al}")
for p in sys.argv[1:]:
    segs(p)
