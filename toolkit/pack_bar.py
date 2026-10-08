#!/usr/bin/env python3
"""Pack a BB10 native (Qnx/Elf) app into an installable .bar.

No SDK, no signing: produces a devmode-installable BAR (the same container
form verified end-to-end on a Classic). Layout the device expects:

    META-INF/MANIFEST.MF          sha512 digests over every asset
    native/<bin>                  the ELF
    native/<bin>.sh               LD_LIBRARY_PATH wrapper (see --wrapper)
    native/lib/*                  bundled shared libs (e.g. libSDL12.so)
    config/icon.png

Package-Id rule: 'testDev_' + base64url(first 17 chars of author string),
trailing '=' stripped -> 27-28 chars total, must be urlsafe-b64 decodable.

Examples:
    ./pack_bar.py --name myapp --bin out/myapp --icon assets/icon.png
    ./pack_bar.py --name console --bin out/wgd-console \
        --lib /path/libSDL12.so:/path/libTouchControlOverlay.so \
        --author paradox --ver 1.0.0.0 --pkg com.tbbx.console --out dist/
"""
import argparse, base64, hashlib, zipfile
from pathlib import Path

AP = argparse.ArgumentParser()
AP.add_argument("--name", required=True, help="app/entry-point display name")
AP.add_argument("--bin", required=True, help="path to the ELF binary")
AP.add_argument("--icon", help="png icon (default: a 1x1 placeholder)")
AP.add_argument("--lib", action="append", default=[],
                help="shared lib to bundle under native/lib/ (repeatable)")
AP.add_argument("--asset", action="append", default=[],
                help="extra 'srcpath:destpath-in-bar' (repeatable)")
AP.add_argument("--pkg", default=None, help="Package-Name (default com.<author>.<name>)")
AP.add_argument("--author", default="bb10rust", help="Package-Author")
AP.add_argument("--ver", default="1.0.0.0")
AP.add_argument("--out", default="dist", help="output dir")
AP.add_argument("--orientation", default="auto", choices=["auto", "portrait", "landscape"])
AP.add_argument("--actions", default="access_shared,run_native",
                help="comma list of User-Actions; run_native is added to System automatically")
AP.add_argument("--libpath-extra", default="/proc/boot:/base/lib:/base/usr/lib:/usr/lib",
                help="LD_LIBRARY_PATH tail appended to app/native/lib in the wrapper")
AP.add_argument("--no-wrapper", action="store_true",
                help="use plain Entry-Point (only if the binary needs no env)")
ARGS = AP.parse_args()

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent

def b64id(s: str, width=17):
    raw = (s + " " * width)[:width].encode()
    return base64.urlsafe_b64encode(raw).decode().rstrip("=")

pkg_author = ARGS.author
pkg_name = ARGS.pkg or f"com.{pkg_author.lower()}.{ARGS.name.lower().replace(' ', '_').replace('-', '_')}"
assert pkg_name[:4] == "com.", "BB10 Package-Name must start with 'com.'"

# Verified device invariant: Package-Id looks like the id real devmode
# installs mint ("testDev_" + 12-14 chars of [A-Za-z0-9-_], 27-28 total).
# Anything matching that shape installs; deriving the tail from
# sha256(author+pkg_name) keeps re-installs idempotent across builds.
import re as _re
tail = base64.urlsafe_b64encode(
    hashlib.sha256((pkg_author + pkg_name).encode()).digest()
).decode().rstrip("=")
pkg_id = ("testDev_" + _re.sub(r"[^A-Za-z0-9_-]", "x", tail))[:27]
assert 24 <= len(pkg_id) <= 28, pkg_id

def digest(data: bytes) -> str:
    return base64.urlsafe_b64encode(hashlib.sha512(data).digest()).decode().rstrip("=")

binp = Path(ARGS.bin)
elf = binp.read_bytes()

if ARGS.icon:
    icon = Path(ARGS.icon).read_bytes()
else:
    # 1x1 transparent png placeholder
    icon = base64.b64decode(
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk"
        "AAAAMAAAAOAHAug1AAAAAElFTkSuQmCC")

