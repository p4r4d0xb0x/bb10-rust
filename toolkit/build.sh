#!/bin/bash
# bb10-rust build pipeline: cargo (build-std=core, PIC) -> rust-lld dynamic
# link vs BB10 libc -> ELF post-normalization for QNX ldqnx/procnto.
#
#   ./build.sh <binary-name> <crate-dir> [extra .so to link...]
#
# Examples:
#   ./build.sh wgd-console ../examples/wgd-console
#   ./build.sh wgd-daemon crates/wgd-daemon          # BB10_MODE=static
# env: BB10_MODE=dynamic(default)|static  static = crt1S+libc.a ET_EXEC
#      (daemon payload: runs from /accounts/devuser via watchdog)
#
# Prereqs (see docs/01-toolchain.md):
#   - rustup nightly (any host; build host need NOT be the target)
#   - BB10 native target package (armle-v7/lib/libc.so.3, libsocket.so.3)
#     — set $BB10_TARGET_LIB (or the default path under the repo checkout)
#
# The link line NEVER passes --soname to the linker: an executable SONAME
# hijacks NEEDED resolution in QNX's loader (docs/04-sdl-rendering.md).
set -e
HERE="$(cd "$(dirname "$0")" && pwd)"
NAME="${1:?usage: build.sh <binary> <crate-dir> [extra-libs...]}"
CRATE="${2:?crate dir with Cargo.toml}"
shift 2

# --- host config (edit for your machine) ------------------------------------
RUSTUP_N="${RUSTUP_N:-}"
if [ -z "$RUSTUP_N" ] || [ ! -d "$RUSTUP_N" ]; then
  RUSTUP_N=$(ls -d "$HOME"/.rustup/toolchains/nightly-* 2>/dev/null | sed -n 1p)
fi
[ -n "$RUSTUP_N" ] || { echo "FATAL: no rustup nightly found (set RUSTUP_N)"; exit 1; }
RUST_LLD="$RUSTUP_N/lib/rustlib/$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')/bin/rust-lld"
[ -x "$RUST_LLD" ] || { echo "FATAL: rust-lld missing at $RUST_LLD"; exit 1; }
LLVM_MC="${LLVM_MC:-llvm-mc}"                      # any llvm ≥ 14 (brew install llvm)
BB10_TARGET_LIB="${BB10_TARGET_LIB:-$HERE/../bb10qnx/tools/target_10_2_0_1155/qnx6/armle-v7/lib}"

LIBC="$BB10_TARGET_LIB/libc.so.3"
LIBSOCK="$BB10_TARGET_LIB/libsocket.so.3"
[ -f "$LIBC" ] || { echo "FATAL: $LIBC not found — set BB10_TARGET_LIB (docs/01-toolchain.md)"; exit 1; }

# extra shared libs: arg form "soname:path" (linkable), appended to NEEDED
NEEDED_ARGS=()
EXTRA_SO=()
for spec in "$@"; do
  soname="${spec%%:*}"; path="${spec#*:}"
  [ "$soname" = "$path" ] && { echo "FATAL: extra lib must be soname:/path ($spec)"; exit 1; }
  [ -f "$path" ] || { echo "FATAL: $path missing (see docs/01-toolchain.md §pulled libs)"; exit 1; }
  EXTRA_SO+=("$path")
  NEEDED_ARGS+=("$soname")
done

CRATE_ABS="$(cd "$CRATE" && pwd)"
OUTD="$CRATE_ABS/out"   # rustc's cwd is the workspace root: paths must be absolute
cd "$CRATE_ABS"
export PATH="$RUSTUP_N/bin:$PATH" DYLD_LIBRARY_PATH="$RUSTUP_N/lib"
rm -f "$OUTD/$NAME" "$OUTD/${NAME}_pic.o"
mkdir -p "$OUTD"
SPEC=beacon-arm-pic.json
[ "${BB10_MODE:-dynamic}" = static ] && SPEC=beacon-arm-static.json
cargo rustc -Z unstable-options -Z json-target-spec -Z build-std=core \
  --target "$HERE/../toolkit/spec/$SPEC" --release --bin "$NAME" \
  -- --emit obj -o "$OUTD/${NAME}_pic.o"
