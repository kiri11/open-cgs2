#!/bin/sh
# open-cgs2's boot hook on firmware whose root is read-only: the read-only S50launcher sources
# /qingping/bin/start.sh (on /oem), so the installer moves the stock script to start-stock.sh and
# puts this in its place.
if [ -f /oem/open-cgs2/opt/open-cgs2/boot.sh ]; then
  STOCK=/oem/bin/start-stock.sh
  OEM=/oem/open-cgs2
  . /oem/open-cgs2/opt/open-cgs2/boot.sh
else
  . /oem/bin/start-stock.sh
fi
