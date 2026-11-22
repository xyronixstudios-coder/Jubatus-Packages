#!/bin/sh
[ "$JPS_ROOT" = "/" ] || exit 0
M="/var/lib/jps-apt/$JPS_PACKAGE.ours"
if [ -f "$M" ]; then
  DEBIAN_FRONTEND=noninteractive apt-get remove -y "__APT__" || true
  rm -f "$M"
fi
