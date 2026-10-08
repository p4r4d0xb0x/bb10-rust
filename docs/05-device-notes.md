# 05 — Device notes (BB Classic 10.3.1 field manual)

## Shell reality (this is ksh88 + QNX bin, not GNU)

* `head`, `printf` **as separate binaries**, `tr`, `od` — absent or partial.
  Use `sed -n 1,10p` for head, `awk`/`cut` for fields, `strings` for
  binary greps. `printf` is a ksh builtin — works in interactive ksh but
  NOT as the last element of a pipe (`echo` into it).
* `pidin ar` = ps; `pidin -p PID threads/stack` for live procs;
  `/proc/PID/pmap` = lsof-for-mappings, parse with `grep -o` / sed
  (`grep -Eo` behaves oddly — use `sed -n 's/.*,\(\/[^,]*\)$/\1/p'`).
* `on -d` = detached start via slogger-attached service manager;
  `on -u devuser CMD` runs as uid 100.
* `slog2info` dumps the system log; crashdumps: none visible on stock —
  instrument yourself (our binaries log signals explicitly, docs/02).
* here-docs through `ssh ksh -s` work; through `ksh -c` with quoting don't
  (deploy_bar.py pipes via stdin for exactly this reason).

## Install/uninstall mechanics (no blackberry-deploy)

The installer daemon (`sudo`, PPS `/pps/system/installer/`) takes job files:

```
/pps/system/installer/upd/current/job.tag:
  @<Package-Id>
  action::install|uninstall
  package_id::<Package-Id>
  package_name::<com.x.y>          (install only)
  package_location::/var/android/app.bar
  extras::nomedia
```

`chown upd:upd`, `chmod 660`, then `on -d /base/usr/sbin/sud.sh` kicks it.
Result appears in the same file as `result::success` /
`result::failure <code> <Exception>: <message>`.

**Residue trap:** after a failed install, `action::install` for the same
Package-Id keeps failing with `500 OSError: [Errno 2]` forever. Remedy:
uninstall the id first + `rm -rf /apps/.new.*`, or bump Package-Id
(pack_bar derives ids from content so a rebuild is a new id).

**Zombie sud daemons:** every `on -d /base/usr/sbin/sud.sh` leaves a daemon
when the job completes/aborts; multiple instances then race-consume new
job.tags and poison transactions (symptom: `500 OSError` pointing at random
files under `/apps/.new.*`). deploy_bar.py kills all `[s]ud(.sh|.py)` pids
before writing a job — do the same in hand-run flows.
Also note: devmode `true` apps cannot be replaced by `false` manifests —
the daemon throws ApplicationModeMismatch; uninstall then install.

## Runtime environment gotchas

* App runs **uid = 10218xxxx (per-app sandbox)**, not devuser(100), when
  launched from Home. `/apps/.../native` is mounted `noexec` for the
  appdata symlink path — binaries must live in `/apps/.../native` proper.
* Only `/accounts/1000/shared/**` is writable cross-app (sdcard-like). Our
  apps log there.
* `LD_LIBRARY_PATH` comes ONLY from the Entry-Point env-prefix/wrapper
  trick; nothing else sets it.
* Files the OS searches: `libm.so.2` = `/proc/boot` **only**; `libasound`
  = `/base/lib`; screen/bps/egl/gles = `/base/usr/lib`.
* `Resource busy` on overwrite = the binary is currently exec'd (QNX text
  binding) — kill the process before hot-swapping.

## Verifying a launch actually happened

```
pidin ar | grep myapp                       # running?
cat /accounts/1000/shared/documents/logs/exp_app.log   # boot marker: uid/pid
ls -d /apps/com.x.myapp.*/native           # installed at all?
grep -o 'com.x.myapp[^ :]*' /pps/system/navigator/applications/applications
```

Empty log + no process = died pre-main (ELF/loader; docs/03/04).
Log + no render = group/composition (docs/04). Panic file = Rust panic.

## Screen/geometry

* Classic: 720x720 square, 140dpi-ish; landscape apps letterbox — the
  wgd-console example targets the classic-square portrait dashboard.
* Touch reaches SDL as mouse events; the TCO (TouchControlOverlay) lib in
  native/lib is the screen↔SDL adapter (bundle it with libSDL12).
