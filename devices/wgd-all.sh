#!/bin/ksh
# wgd-all v13 (v12: payload in /accounts/devuser trusted dir; v13: consumes
#   native-app command files: /accounts/devuser/wgcmd_rebind, wgcmd_ft)
#   "tr: cannot execute" spam from fsize(); field-split via `set --` instead)
#
# Run as ROOT inside the g_nto/netio context (from install.sh):
#   run_as g_nto "ksh < /accounts/devuser/wgd-all.sh"
# or manually from a root shell:
#   on -d ksh < /accounts/devuser/wgd-all.sh </dev/null >/dev/null 2>&1
#
# v10 changes:
#   - FIX freeze-blindness (carried from v8/v9): stalled counter only grew while
#     the last WLOG line said "retransmit". A daemon that FREEZES (writes nothing)
#     left the last line on an old "handshake" -> stalled kept resetting to 0 and
#     an alive-but-frozen tunnel was never relaunched. Now a second counter tracks
#     WLOG size: unchanged for FZ_STALL polls (>=10 min) while tun0 is up -> force
#     relaunch. Works without `find -mmin` (pure wc -c), covers any silent freeze.
#   - FIX rm-race on stop: old code did blind `rm -f $PIDF` at exit; if a newer
#     watchdog had already claimed the pidfile, this deleted ITS file -> next
#     replace-block skipped a live instance -> two watchdogs coexist. PIDF now
#     holds "<pid> <gen-token>"; on exit the watchdog removes it ONLY if the gen
#     token still matches its own (inheritance via pre-fork variable; $$ inside a
#     ksh subshell is the parent pid, so pid-based identity does not work there).
#   - Guard: probe tail/wc once at start; if missing, rotation + freeze gate
#     degrade to logged no-ops instead of silent failure (BB10 shell lacks e.g.
#     printf — do not assume tool availability).
# v9 (kept): pidfile written by PARENT via $! (real watchdog pid; fixes the
#   v8 boot-time self-kill from $$-in-subshell + deterministic pid reuse);
#   replace-block validates numeric/!self/!PPID/alive/cmdline==ksh before kill
#   and always rm's a stale pidfile; 512K copytruncate log rotation.
#
# Stop: touch /accounts/devuser/wgd_watch.stop
# v3 layout: persisted binary at /var/rootdata/wgd.
# Revert: delete install.sh block + this file + wgd_watch.* files.

# ---------- configuration ----------
ENDPOINT_IP=130.162.151.113
DNS_VIA_TUN=193.123.244.77
WGD_STORE=/var/rootdata/wgd
# v12: trusted exec domain (see launch()): devuser-owned dir, survives reboot
WGD_RUN=/accounts/devuser/wgd
CONF=/accounts/devuser/wgd.conf
FULLTUN=/accounts/devuser/wgd_fulltunnel.on
PIDF=/accounts/devuser/wgd_watch.pid
STOP=/accounts/devuser/wgd_watch.stop
LOG=/accounts/devuser/wgd_watch.log
WLOG=/accounts/devuser/wgd.log
LOG_MAX=524288
LOG_KEEP=131072
# v14: watchdog's own log gets its OWN cap (512K->128K copytruncate)
WLOG_MAX=524288
WLOG_KEEP=131072
FZ_STALL=40          # polls of zero WLOG growth (15s each -> >=10 min) = frozen

log() { print "$(date) $1" >> $LOG; }

# ---------- tool probe (BB10 shell tooling is not guaranteed) ----------
HAVE_TAIL=1; HAVE_WC=1
type tail >/dev/null 2>&1 || { HAVE_TAIL=0; }
type wc   >/dev/null 2>&1 || { HAVE_WC=0; }

# ---------- helpers ----------
fsize() {
  [ "$HAVE_WC" = 1 ] || { print 0; return; }
  # BB10 has no tr: split wc output with set -- instead of `tr -d ' '`
  set -- $(wc -c <"$1" 2>/dev/null)
  sz=$1
  case "$sz" in ''|*[!0-9]*) sz=0 ;; esac
  print "$sz"
}

