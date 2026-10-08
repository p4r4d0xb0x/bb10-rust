# 01 — Toolchain setup (host side)

What you need on a modern machine (macOS/Linux) to build BB10 armv7 binaries:

| piece | why | get |
|---|---|---|
| rustup **nightly** | `-Z build-std=core` + JSON target specs | `rustup toolchain install nightly` |
| LLVM ≥ 14 (`llvm-mc`) | assemble `toolkit/spec/entry_pic.s` → PIC `_start` | `brew install llvm` / apt llvm |
| python3 | the three ELF normalizers in `toolkit/elf/` | system |
| BB10 target libs | link-time `libc.so.3`, `libsocket.so.3` | extract `target_10_3_*_*.7z` from the (archive.org) Momentics native SDK, or pull `/proc/boot/libc.so.3` from the device |
| device runtime libs (SDL path) | `libSDL12.so`, `libTouchControlOverlay.so` — bundle into your .bar | pull from any working SDL app dir: `/apps/com.example.Term49*/native/lib/` (same bytes across Term49/FreeGAG builds on 10.3.1) |

## Pulling libs from the device (SSH/Dev Mode)

```sh
# one-time, from a working devmode app on the phone
adb/scp equivalent over ssh:
  scp root@PHONE:/apps/com.example.Term49.*/native/lib/libSDL12.so     /path/libs/
  scp root@PHONE:/apps/com.example.Term49.*/native/lib/libTouchControlOverlay.so /path/libs/
# sanity: Term49 pmap shows the exact loader paths of every dep:
  cat /proc/$(pidin ar | awk '/Term49/{print $1}')[0]/pmap
  # libm.so.2   -> /proc/boot/libm.so.2   (NOT in /usr/lib!)
  # libasound.so.2 -> /base/lib/libasound.so.2
```

Set `BB10_TARGET_LIB` to the dir containing `libc.so.3` + `libsocket.so.3`
(the SDK target pkg's `armle-v7/lib`). We verified against the extracted
Momentics target package `target_10_2_0_1155` — the link-time libc flavour
doesn't matter much; only SONAME/ABI do.

**Do not commit device-pulled `.so` files** — copyright + they're 270 KB of
someone else's build. Add them to a local `vendor/` dir covered by .gitignore.

## Device-side prereqs

* Dev Mode ON (Settings → Security and Development).
* SSH reachable as root (our experiments ran over WireGuard `PHONE_IP:22`;
  the old `dbdaemon`/USB path works too but deploy_bar.py speaks SSH).
* SSHd on 10.3.1 is ancient: needs `HostKeyAlgorithms=+ssh-rsa`,
  `KexAlgorithms=diffie-hellman-group14-sha1`, `Ciphers=aes128-ctr` —
  deploy_bar.py already passes all of these.
