# 06 — Full-stack system: app + watchdog + daemon

One BB10 phone runs three cooperating layers (all sources now in-repo):

```
┌ Home icon ────────────────┐        ┌ boot (install.sh hook) ──────────┐
│ wgd-console (UI, this repo│        │ ksh wgd-all.sh  (watchdog, root) │
│ examples/) — SDL render   │        │   └ on -d …/wgd  (daemon binary  │
└──────┬────────────────────┘        │      from crates/wgd-daemon,     │
       │ files below                 │      tun0 + UDP crypto loop)     │
       ▼                             └──────────────┬───────────────────┘
 /accounts/devuser/                                 │ tun0 @10.0.10.8
   wgd.log          ◀── daemon stdout ──────────────┤
   wgd_watch.log    ◀── watchdog decisions ─────────┤
   wgd_watch.pid    ◀── "<wgd pid> <generation>"    │
   wgcmd_rebind/ft  ──▶ one-shot commands (app → watchdog)
   wgd_fulltunnel.on── default-route policy flag
```

* **Daemon** (`crates/wgd-daemon` + `crates/wgcrypto`): no_std userspace
  WireGuard — select() over tun0 + UDP, handshake retransmit 5s,
  re-handshake 120s, keepalive 30s, self-daemonizes. Build with
  `BB10_MODE=static ./toolkit/build.sh wgd crates/wgd-daemon` (ET_EXEC,
  statically linked vs QNX libc.a — NO SDL needed; it never touches screen).
  Keys live ONLY in `/accounts/devuser/wgd.conf` on the device (see
  `.example`, never in git).
* **Watchdog** (`devices/wgd-all.sh` v14): 15s poll loop — freeze detection
  (log-size stall counters), relaunch with fresh generation token, per-file
  log rotation (copytruncate), consumes the two `wgcmd_*` files, and owns
  the fulltunnel default-route swap. Installed at `/var/rootdata/wgd-all.sh`
  and started from the user-managed `/var/rootdata/install.sh` hook.
* **UI** (`examples/wgd-console`): tail of `wgd.log`, age-parse of
  `HANDSHAKE-OK`/`rx open ok` lines, `ioctl(SIOCGIFFLAGS)` for tun0 state,
  `/proc/<pid>/as` existence for liveness, two tap targets issuing
  commands via the drop-files.

## Why files, not IPC
The app is sandboxed (uid 102xxxxx), the daemon root+netio — no shared
socket/PPS channel both sides can legally own. `/accounts/devuser` is
`drwxrwxrwx` and the watchdog pre-creates command files `0666`, so
write-to-dropfile + poll is the minimal, reboot-safe bus. (The older UI
generations used `/accounts/1000/shared/documents/...` — same idea; keep
one location per deployment, now standardized on `/accounts/devuser`.)

## Upgrade flow (tunnel stays up)
1. `build.sh` (static) → stage scp `wgd` to `/var/rootdata/wgd.stage`
   (running text can't be overwritten — `Resource busy`).
2. install new watchdog: `scp devices/wgd-all.sh root@PHONE:/var/rootdata/wgd-all.sh`
3. kill the daemon pid from the pidfile → watchdog relaunch loop picks up
   `/var/rootdata/wgd` (launch() copies to /accounts/devuser itself).
4. verify: `wgd.log` grows, `HANDSHAKE-OK` within a minute, console shows
   green DAEMON/tun0.

install.sh itself is user-territory: propose diffs, never auto-edit.