# copytruncate rotation: file inode/fd stays valid for the writer (append-mode).
# v14: per-file cap: rotate <file> <max> <keep> (was single global LOG_MAX, so
# wgd_watch.log shared the daemon's budget; watchdog log grows on every relaunch
# attempt -> it needs its own bound).
rotate() {
  [ "$HAVE_TAIL" = 1 ] && [ "$HAVE_WC" = 1 ] || return 0
  f=$1
  mx=$2
  kp=$3
  sz=$(fsize "$f")
  [ "$sz" -gt "$mx" ] || return 0
  tail -c "$kp" "$f" >"$f.r.$$" 2>/dev/null || { rm -f "$f.r.$$"; return 0; }
  cat "$f.r.$$" >"$f" 2>/dev/null
  rm -f "$f.r.$$"
}

logcheck() { rotate "$WLOG" "$LOG_MAX" "$LOG_KEEP"; rotate "$LOG" "$WLOG_MAX" "$WLOG_KEEP"; }

cur_gw() {
  g=$(route get -net default 2>/dev/null | sed -n 's/^ *gateway: //p')
  case "$g" in 10.0.10.*|"") g=10.0.1.1;; esac
  print "$g"
}

netfix() {
  route get -net 10.0.10.0/24 2>/dev/null | grep -q "interface: tun0" && return 0
  GW=$(cur_gw)
  route delete -net 10.0.10.0/24 10.0.10.8 2>/dev/null
  route delete -host 10.0.10.254 10.0.10.8 2>/dev/null
  route delete -host $DNS_VIA_TUN 10.0.10.8 2>/dev/null
  route delete -net 10.0.10.0/24 10.0.10.254 2>/dev/null
  route delete -host 10.0.10.254 10.0.10.254 2>/dev/null
  route delete -host $DNS_VIA_TUN 10.0.10.254 2>/dev/null
  route delete -host $ENDPOINT_IP 10.0.10.254 2>/dev/null
  route add -host $ENDPOINT_IP $GW 2>/dev/null
  route add -host 10.0.10.254 10.0.10.254 2>/dev/null
  route add -net 10.0.10.0/24 10.0.10.254 2>/dev/null
  route add -host $DNS_VIA_TUN 10.0.10.254 2>/dev/null
  if [ -f $FULLTUN ]; then
    if ! route get -net default 2>/dev/null | grep -q "interface: tun0"; then
      route delete -net default $GW 2>/dev/null
      route add -net default 10.0.10.254 2>/dev/null
      log "fulltunnel default restored via tun0 (GW=$GW)"
    fi
  fi
  route get -net 10.0.10.0/24 2>/dev/null | grep -q "interface: tun0"
}

fulltunnel_gate() {
  # Default-route takeover with ping-gated rollback (watchdog context).
  GW=$(cur_gw)
  route delete -net default $GW 2>/dev/null
  route add -net default 10.0.10.254 2>/dev/null
  defok=0
  sleep 20
  ping -c2 -i2 1.1.1.1 >/dev/null 2>&1 && defok=1
  [ $defok -eq 0 ] && { sleep 15; ping -c2 -i2 1.1.1.1 >/dev/null 2>&1 && defok=1; }
  log "fulltunnel defok=$defok"
  if [ $defok -ne 1 ]; then
    route delete -net default 10.0.10.254 2>/dev/null
    route add -net default $GW 2>/dev/null
    log "fulltunnel ROLLED BACK"
  fi
}

