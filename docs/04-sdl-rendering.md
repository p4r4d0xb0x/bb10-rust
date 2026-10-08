# 04 — Native rendering on BB10: the window-group problem (and the SONAME trap)

## Why hand-rolled libscreen apps stay black

A Qnx/Elf app that opens `screen_create_session` + `screen_create_window`
directly **composites behind the navigator splash forever**. BB10's display
server only shows windows that belong to the *window group* navigator hands
a launched app; a standalone group never gets scanned out. Attempts that
**all failed on a Classic 10.3.1** before we accepted this:

* `screen_create_window_group` with ids scraped from `/pps/system/navigator/*`
* joining the "own" group from `NAV_WINDOW_STATE` events (that callback can
  legitimately return the literal string `"none"` — a 5-byte `"none\0"`,
  compare it fully if you use it)
* posting from a `screen_create_window_group` attached to a hidden
  `screen_create_pixmap`
* white-probe frames (paint 0xFFFFFFFF, flip 4s): stayed black ⇒ the
  problem is *composition policy*, not pixels.

FreeGAG's SDL3 QNX driver manages windows through plain `screen_create_window`
+ buffer fill/post, and Term49's libSDL12 goes further: its PLAYBOOK driver
imports `screen_create_window_type` + `screen_join_window_group` — the join
is done *by the blessed SDL library* (which is why every visible native app
you find in the wild carries libSDL12, Cascades, or AIR: they all reach the
compositor through sanctioned libraries that know the group dance).

**=> Do the SDL route: link libSDL12 into your binary and call the C ABI.**

## The SONAME trap (one week of 'crashes' in one paragraph)

`build.sh` must **never pass `--soname`** when linking an executable.

Observed failure: binary linked with `--soname=libSDL12.so` boots, logs
`pre-init mark`, then dies silently inside `SDL_Init` — no signal (even a
SIGACTION-captured 1..31 never fired), no panic file. With `LD_DEBUG=1` the
QNX loader shows the truth:

```
debug: Symbol SDL_Init could not be resolved in this scope
unknown symbol: SDL_Init
ldd:FATAL: Unresolved symbol "SDL_Init" called from Executable
Resolution scope for libSDL12.so->.../native/wgd-sdl:
        libSDL12.so->/apps/.../native/wgd-sdl      ← the EXE, not the .so!
        libc.so.3->/usr/lib/ldqnx.so.2
```

Mechanism: ldqnx resolves NEEDED by scanning the link map by SONAME. Our
executable advertised `SONAME libSDL12.so`, so the NEEDED entry matched the
**executable itself**; the real `native/lib/libSDL12.so` was never mapped,
and the first lazy PLT call into it (inside SDL_Init) aborts the process.

Fixes:

1. never `--soname` on binaries (toolkit/build.sh enforces),
2. keep NEEDED as a bare soname — if the linker recorded a host path,
   `elf/needed_path_to_soname.py` rewrites it in place,
3. confirm `llvm-readelf -d out/app | grep SONAME` is **empty**.

## What libSDL12 (and friends) demand from the executable

Expand `bb10_sdl::sdl_link_prims!()` **in the binary crate**. Missing these
= `SIGBUS`-less instant abort inside `SDL_Init` (PLT lazy-bind):

* playbook input callbacks: `handleKeyboardEvent`, `handle_virtualkeyboard_event`,
  `indicate_event_input`, `lock_input`, `unlock_input`
  (Term49's nm shows them as its own T symbols)
* frame registry: `__register_frame_info`, `__deregister_frame_info`,
  `__register_frame`, `__deregister_frame` — imported by the QNX GLES
  drivers which dlopen lazily under EGL init
* EGL trace hooks: `_egl_tls`, `_egl_default_context`

Why the macro and not an rlib: rust-lld resolves shared-library undefineds
by *ignoring* them (`--allow-shlib-undefined`); it never pulls an archive
member out of a .rlib just to satisfy a `.so`'s import. The export has to
be in an object passed directly on the link line — i.e. the binary's own
`--emit obj` output. (This exact trap moved us from `bb10-sdl/src/lib.rs`
into the macro; a binary that forgets to expand it dies with zero logs.)

## Surface layout facts (ARM 32-bit SDL 1.2)

`SDL_Surface { flags@0 format@4 w@8 h@12 pitch@16 pixels@20 }`; the
PLAYBOOK driver's `flip` posts the pixel buffer behind `pixels`. On the
Classic: `SDL_SetVideoMode(720,720,32,HWSURFACE|DOUBLEBUF)` →
pitch=2880, 2 back buffers. Event struct = 56 bytes; `type@0`,
`state@1`, mouse `x@4(int16) y@6(int16)`.
