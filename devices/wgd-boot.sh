#!/bin/ksh
# wgd-boot.sh v6 (2026-10-02) - run as ROOT (via run_as g_nto from install.sh,
# or manually: on -d ksh < /accounts/devuser/wgd-boot.sh)
# Full tunnel assembly + watchdog spawn. Logs: /accounts/devuser/wgd_boot.log
L=/accounts/devuser/wgd_boot.log
echo "$(date) BOOT-BEGIN" >> $L
ENDPOINT_IP=130.162.151.113
slay -fQ -sKILL wgd 2>/dev/null
sleep 1
cp -f /accounts/devuser/rootdata/wgd /var/tmp/wgd && chmod 755 /var/tmp/wgd
chmod 644 /accounts/devuser/wgd.conf 2>/dev/null
ifconfig tun0 create 2>/dev/null
chmod 666 /dev/tun0 2>/dev/null
on -d -u devuser /var/tmp/wgd </dev/null >/dev/null 2>&1
sleep 7
ifconfig tun0 10.0.10.8 10.0.10.254 netmask 255.255.255.0 up 2>/dev/null
GW=$(route get -net default 2>/dev/null | sed -n 's/^ *gateway: //p')
case "$GW" in 10.0.10.*|"") GW=10.0.1.1;; esac
route delete -host $ENDPOINT_IP 10.0.10.254 2>/dev/null
route add -host $ENDPOINT_IP $GW 2>/dev/null
route add -host 10.0.10.254 10.0.10.254 2>/dev/null
route add -net 10.0.10.0/24 10.0.10.254 2>/dev/null
route add -host 193.123.244.77 10.0.10.254 2>/dev/null
echo "$(date) GW=$GW assembled" >> $L
if [ -f /accounts/devuser/wgd_fulltunnel.on ]; then
  route delete -net default $GW 2>/dev/null
  route add -net default 10.0.10.254 2>/dev/null
  defok=0
  sleep 20
  ping -c2 -i2 1.1.1.1 >/dev/null 2>&1 && defok=1
  [ $defok -eq 0 ] && { sleep 15; ping -c2 -i2 1.1.1.1 >/dev/null 2>&1 && defok=1; }
  echo "$(date) fulltunnel defok=$defok" >> $L
  if [ $defok -ne 1 ]; then
    route delete -net default 10.0.10.254 2>/dev/null
    route add -net default $GW 2>/dev/null
    echo "$(date) fulltunnel ROLLED BACK" >> $L
  fi
fi
on -d ksh < /accounts/devuser/wgd-watch.sh >/dev/null 2>&1
echo "$(date) BOOT-DONE" >> $L
