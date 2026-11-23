#!/bin/sh
# Kullanım: JPS=/yol/jps [BIN64=..] [BIN32=..] scripts/build-all.sh <dist> <trees>
set -eu
cd "$(dirname "$0")/.."
JPS=$(readlink -f "${JPS:?JPS=/yol/jps gerekli}")
OUT=${1:-dist}
TREES=${2:-trees}
TAB=$(printf '\t')
MAXFILE=$((95*1024*1024))
MAXTOTAL_MB=${MAXTOTAL_MB:-1200}
rm -rf "$OUT"
mkdir -p "$OUT/pool"
OUT=$(readlink -f "$OUT")
[ -d "$TREES" ] && TREES=$(readlink -f "$TREES")
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
IDX="$OUT/index.tmp"
: > "$IDX"
: > "$OUT/SKIPPED.txt"

field() { sed -n "s/^$2: *//p" "$1/JPS/control" | head -n1; }

# emit <paket dizini> <top|dep>
emit() {
  d=$1; flag=$2
  name=$(field "$d" Package); ver=$(field "$d" Version); arch=$(field "$d" Architecture)
  deps=$(field "$d" Depends | tr -d ' '); desc=$(field "$d" Description)
  if awk -F"$TAB" -v n="$name" -v a="$arch" '$1==n && $3==a {f=1} END{exit !f}' "$IDX"; then return 0; fi
  file="${name}_${ver}_${arch}.jps"
  if ! "$JPS" build "$d" -o "$OUT/pool/$file" >/dev/null 2>"$WORK/err"; then
    echo "$name ($arch): derlenemedi: $(tail -n1 "$WORK/err")" >> "$OUT/SKIPPED.txt"
    rm -f "$OUT/pool/$file"; return 0
  fi
  if [ -f "$HOME/.config/jps/signing.key" ]; then
    "$JPS" sign "$OUT/pool/$file" >/dev/null
  elif [ "${REQUIRE_SIGN:-0}" = 1 ]; then
    echo "HATA: imza anahtarı yok" >&2; exit 1
  fi
  "$JPS" verify "$OUT/pool/$file" >/dev/null
  size=$(wc -c < "$OUT/pool/$file" | tr -d ' ')
  if [ "$size" -gt "$MAXFILE" ]; then
    echo "$name ($arch): $size bayt, GitHub dosya sınırını aşıyor" >> "$OUT/SKIPPED.txt"
    rm -f "$OUT/pool/$file"; return 0
  fi
  sha=$(sha256sum "$OUT/pool/$file" | cut -d' ' -f1)
  printf '%s\t%s\t%s\t%s\t%s\tpool/%s\t%s\t%s\t%s\n' \
    "$name" "$ver" "$arch" "$size" "$sha" "$file" "$deps" "$desc" "$flag" >> "$IDX"
}

# 1) Debian'dan içe aktarılan paketler (trees/<mimari>/<paket>)
if [ -d "$TREES" ]; then
  for adir in "$TREES"/*/; do
    [ -f "${adir}top.list" ] || continue
    for d in "$adir"*/; do
      [ -d "${d}JPS" ] || continue
      n=$(basename "$d")
      if grep -qxF "$n" "${adir}top.list"; then flag=top; else flag=dep; fi
      emit "${d%/}" "$flag"
    done
  done
fi

# 2) Jubatus'un kendi gerçek paketleri (real/<ad>)
for src in real/*/; do
  [ -d "${src}JPS" ] || continue
  d="$WORK/real-$(basename "$src")"
  mkdir -p "$d"
  cp -a "${src}." "$d/"
  emit "$d" top
  rm -rf "$d"
done

# 3) jubatus-forge: jps-64 → amd64, jps-32 → i386
forge_pkg() {
  arch=$1; bin=$2
  if [ -z "$bin" ] || [ ! -f "$bin" ]; then echo "uyarı: jubatus-forge ($arch) atlandı (ikili yok)"; return 0; fi
  fver=$(sed -n 's/^version = "\(.*\)"/\1/p' forge/Cargo.toml | head -n1)
  d="$WORK/forge-$arch"
  mkdir -p "$d/JPS" "$d/root/usr/local/bin"
  cp "$bin" "$d/root/usr/local/bin/jps"
  chmod 755 "$d/root/usr/local/bin/jps"
  cp forge/assets/jps-logo.png "$d/JPS/icon.png"
  cat > "$d/JPS/control" <<CTL
Package: jubatus-forge
Version: $fver
Architecture: $arch
Maintainer: Jubatus Forge <packages@jubatus.local>
Depends:
Description: Jubatus Forge - .jps paket yöneticisi
CTL
  cat > "$d/JPS/postinst" <<'PI'
#!/bin/sh
[ "$JPS_ROOT" = "/" ] || exit 0
/usr/local/bin/jps desktop-install --system || true
PI
  cat > "$d/JPS/prerm" <<'PR'
#!/bin/sh
[ "$JPS_ROOT" = "/" ] || exit 0
/usr/local/bin/jps desktop-remove --system || true
PR
  chmod 755 "$d/JPS/postinst" "$d/JPS/prerm"
  # GEÇİŞ: jps 0.3.0 istemcileri zstd okuyamaz; kendini yükseltebilsin diye bu paket gzip.
  ( export JPS_COMPRESSION=gzip; emit "$d" top )
  rm -rf "$d"
}
forge_pkg amd64 "${BIN64:-}"
forge_pkg i386 "${BIN32:-}"

LC_ALL=C sort "$IDX" > "$OUT/index"
rm -f "$IDX"
[ -f signing.pub ] && cp signing.pub "$OUT/signing.pub"

awk -F'\t' '$9=="top" && ($3=="amd64"||$3=="all"){a++} $9=="top" && ($3=="i386"||$3=="all"){b++}
  END{print "amd64=" a+0; print "i386=" b+0}' "$OUT/index" > "$OUT/STATS.txt"
total=$(du -sm "$OUT/pool" | cut -f1)
echo "toplam satır: $(wc -l < "$OUT/index"), pool: ${total} MB"
cat "$OUT/STATS.txt"
if [ "$total" -gt "$MAXTOTAL_MB" ]; then
  echo "HATA: pool ${total} MB, sınır ${MAXTOTAL_MB} MB. packages.list'i küçült." >&2
  exit 1
fi
