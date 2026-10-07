#!/bin/sh
# Keeps sensord, the screen (panel and qmlscene), wpa_supplicant, dhcpcd and, on firmware without
# one, ntpd running and marks the boot healthy (bootcount=0) once sensord has stayed up for HEALTHY_AFTER seconds.
. /etc/profile

STATE=/userdata/open-cgs2
LOG=$STATE/supervisor.log
OPT=/opt/open-cgs2
UI=$OPT/ui
HEALTHY_AFTER=60

export LC_ALL=zh_CN.utf8 XDG_RUNTIME_DIR=/var/run QT_QPA_FONTDIR=/usr/share/fonts
export QT_PLUGIN_PATH=$OPT/qt/plugins
export QT_QPA_EGLFS_ALWAYS_SET_MODE=1 QT_QPA_EGLFS_KMS_ATOMIC=1
export QT_QPA_EGLFS_WIDTH=720 QT_QPA_EGLFS_HEIGHT=720 QT_QPA_EGLFS_PHYSICAL_WIDTH=720 QT_QPA_EGLFS_PHYSICAL_HEIGHT=720

echo $$ > /run/open-cgs2-supervisor.pid
mkdir -p $STATE

log() {
  echo "$(date '+%F %T') $*" >> $LOG
  [ "$(wc -c < $LOG)" -lt 262144 ] || mv $LOG $LOG.1
}

# Keeps the tail of a log that a running process appends to.
trim() {
  [ "$(wc -c < $1 2>/dev/null || echo 0)" -gt 262144 ] || return 0
  tail -c 65536 $1 > $1.tmp && cat $1.tmp > $1
  rm -f $1.tmp
}

now() { cut -d. -f1 /proc/uptime; }

# Files in /userdata can lose their executable bit across a reboot on some firmware.
chmod 755 $OPT/sensord $OPT/panel 2>/dev/null

start_sensord() {
  log "starting sensord"
  $OPT/sensord </dev/null >/dev/null 2>>$STATE/sensord.log &
  sensord_since=$(now)
}

start_panel() {
  log "starting panel"
  $OPT/panel $STATE/config.json </dev/null >/dev/null 2>>$STATE/panel.log &
}

start_screen() {
  log "starting screen"
  mkdir -p -m 700 /run/qt
  XDG_RUNTIME_DIR=/run/qt qmlscene -platform eglfs $UI/qml/main.qml </dev/null >/dev/null 2>>$STATE/screen.log &
  screen_since=$(now)
}

log "supervisor started, mode $(cat /run/open-cgs2-mode 2>/dev/null)"
screen_since=0
sensord_since=$(now)
restarts=0
window_start=$(now)

while :; do
  t=$(now)

  # sensord feeds the hardware watchdog, so a sensord that cannot start ends in a reboot.
  if ! pidof sensord >/dev/null; then
    [ $((t - window_start)) -lt 300 ] || { window_start=$t; restarts=0; }
    restarts=$((restarts + 1))
    if [ $restarts -gt 5 ]; then
      log "sensord keeps exiting, waiting 60 s"
      sleep 60
    fi
    start_sensord
  elif [ "$(cat $STATE/bootcount 2>/dev/null)" != 0 ] && [ $((t - sensord_since)) -ge $HEALTHY_AFTER ]; then
    echo 0 > $STATE/bootcount
    sync
    log "healthy"
  fi

  if [ -x $OPT/panel ] && [ -f $STATE/config.json ]; then
    pidof panel >/dev/null || start_panel
    if ! pidof qmlscene >/dev/null && [ $((t - screen_since)) -ge 10 ]; then
      start_screen
    fi
  fi
  trim $STATE/panel.log
  trim $STATE/screen.log
  trim $STATE/sensord.log

  if ! pidof wpa_supplicant >/dev/null && ! grep -q wifi_disabled=true /data/etc/setting.ini; then
    log "restarting wpa_supplicant"
    wpa_supplicant -Dnl80211 -iwlan0 -c/data/cfg/wpa_supplicant.conf </dev/null >/dev/null 2>&1 &
  fi
  if ! pidof dhcpcd >/dev/null; then
    log "restarting dhcpcd"
    /etc/init.d/S41dhcpcd start </dev/null >/dev/null 2>&1
  fi
  if [ -x $OPT/ntpd ] && ! pidof ntpd >/dev/null; then
    log "starting ntpd"
    $OPT/ntpd -n -p "$(cat $STATE/ntp-server 2>/dev/null || echo pool.ntp.org)" </dev/null >/dev/null 2>&1 &
  fi

  sleep 5
done
