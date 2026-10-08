#!/usr/bin/env python3
"""Install/uninstall a .bar on a BB10 device with Dev Mode on — no SDK.

Mechanism (verified end-to-end on QNX 10.2/10.3): the device's *installer
daemon* (sudo) consumes job files dropped at
    /pps/system/installer/upd/current/job.tag
and reports `result::` into the same file. We write the tag over SSH and
kick the daemon with `on -d /base/usr/sbin/sud.sh`.

Residue gotcha (docs/05-device-notes.md): a prior *failed* transaction for
the same Package-Id makes new installs fail with
    result::failure 500 OSError: [Errno 2] No such file or directory
Fix = uninstall the id first (or bump the id), plus `rm -rf /apps/.new.*`.

Usage:
    ./deploy_bar.py --host PHONE_IP --key ~/.ssh/bb10 --install dist/app.bar
    ./deploy_bar.py --host ... --key ... --uninstall-pkgid testDev_xxxx
"""
import argparse, subprocess, sys, time
from pathlib import Path

AP = argparse.ArgumentParser()
AP.add_argument("--host", required=True)
AP.add_argument("--key", default=None)
AP.add_argument("--user", default="root")
AP.add_argument("--install", help=".bar to push+install")
AP.add_argument("--uninstall", help="uninstall then install this .bar")
AP.add_argument("--pkgid", required=True, help="Package-Id inside the bar")
AP.add_argument("--pkgname", required=True, help="Package-Name (com....)")
ARGS = AP.parse_args()

SSHOPTS = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=15",
           "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null",
           "-o", "ServerAliveInterval=10", "-o", "ServerAliveCountMax=12"]
if ARGS.key:
    SSHOPTS += ["-i", ARGS.key, "-o", "IdentitiesOnly=yes"]
# legacy BB10 sshd algorithm set
SSHOPTS += ["-o", "HostKeyAlgorithms=+ssh-rsa", "-o", "PubkeyAcceptedAlgorithms=+ssh-rsa",
            "-o", "KexAlgorithms=diffie-hellman-group14-sha1", "-o", "Ciphers=aes128-ctr",
            "-o", "MACs=hmac-sha1"]
TGT = f"{ARGS.user}@{ARGS.host}"

def run(cmd, timeout=400):
    print("+", cmd.splitlines()[0], file=sys.stderr)
    # multi-line heredocs survive only via stdin (argv quoting mangles \n)
    return subprocess.run(["ssh", *SSHOPTS, TGT, "ksh -s"],
                          input=cmd, capture_output=True, text=True, timeout=timeout)

def poll_result(tag, deadline_s=240):
    end = time.time() + deadline_s
    while time.time() < end:
        r = run(f'grep result /pps/system/installer/upd/current/job.tag 2>/dev/null')
        if "result::" in r.stdout:
            return r.stdout.strip()
        time.sleep(6)
    return "result::TIMEOUT"

def job(action, extra=""):
    bar = "/var/android/deploy.bar"
    tag = (f"@{ARGS.pkgid}\naction::{action}\npackage_id::{ARGS.pkgid}\n"
           f"package_name::{ARGS.pkgname}\n{extra}")
    run(f'rm -f /dev/shmem/sud_handover /pps/system/installer/upd/current/job.tag')
    run(f'cat > /pps/system/installer/upd/current/job.tag <<EOF2\n{tag}EOF2\n'
        f'chown upd:upd /pps/system/installer/upd/current/job.tag 2>/dev/null; '
        f'chmod 660 /pps/system/installer/upd/current/job.tag')
    run('on -d /base/usr/sbin/sud.sh >/var/tmp/sud.out 2>&1')
    return poll_result(ARGS.pkgid)

if ARGS.install or ARGS.uninstall:
    src = Path(ARGS.install or ARGS.uninstall)
    print(f"scp {src} -> {TGT}:/var/android/deploy.bar")
    scp = ["scp", "-O", *SSHOPTS, str(src), f"{TGT}:/var/android/deploy.bar"]
    if ARGS.key:
        pass  # -i already in SSHOPTS
    r = subprocess.run(scp, capture_output=True, text=True)
    if r.returncode != 0:
        print("SCP FAILED:", r.stderr[-400:]); sys.exit(1)
    run("chmod 777 /var/android/deploy.bar; rm -rf /apps/.new.* 2>/dev/null; mkdir -p /var/android")
    if ARGS.uninstall:
        print("uninstall:", job("uninstall"))
        run("rm -rf /apps/.new.* 2>/dev/null")
    print("install:", job("install", "package_location::/var/android/deploy.bar\nextras::nomedia\n"))
    # sandbox perms: installer preserves bar modes only for config; force-execute
    run(f'chmod 755 /apps/{ARGS.pkgname}.{ARGS.pkgid}/native/* '
        f'/apps/{ARGS.pkgname}.{ARGS.pkgid}/native/lib/*.so 2>/dev/null')
else:
    print("nothing to do (pass --install/--uninstall)")
