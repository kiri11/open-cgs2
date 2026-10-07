# Picks open-cgs2 or the stock firmware for this boot. Sourced by the launcher in place of the stock
# start script (STOCK), with OEM set on firmware whose root is read-only, where open-cgs2 lives in
# /oem/open-cgs2 and is mounted over /opt. The stock firmware boots when the kill switch exists,
# when Settings asked for one stock boot (stock-once) or when open-cgs2 failed to come up
# MAX_BOOTS times in a row; supervisor.sh resets bootcount once the system is healthy.
STATE=/userdata/open-cgs2
MAX_BOOTS=3
STOCK=${STOCK:-/qingping/bin/start.sh}

mkdir -p $STATE
boots=$(( $(cat $STATE/bootcount 2>/dev/null || echo 0) + 1 ))
echo $boots > $STATE/bootcount
sync
once=$STATE/stock-once
if [ -e $once ] && rm $once || [ -e $STATE/stock ] || [ $boots -gt $MAX_BOOTS ]; then
  echo "$(date '+%F %T') stock boot $boots" >> $STATE/boot.log
  echo stock > /run/open-cgs2-mode
  . $STOCK
else
  echo "$(date '+%F %T') open-cgs2 boot $boots" >> $STATE/boot.log
  echo open-cgs2 > /run/open-cgs2-mode
  if [ -n "$OEM" ]; then
    # /root holds the SSH keys; stock keeps dropbear's host keys on tmpfs and makes new ones each boot.
    mkdir -p $OEM/root $OEM/dropbear /var/run/dropbear
    mount --bind $OEM/opt /opt
    mount --bind $OEM/root /root
    mount --bind $OEM/dropbear /var/run/dropbear
  fi
  ( sleep 300
    if [ "$(cat $STATE/bootcount)" != 0 ]; then
      echo "$(date '+%F %T') not healthy after 300 s, rebooting" >> $STATE/boot.log
      sync
      reboot
    fi ) </dev/null >/dev/null 2>&1 &
  sh /opt/open-cgs2/start.sh boot
fi
