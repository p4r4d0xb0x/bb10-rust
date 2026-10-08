# bb10-rust

Rust on **BlackBerry 10** (QNX Neutrino, armv7/cortex-a9, e.g. Classic/Q10/Z10/Z30)
with **no SDK, no signing, no device-side toolchain** — a pure-host cross
pipeline that produces installable native `.bar` apps rendering through the
device's own `libSDL12.so`.

Proven on a BlackBerry Classic (10.3.1): a 720×720 dashboard app boots from
Home, draws via SDL→libscreen, survives the navigator compositor, and talks
to on-device daemons through shared-docs files + ioctl.

## Why this exists

BB10's official Cascades/Qt path is dead tooling. Reverse-engineering the
platform shows exactly **two** ways a Qnx/Elf app reaches the screen:

1. Hand-rolled `libscreen` windows — **render behind the splash forever**
   (you never join the navigator window group correctly; even white-frame
   probes stay black).
2. The device's bundled **SDL 1.2 port** (`libSDL12.so`, "PLAYBOOK" driver,
   the one Term49/FreeGAG/Stunt Car Racer use): it creates a
   `screen_create_window_type` window and **joins the navigator window group
   itself** at `SDL_Init` time. This repo gives you that path from Rust.

## Layout

```
crates/
  bbui-core/    no_std UI kit: canvas, 12x22 bitmap font, hangul, widgets,
                event model (+ optional libscreen/bps direct-FFI shell)
  bb10-sdl/     no_std bindings to libSDL12.so — the rendering path
toolkit/
  build.sh      cargo -> rust-lld dynamic link -> ELF normalization
  pack_bar.py   hand-rolled .bar packager (MANIFEST.MF + assets, devmode)
  deploy_bar.py on-device installer via the sudo PPS job queue (no SDK)
  elf/          stripver.py, fixphdr.py, needed_path_to_soname.py
  spec/         target json, linker script, _start asm
docs/
  01-toolchain.md   host setup, pulled device libs
  02-pipeline.md    what build.sh actually does, step by step
  03-elf-notes.md   QNX loader quirks the ELF normalizers fix
  04-sdl-rendering.md  the window-group problem + the SONAME trap
  05-device-notes.md   install/uninstall residue, pids, env, debugging
examples/
  wgd-console/  device-proven app: WireGuard dashboard (file-tail UI + ioctl
                link state + tap-to-command buttons)
```

## Quickstart

```sh
# 0. one-time: pull the link-time libs + SDL runtime from a device (docs/01)
#    libc.so.3, libsocket.so.3 (target pkg), libSDL12.so + libTouchControlOverlay.so

# 1. build (host: rustup nightly + llvm-mc + python3)
BB10_TARGET_LIB=/path/to/target_10_2_0_1155/qnx6/armle-v7/lib \
  ./toolkit/build.sh wgd-console examples/wgd-console \
  libSDL12.so:/path/libSDL12.so

# 2. pack
./toolkit/pack_bar.py --name wgd-console \
  --bin examples/wgd-console/out/wgd-console \
  --lib /path/libSDL12.so --lib /path/libTouchControlOverlay.so

# 3. install (device on same LAN / tunnel, Dev Mode ON)
./toolkit/deploy_bar.py --host PHONE_IP --key ~/.ssh/bb10 \
  --pkgname com.bb10rust.wgd_console --pkgid <printed-id> \
  --install dist/wgd-console.bar

# 4. tap the icon on the phone. really.
```

Minimum binary source is 15 lines — see `examples/wgd-console/src/main.rs`
for the full pattern (`sdl_link_prims!()` in the **binary**, then
`bb10_sdl::init(720,720)`).

## Hard rules learned the expensive way

* The executable must NOT have a `DT_SONAME`. If it repeats `libSDL12.so`,
  QNX's loader resolves the NEEDED entry to the *executable itself*, skips
  loading the real DSO, and `SDL_Init` dies as an unresolved bootstrap
  symbol (`ldd:FATAL: Unresolved symbol "SDL_Init" called from Executable`)
  — after `main` already started, so it looks like a crash.
* `libSDL12.so` is a **link-time** dependency (passed to rust-lld, recorded
  as NEEDED). Do not `dlopen` it at runtime.
* Five playbook-driver callbacks (`handleKeyboardEvent`,
  `handle_virtualkeyboard_event`, `indicate_event_input`, `lock_input`,
  `unlock_input`) + four frame-registry stubs must be **exported by the
  executable**. They cannot live in an rlib — rust-lld won't pull archive
  members to satisfy shared-lib imports. `sdl_link_prims!()` in main.rs.
* Entry-Point needs `LD_LIBRARY_PATH` covering `app/native/lib`,
  `/proc/boot` (libm.so.2) and `/base/lib` (libasound.so.2) —
  `pack_bar.py` generates the `.sh` wrapper automatically.
* Device ELF hygiene: no `.dynsym` versioning, `DT_HASH`, init/fini arrays;
  `PT_ARM_EXIDX` repurposed as `PT_PHDR`; `p_paddr == p_vaddr`; ≤6 phdrs.
  `toolkit/elf/*` does all of it.

## License

MIT OR Apache-2.0 for the code in this repository. `libSDL12.so` /
`libTouchControlOverlay.so` are NOT included — they are runtime files from
your device (docs/01 explains how to pull them); keep them out of git.
