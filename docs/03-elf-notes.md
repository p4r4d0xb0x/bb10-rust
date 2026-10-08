# 03 — ELF notes for QNX 10's loader (ldqnx + procnto)

The 2013-vintage QNX loader rejects/ignores several things modern linkers
emit by default. Every rule below was established by an observed failure on
device, not by reading docs.

| invariant | tool that enforces | symptom when violated |
|---|---|---|
| no `DT_VERNEED`/`DT_VERNEEDNUM`/`DT_VERSYM` | elf/stripver.py | `ldd:FATAL` at exec, before main, empty log |
| `DT_HASH` dropped (GNU_HASH kept) | elf/stripver.py | stale `.hash` table desyncs after in-place dyn edits |
| init/fini *arrays* removed (only `DT_INIT` used) | elf/stripver.py | procnto SIGSEGV on null fn ptr in array walk |
| `DT_RUNPATH` demoted to `DT_RPATH` | elf/stripver.py | bundled libs invisible regardless of LD_LIBRARY_PATH ordering |
| `DT_RELCOUNT` emitted by rust-lld; kept as-is | (linker) | old ldqnx needs it to know how many leading `.rel` entries are symbolless RELATIVEs |
| first PT_LOAD starts at file offset 0 | elf/stripver.py | procnto `EINVAL` on exec (stock QNX binaries map ehdr+phdrs) |
| `PT_PHDR` exists; ARM exidx phdr repurposed | elf/fixphdr.py | `cannot execute` on exec |
| phdr order: PHDR,INTERP,LOAD-X,DYNAMIC,LOAD-W | elf/fixphdr.py | sometimes maps, sometimes SIGBUS |
| `p_paddr == p_vaddr` for PT_LOAD | elf/fixphdr.py | PIE base math off by the load bias |
| ≤6 PT_LOAD-class phdrs, 4K alignment | app.ld segment merge | `not enough space for headers`? → exec fails |
| NEEDED entries are **bare sonames** | elf/needed_path_to_soname.py | loader searches the literal path → `Could not load library` |
| executable has **no DT_SONAME** | build.sh (never passes --soname) | catastrophic aliasing: see docs/04 |

Check an artifact:

```sh
llvm-readelf -d out/app | grep -E "NEEDED|SONAME|RELCOUNT|VERNEED"
llvm-readelf --program-headers out/app | grep -cE "LOAD|PHDR|INTERP|DYNAMIC"
```

Why the version scripts matter: rust's own `libcompiler_builtins` pulls in
unversioned symbols fine, but `libc.so.3` on device carries SVID-style
versioned symbols from 2011 QNX; emitting DT_VERNEED against them makes
ldqnx bail with an opaque OSError 2 from its python-side installer hook
when you're lucky, or just SIGKILL the exec otherwise. stripver keeps the
*reference* to the versioned definition (fine for lazy lookup) while
removing the *verneed table* — the loader then treats everything as
"match by name" which is exactly what Term49-era binaries look like.
