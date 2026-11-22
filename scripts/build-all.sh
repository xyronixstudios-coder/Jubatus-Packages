#!/bin/sh
# Kullanım: JPS=/yol/jps scripts/build-all.sh <çıktı-dizini>
set -eu
cd "$(dirname "$0")/.."
JPS=$(readlink -f "${JPS:?JPS=/yol/jps gerekli}")
OUT=${1:-dist}
VERSION=${PKG_VERSION:-1.0.0}
MIN=150
rm -rf "$OUT"
mkdir -p "$OUT/pool"
OUT=$(readlink -f "$OUT")
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
: > "$OUT/index.tmp"
n=0
while IFS='|' read -r name desc deps apt; do
  case "$name" in ''|'#'*) continue ;; esac
  apt=${apt:-$name}
  deps=$(printf '%s' "$deps" | tr -d ' ')
  d="$WORK/$name"
  mkdir -p "$d/JPS" "$d/root/usr/share/jps/meta"
  cat > "$d/JPS/control" <<CTL
Package: $name
Version: $VERSION
Architecture: all
Maintainer: Jubatus Forge <packages@jubatus.local>
Depends: $deps
Description: $desc
CTL
  printf '%s\n' "$apt" > "$d/root/usr/share/jps/meta/$name"
  sed "s|__APT__|$apt|g" templates/apt-postinst.sh > "$d/JPS/postinst"
  sed "s|__APT__|$apt|g" templates/apt-postrm.sh > "$d/JPS/postrm"
  chmod 755 "$d/JPS/postinst" "$d/JPS/postrm"
  file="${name}_${VERSION}_all.jps"
  "$JPS" build "$d" -o "$OUT/pool/$file" >/dev/null
  if [ -f "$HOME/.config/jps/signing.key" ]; then
    "$JPS" sign "$OUT/pool/$file" >/dev/null
  elif [ "${REQUIRE_SIGN:-0}" = 1 ]; then
    echo "HATA: imza anahtarı yok" >&2; exit 1
  fi
  "$JPS" verify "$OUT/pool/$file" >/dev/null
  size=$(wc -c < "$OUT/pool/$file" | tr -d ' ')
  sha=$(sha256sum "$OUT/pool/$file" | cut -d' ' -f1)
  printf '%s\t%s\tall\t%s\t%s\tpool/%s\t%s\t%s\n' \
    "$name" "$VERSION" "$size" "$sha" "$file" "$deps" "$desc" >> "$OUT/index.tmp"
  n=$((n+1))
  rm -rf "$d"
done < packages.list

if [ "$n" -lt "$MIN" ]; then
  echo "HATA: yalnızca $n paket üretildi (en az $MIN gerekli)" >&2
  exit 1
fi
LC_ALL=C sort "$OUT/index.tmp" > "$OUT/index"
rm -f "$OUT/index.tmp"
[ -f signing.pub ] && cp signing.pub "$OUT/signing.pub"
echo "✔ $n paket üretildi → $OUT"
