#!/bin/bash
# Bir Debian paketinin bakımcı betiklerini kurmadan gösterir (override yazmadan önce incele).
# Kullanım: scripts/show-scripts.sh tcpdump iperf3 ...
set -u
for p in "$@"; do
  T=$(mktemp -d)
  ( cd "$T" && apt-get download "$p" >/dev/null 2>&1 ) || { echo "indirilemedi: $p"; rm -rf "$T"; continue; }
  f=$(ls "$T"/*.deb 2>/dev/null | head -n1)
  mkdir "$T/c"; dpkg-deb -e "$f" "$T/c"
  echo "════════ $p ════════"
  for s in preinst postinst prerm postrm; do
    [ -f "$T/c/$s" ] && { echo "── $s ──"; cat "$T/c/$s"; }
  done
  rm -rf "$T"
done
