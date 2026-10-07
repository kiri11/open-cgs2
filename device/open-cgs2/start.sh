#!/bin/sh
# open-cgs2's boot path: the hardware bring-up from /qingping/bin/start.sh without the stock app,
# the vendor watchdog, the Mi Home client (which drops Wi-Fi when its cloud is unreachable) and the
# noise sampler, then SSH (keys only) and the supervisor.
# Usage: start.sh boot | supervisor
. /etc/profile

start_supervisor() {
  [ -f /run/open-cgs2-supervisor.pid ] && kill "$(cat /run/open-cgs2-supervisor.pid)" 2>/dev/null
  setsid sh /opt/open-cgs2/supervisor.sh </dev/null >/dev/null 2>&1 &
}

case "$1" in
  supervisor) start_supervisor; exit 0 ;;
  boot) ;;
  *) echo "Usage: $0 boot|supervisor"; exit 1 ;;
esac

chmod 666 /data/* -R
hwclock -s -u

mkdir -p /data/etc/wifi
cp /qingping/data/* /data/
cp /qingping/etc/asound.conf /etc
mkdir -p /data/bin/tools
cp /qingping/bin/tools/* /data/bin/tools/
chmod +x /data/bin/ -R

[ -f /qingping/etc/product.conf ] ||
  vendor_storage -r VENDOR_CUSTOM_ID_1B -t file -i /qingping/etc/product.conf

echo 1 > /sys/bus/i2c/devices/1-0062/co2_scd40_reinit
sleep 1
/qingping/bin/enableWiFiPower.sh
sleep 1
/qingping/bin/skw_insmod.sh

amixer sset 'Playback Path' SPK
amixer sset 'Capture MIC Path' 'Main Mic'

# LAN-only mode (the installer's --lan-only) also keeps IPv6 off; dhcpcd.conf has nogateway.
if [ -e /userdata/open-cgs2/lan-only ]; then
  for f in all default lo; do echo 1 > /proc/sys/net/ipv6/conf/$f/disable_ipv6; done
fi

grep -q wifi_disabled=true /data/etc/setting.ini ||
  wpa_supplicant -Dnl80211 -iwlan0 -c/data/cfg/wpa_supplicant.conf &

killall dropbear 2>/dev/null
dropbear -R -s -T 3

start_supervisor