launch() {
  slay -fQ -sKILL wgd >/dev/null 2>&1
  # slay is name-based; g_nto-owned instances have name=g_nto -> kill by cmdline
  pidin ar 2>/dev/null | awk -v p="$WGD_RUN" '$2=="g_nto" && $3==p {print $1}' | \
    while read x; do kill -9 "$x" 2>/dev/null; done
  pidin ar 2>/dev/null | awk -v p="$WGD_RUN" '$3==p {print $1}' | \
    while read x; do kill -9 "$x" 2>/dev/null; done
  sleep 1
  # v12: payload lives in /accounts/devuser (PathTrust-trusted dir owned by
  # devuser). /var/tmp is `untrusted`: after a COLD boot (devmode off) even a
  # live sud window may refuse devuser exec of unsigned ELF there, while the
  # /accounts copy runs unattended. sud window hack removed.
  # NOTE: PathTrust also checks OWNER — a root-owned copy in /accounts is
  # still refused; chown to devuser after every cp.
  # v14: devuser uid is read LIVE (this device: devuser=100, racd=399; the
  # hardcoded 399 in v13.3 made every launch a false "owner drift" -> rewrite
  # churn). Repair only on REAL drift, content-preserving redirect.
  DU_UID=$(sed -n 's/^devuser:[^:]*:\([0-9]*\):.*/\1/p' /etc/passwd 2>/dev/null)
  [ -n "$DU_UID" ] || DU_UID=100
  O=$(ls -ln $WGD_RUN 2>/dev/null | awk '{print $3}')
  if [ "$O" != "$DU_UID" ]; then
    if [ -f "$WGD_RUN" ]; then
      cat $WGD_STORE > $WGD_RUN 2>/dev/null
    else
      cp -f $WGD_STORE $WGD_RUN 2>/dev/null
    fi
    chmod 755 $WGD_RUN 2>/dev/null
    chown devuser:devuser $WGD_RUN 2>/dev/null
    O=$(ls -ln $WGD_RUN 2>/dev/null | awk '{print $3}')
    [ "$O" = "$DU_UID" ] || log "launch WARN: wgd owner=$O (want $DU_UID devuser)"
  fi
  chmod 755 $WGD_RUN 2>/dev/null
  chmod 644 $CONF 2>/dev/null
  # v13.1 ORDERING FIX: io-pkt kills sockets opened on a destroyed node with
  # EBADMSG(77). Bring tun0 fully UP + routed BEFORE starting the daemon, so
  # wgd's UDP socket is born on a live interface (err=77 storm root cause).
  ifconfig tun0 destroy 2>/dev/null   # fresh node: old one may be 0600/held-open
  sleep 1
  ifconfig tun0 create 2>/dev/null
  chmod 666 /dev/tun0 2>/dev/null     # AFTER create (new node defaults 0600)
  ifconfig tun0 10.0.10.8 10.0.10.254 netmask 255.255.255.0 up 2>/dev/null
  netfix
  on -d -u devuser $WGD_RUN </dev/null >/dev/null 2>&1
  sleep 6
  # v12.2: self-diagnosis — when the watchdog's own launch dies, capture why
  # v14: exact-column awk match was false-DEAD on this device (pidin column
  # layout varies); grep the full cmdline like the main loop does.
  if ! pidin ar 2>/dev/null | grep -q "$WGD_RUN"; then
    O=$(ls -l $WGD_RUN 2>/dev/null)
    N=$(pidin ar 2>/dev/null | grep -c devuser)
    log "launch DEAD: file[$O] devuserprocs=$N tun[$(ls -l /dev/tun0 2>/dev/null)]"
  fi
  [ -f $FULLTUN ] && fulltunnel_gate
}

# ---------- replace previous watchdog instance ----------
# NEVER trust a surviving pidfile blindly: pid slots are reused across boots.
# PIDF format: "<pid> <gen-token>" (v9-compatible first field = pid).
if [ -f $PIDF ]; then
  read oldpid oldgen <"$PIDF" 2>/dev/null
  case "$oldpid" in
    ''|*[!0-9]*) : ;;                       # garbage -> just drop the file
    *)
      # Kill only a live ksh that is definitely not us/our parent.
      if [ "$oldpid" != "$$" ] && [ "$oldpid" != "${PPID:-0}" ] && \
         [ -e /proc/$oldpid/as ] && \
         pidin ar 2>/dev/null | awk -v p="$oldpid" '$1==p && $2=="ksh" {f=1} END{exit !f}'; then
        kill -9 "$oldpid" 2>/dev/null
        log "replaced live watchdog pid=$oldpid"
      fi
      ;;
  esac
  rm -f $PIDF
fi
rm -f $STOP
rm -f $WLOG.r.* $LOG.r.* 2>/dev/null   # leftovers from an interrupted rotation
logcheck
log "wgd-all v12 BOOT-BEGIN (no launch at boot; watch owns it)"
[ "$HAVE_TAIL" = 1 ] && [ "$HAVE_WC" = 1 ] || log "WARN: tail/wc missing -> rotation+freeze gate disabled"

# generation token: identifies THIS watchdog for race-safe pidfile removal.
# Set BEFORE the fork so the subshell inherits it ($$ inside a ksh subshell is
# the parent's pid, so pid-based identity does not work there).
GEN="$$.$RANDOM$RANDOM"

