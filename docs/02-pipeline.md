# 02 — Pipeline: from `cargo` to a running app

```
  main.rs ──cargo rustc──▶ out/app_pic.o ─┐
  entry_pic.s ──llvm-mc──▶ entry_pic.o  ──┤rust-lld + app.ld + libc.so.3(BB10)
  libcore.rlib / rlibs ──────────────────▶┤
                                          ▼
                              out/app (ELF32 ET_DYN PIE, NEEDED: libc,socket)
                                          │ stripver.py / fixphdr.py / needed_path_to_soname.py
                                          ▼
                              toolkit/pack_bar.py ──▶ dist/app.bar
                                          │ scp + job.tag + sud
                                          ▼
                              /apps/com.*.testDev_*/native/app(.sh)  → Home icon
```

## Step 1 — compile to a single PIC object

`cargo rustc -Z build-std=core --target toolkit/spec/beacon-arm-pic.json
-- --emit obj`

* JSON target: `arm-unknown-none-gnueabi`-shaped, cortex-a9, soft-float,
  `relocation-model: pic`, `panic-strategy: abort`,
  `no-default-libraries: true`. build-std gives a fresh libcore; the
  binary crate is `#![no_std]#![no_main]` + its own `#[panic_handler]`.
* One `.o` keeps the link line dumb and auditable.

## Step 2 — link without any crt

`toolkit/spec/app.ld` + `rust-lld -flavor gnu`:

* `--dynamic-linker=/usr/lib/ldqnx.so.2 --pie` — QNX procnto wants ET_DYN
  + a stock PT_INTERP; `_start` lives in `entry_pic.s` (bl main; bl exit).
* Link inputs: BB10 `libc.so.3`, `libsocket.so.3`, your rlibs, and — for
  the SDL path — the device's `libSDL12.so` itself (so the linker records
  `DT_NEEDED libSDL12.so` and the loader maps it at startup).
* `--allow-shlib-undefined`: libSDL12's own UNDEF refs (GLES etc.) stay
  lazy; you satisfy them via `sdl_link_prims!()`, not via link libs.
* **No `--soname` on executables, ever.** (docs/04 has the post-mortem.)

## Step 3 — normalize the ELF for QNX's loader

1. `elf/stripver.py` — drop `DT_VERNEED/DT_VERNEEDNUM/DT_VERSYM/DT_HASH`,
   init/fini arrays, rewrite `DT_RUNPATH→DT_RPATH`, force
   `DT_RELCOUNT = DT_REL count` (old ldqnx needs it to know how many
   leading `R_*_RELATIVE` relocs are symbolless).
2. `elf/fixphdr.py` — `PT_ARM_EXIDX → PT_PHDR`, canonical phdr order
   (PHDR/INTERP/LOAD-X/DYNAMIC/LOAD-W), `p_paddr = p_vaddr` on loads.
3. `elf/needed_path_to_soname.py` — NEEDED strings that still carry the
   host path (`/tmp/libSDL12.so`) are rewritten to bare sonames in place.

Each script prints what it changed; re-run llvm-readelf to verify the
invariants in docs/03.

## Step 4 — pack

`toolkit/pack_bar.py` writes the exact BAR container a real devmode .bar
has: `META-INF/MANIFEST.MF` (CRLF, sha512-digests per asset) + assets with
zip unix-mode bits. Entry-Point becomes a generated `native/<app>.sh`
wrapper exporting `LD_LIBRARY_PATH=app/native/lib:/proc/boot:/base/lib:...`
(the Term49 mechanism) — a plain-binary EP loses the loader env.

## Step 5 — install

`toolkit/deploy_bar.py` pushes the .bar to `/var/android/`, writes a
`job.tag` into `/pps/system/installer/upd/current/` and wakes the device's
own installer daemon (`on -d /base/usr/sbin/sud.sh`). It polls
`result::success|failure` from the same PPS file. No password, no signing,
no blackberry-deploy binary.

## Debugging a silent app

The example logs every bring-up step to
`/accounts/1000/shared/documents/logs/exp_sdl.log` (boot marker, uid, pid,
SDL_Init tag on failure + `SDL_GetError()` text). Read that file *before*
guessing — on a device without a serial console it is the pipeline's
black box. `exp_sdl.panic` appears only on a Rust panic; a missing log at
all means the process died before `main` (loader/ELF problem).
