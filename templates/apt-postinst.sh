#!/bin/sh
set -e
[ "$JPS_ROOT" = "/" ] || { echo "sandbox kökü: apt adımı atlandı"; exit 0; }
APT_PKG="__APT__"
command -v apt-get >/dev/null 2>&1 || { echo "apt-get bulunamadı: Debian/Ubuntu tabanlı sistem gerekir" >&2; exit 1; }
MARK=/var/lib/jps-apt
mkdir -p "$MARK"
if dpkg -s "$APT_PKG" >/dev/null 2>&1; then
  echo "$APT_PKG zaten sistemde kurulu"
else
  export DEBIAN_FRONTEND=noninteractive
  apt-get install -y "$APT_PKG" || { apt-get update && apt-get install -y "$APT_PKG"; }
  : > "$MARK/$JPS_PACKAGE.ours"
fi