# ---------- watchdog: the only launch path ----------
(
  log "watch start pid=$$ gen=$GEN"
  stalled=0
  froze=0
  prev_sz=-1
  # native-app command files (pre-created 666; app truncates+writes one word)
CMD_REBIND=/accounts/devuser/wgcmd_rebind
CMD_FT=/accounts/devuser/wgcmd_ft

cmdcheck() {
  # ensure command files exist (world-writable) for the native app (uid apps)
  [ -e "$CMD_REBIND" ] || : > "$CMD_REBIND"
  [ -e "$CMD_FT" ] || : > "$CMD_FT"
  chmod 666 "$CMD_REBIND" "$CMD_FT" 2>/dev/null
  if [ -s "$CMD_REBIND" ]; then
    log "cmd: rebind -> relaunch"
    : > "$CMD_REBIND" 2>/dev/null || rm -f "$CMD_REBIND" 2>/dev/null
    launch; stalled=0; froze=0; prev_sz=-1
    return 0
  fi
  if [ -s "$CMD_FT" ]; then
    C=$(cat "$CMD_FT" 2>/dev/null)
    : > "$CMD_FT" 2>/dev/null || rm -f "$CMD_FT" 2>/dev/null
    case "$C" in
      fton*)
        log "cmd: fton -> fulltunnel_gate"
        touch "$FULLTUN" 2>/dev/null
        fulltunnel_gate ;;
      ftoff*)
        log "cmd: ftoff -> restore default route"
        rm -f "$FULLTUN" 2>/dev/null
        GW=$(cur_gw)
        route delete -net default 10.0.10.254 2>/dev/null
        route add -net default "$GW" 2>/dev/null ;;
    esac
    return 0
  fi
  return 1
}

while [ ! -f $STOP ]; do
    sleep 15
    logcheck
    cmdcheck && continue
    # gate: real (non-tun) default GW must exist before any launch
    g=$(route get -net default 2>/dev/null | sed -n 's/^ *gateway: //p')
    case "$g" in ""|10.0.10.*) continue ;; esac
    ifconfig tun0 >/dev/null 2>&1 || { log "launch: net up, tun0 gone"; launch; stalled=0; froze=0; prev_sz=-1; continue; }
    pidin ar 2>/dev/null | grep -q "$WGD_RUN"'$' || { log "recovery: wgd missing"; launch; stalled=0; froze=0; prev_sz=-1; continue; }
    # alive-but-dead A: last log line stuck on retransmit for >=4 polls (60s)
    last=$(tail -1 $WLOG 2>/dev/null)
    case "$last" in
      *retransmit*)
        stalled=$((stalled+1))
        if [ $stalled -ge 4 ]; then
          log "recovery: stalled in retransmit x4 -> launch"
          launch; stalled=0; froze=0; prev_sz=-1; continue
        fi ;;
      *) stalled=0 ;;
    esac
    # alive-but-dead B: silent freeze — DAEMON log ($LOG, heartbeat every 60s)
    # not growing while proc alive. v13.1 fix: v13 measured $WLOG (watchdog's
    # own log, quiet by design) -> guaranteed false "frozen" relaunch storm.
    if [ "$HAVE_WC" = 1 ]; then
      sz=$(fsize "$LOG")
      if [ "$sz" = "$prev_sz" ]; then
        froze=$((froze+1))
        if [ $froze -ge $FZ_STALL ]; then
          log "recovery: wgd.log frozen ${froze} polls (sz=$sz) -> launch"
          launch; stalled=0; froze=0; prev_sz=-1; continue
        fi
      else
        froze=0
      fi
      prev_sz=$sz
    fi
    netfix || { log "recovery: netfix failed -> launch"; launch; }
  done
  log "watch stop"
  # race-safe: remove pidfile ONLY if it still belongs to this instance
  if read cur_pid cur_gen <"$PIDF" 2>/dev/null && [ "$cur_gen" = "$GEN" ]; then
    rm -f $PIDF
  fi
) </dev/null >/dev/null 2>&1 &
watchpid=$!
# Parent records the REAL subshell pid ($! is exact; $$ inside ksh subshell
# would have been the parent's pid — that was the v8 self-kill root cause).
print "$watchpid $GEN" > $PIDF
logcheck
log "wgd-all v12 BOOT-DONE (watchdog pid=$watchpid detached, boot chain free)"
