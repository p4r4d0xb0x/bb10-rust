# Device-side runtime (watchdog + daemon + hook)

The phone runs three pieces that the UI app (`examples/wgd-console`) talks to
through files. Deploy ONCE per device; the boot hook re-installs itself.

| file | install path | role |
|---|---|---|
| `wgd-all.sh` | `/var/rootdata/wgd-all.sh` (v13+) | watchdog: relaunch/freeze-detect, log rotation, consumes `wgcmd_*` from the app |
| `wgd-boot.sh` | invoked from `/var/rootdata/install.sh` | boot hook: starts watchdog in the `g_nto` netio context |
| `wgd.conf.example` | `/accounts/devuser/wgd.conf` (real one, gitignored) | identity/peer config for the daemon |
| daemon binary | built from `../crates/wgd-daemon` -> stage `/var/rootdata/wgd`, watchdog copies to `/accounts/devuser/wgd` | the userspace WireGuard client itself |

Shared-file contract with the console app (all under `/accounts/devuser/`,
`chmod 666`, watchdog pre-creates the cmd files):
`wgd.log` (daemon log, tail source) · `wgd_watch.log` · `wgd_watch.pid`
(`<pid> <gen>` — app /proc-checks the pid) · `wgcmd_rebind` (any bytes =
relaunch) · `wgcmd_ft` (`fton|ftoff`) · `wgd_fulltunnel.on` (presence flag).

**Never edit `/var/rootdata/install.sh` from a script/agent — propose a diff,
the user applies it** (device policy). The hook block currently is line ~170:
`run_as g_nto "ksh < /var/rootdata/wgd-all.sh"`.

Rebuild + hot-swap the daemon (keep the tunnel!):

```sh
BB10_MODE=static BB10_TARGET_LIB=... ../../toolkit/build.sh wgd crates/wgd-daemon
scp crates/wgd-daemon/out/wgd root@PHONE:/var/rootdata/wgd.stage   # NOT direct
ssh root@PHONE 'kill -9 $(awk "{print \$1}" /accounts/devuser/wgd_watch.pid); \
  mv /var/rootdata/wgd.stage /var/rootdata/wgd'                    # watchdog relaunches
# QNX can't overwrite an exec'd file (Resource busy): stage+mv, or let the
# watchdog's launch() copy it on next relaunch (that's its normal path).
```