appdir = f"/apps/{pkg_name}.{pkg_id}/native"
# Single-instance guard: a swiped-closed Qnx/Elf app is NOT killed by
# navigator (SDL apps get no SDL_QUIT on close either), so a zombie keeps
# the window group and the second launch renders nothing / crashes.
# Kill any previous instance before exec (our own pid isn't exec'd yet).
wrapper = (f"#!/bin/sh\n"
           f"BASE={appdir}\n"
           f"LD_LIBRARY_PATH=$BASE/lib:{ARGS.libpath_extra}\n"
           f"export LD_LIBRARY_PATH\n"
           f"for p in $(pidin ar | grep \"native/{ARGS.name}$\" | awk '{{print $1}}'); do\n"
           f"  kill -9 $p 2>/dev/null\n"
           f"done\n"
           f"exec $BASE/{ARGS.name}\n").encode()

assets = [
    (f"native/{ARGS.name}", elf, 0o755),
    ("config/icon.png", icon, 0o644),
]
if not ARGS.no_wrapper:
    assets.insert(1, (f"native/{ARGS.name}.sh", wrapper, 0o755))
for l in ARGS.lib:
    p = Path(l)
    assets.append((f"native/lib/{p.name}", p.read_bytes(), 0o755))
for a in ARGS.asset:
    src, _, dest = a.partition(":")
    assets.append((dest or f"native/{Path(src).name}", Path(src).read_bytes(), 0o644))

user_actions = [x.strip() for x in ARGS.actions.split(",") if x.strip() and x.strip() != "run_native"]
M = [
    "Archive-Manifest-Version: 1.5",
    "Archive-Created-By: bb10-rust pack_bar 0.1",
    "",
    "Package-Type: application",
    f"Package-Author: {pkg_author}",
    f"Package-Author-Id: {b64id(pkg_author)}",
    f"Package-Name: {pkg_name}",
    f"Package-Id: {pkg_id}",
    f"Package-Version: {ARGS.ver}",
    f"Package-Version-Id: {b64id(ARGS.ver)}",
    "Package-Architecture: armle-v7",
    "",
    f"Application-Name: {ARGS.name}",
    f"Application-Id: {pkg_id}",
    f"Application-Description: {ARGS.name} (bb10-rust)",
    f"Application-Version: {ARGS.ver}",
    f"Application-Version-Id: {b64id(ARGS.ver)}",
    "Application-Requires-System: BlackBerry 10/10.0.9.0",
    "Application-Development-Mode: false",
    "",
    f"Entry-Point-Name: {ARGS.name}",
    "Entry-Point-Key: e1",
    f"Entry-Point: app/native/{ARGS.name}" + (".sh" if not ARGS.no_wrapper else ""),
    "Entry-Point-Type: Qnx/Elf",
    "Entry-Point-Icon: config/icon.png",
    f"Entry-Point-Orientation: {ARGS.orientation}",
]
if user_actions:
    M.append("Entry-Point-User-Actions: " + ",".join(user_actions))
M.append("Entry-Point-System-Actions: run_native")
M.append("")
for name, data, _ in assets:
    M.append(f"Archive-Asset-Name: {name}")
    M.append(f"Archive-Asset-SHA-512-Digest: {digest(data)}")
M.append("")
manifest = "\r\n".join(M).encode()

out = Path(ARGS.out)
out.mkdir(parents=True, exist_ok=True)
bar = out / f"{ARGS.name}.bar"
with zipfile.ZipFile(bar, "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("META-INF/MANIFEST.MF", manifest)
    for name, data, mode in assets:
        zi = zipfile.ZipInfo(name, (2026, 1, 1, 0, 0, 0))
        zi.external_attr = (mode << 16)
        zi.compress_type = zipfile.ZIP_DEFLATED
        z.writestr(zi, data)

meta = {
    "bar": str(bar),
    "bytes": bar.stat().st_size,
    "pkg_name": pkg_name,
    "pkg_id": pkg_id,
    "appdir": f"/apps/{pkg_name}.{pkg_id}",
    "entry": f"app/native/{ARGS.name}" + (".sh" if not ARGS.no_wrapper else ""),
}
print(meta)