test -f "$OUTD/${NAME}_pic.o" || { echo "BUILD FAILED"; exit 1; }

# _start for PIE binaries (no crt0 — QNX procnto hands us a fresh stack)
MODE="${BB10_MODE:-dynamic}"
test -f /tmp/bb10r-entry_pic.o || \
  $LLVM_MC -triple armv7-unknown-none-gnueabi -filetype obj \
      -o /tmp/bb10r-entry_pic.o "$HERE/../toolkit/spec/entry_pic.s"

WS="$(cd "$HERE/.." && pwd)"   # workspace root: cargo target dir lives here
TG=$(basename "$HERE/../toolkit/spec/$SPEC" .json)

# --- link: rust-lld, dynamic ET_DYN PIE (apps) or static ET_EXEC (daemons) --
if [ "$MODE" = static ]; then
  TG2=beacon-arm-static
  SCORE=$(ls -t "$WS"/target/$TG2/release/build/core/*/out/libcore-*.rlib | sed -n 1p)
  SBUILT=$(ls -t "$WS"/target/$TG2/release/build/compiler_builtins/*/out/libcompiler_builtins-*.rlib | sed -n 1p)
  SRLIBS=()
  while IFS= read -r l; do SRLIBS+=("$l"); done < <(
    ls -t "$WS"/target/$TG2/release/build/*/*/out/lib*.rlib 2>/dev/null \
      | grep -vE "libcore-|libcompiler_builtins-" | sort -u)
  L="$BB10_TARGET_LIB"
  "$RUST_LLD" -flavor gnu -o "$OUTD/$NAME" \
    -T "$HERE/../toolkit/spec/static.ld" -z max-page-size=0x10000 \
    "$L/crt1S.o" "$L/crti.o" "$OUTD/${NAME}_pic.o" "$L/crtn.o" \
    "$L/libc.a" "$L/libsocket.a" "$L/libc.a" "$SCORE" ${SRLIBS[@]+"${SRLIBS[@]}"} "$SBUILT"
else
CORE=$(ls -t "$WS"/target/$TG/release/build/core/*/out/libcore-*.rlib | sed -n 1p)
BUILTINS=$(ls -t "$WS"/target/$TG/release/build/compiler_builtins/*/out/libcompiler_builtins-*.rlib | sed -n 1p)
LIBS=()
while IFS= read -r l; do LIBS+=("$l"); done < <(
  ls -t "$WS"/target/$TG/release/build/*/*/out/lib*.rlib 2>/dev/null \
    | grep -vE "libcore-|libcompiler_builtins-" | sort -u)
"$RUST_LLD" -flavor gnu -o "$OUTD/$NAME" \
  -T "$HERE/../toolkit/spec/app.ld" -z max-page-size=0x1000 \
  --dynamic-linker=/usr/lib/ldqnx.so.2 --pie \
  --allow-shlib-undefined --no-undefined-version \
  /tmp/bb10r-entry_pic.o "$OUTD/${NAME}_pic.o" \
  "$LIBC" "$LIBSOCK" ${EXTRA_SO[@]+"${EXTRA_SO[@]}"} \
  "$CORE" ${LIBS[@]+"${LIBS[@]}"} "$BUILTINS"
fi

# --- normalize for old ldqnx/procnto (docs/03-elf-notes.md) -----------------
if [ "$MODE" = dynamic ]; then
python3 "$HERE/elf/stripver.py" "$OUTD/$NAME"
python3 "$HERE/elf/fixphdr.py" "$OUTD/$NAME"
if [ ${#EXTRA_SO[@]} -gt 0 ]; then
  python3 "$HERE/elf/needed_path_to_soname.py" "$OUTD/$NAME" ${NEEDED_ARGS[@]}
fi
fi
ls -la "$OUTD/$NAME"
echo "OK: $OUTD/$NAME"
