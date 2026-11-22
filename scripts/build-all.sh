#!/bin/sh
# Kullanım: JPS=/yol/jps [BIN64=/yol/jps-64] [BIN32=/yol/jps-32] scripts/build-all.sh <çıktı-dizini>
set -eu
cd "$(dirname "$0")/.."
JPS=$(readlink -f "${JPS:?JPS=/yol/jps gerekli}")
OUT=${1:-dist}
VERSION=${PKG_VERSION:-1.0.0}
MIN=150
TAB=$(printf '\t')
rm -rf "$OUT"
mkdir -p "$OUT/pool"
OUT=$(readlink -f "$OUT")
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
IDX="$OUT/index.tmp"
: > "$IDX"

field() { sed -n "s/^$2: *//p" "$1/JPS/control" | head -n1; }

# emit <hazır paket dizini>: üretir, imzalar, doğrular, indekse yazar
emit() {
  d=$1
  name=$(field "$d" Package); ver=$(field "$d" Version); arch=$(field "$d" Architecture)
  deps=$(field "$d" Depends | tr -d ' '); desc=$(field "$d" Description)
  if awk -F"$TAB" -v n="$name" -v a="$arch" '$1==n && $3==a {f=1} END{exit !f}' "$IDX"; then
    echo "HATA: yinelenen paket: $name ($arch)" >&2; exit 1
  fi
  file="${name}_${ver}_${arch}.jps"
  "$JPS" build "$d" -o "$OUT/pool/$file" >/dev/null
  if [ -f "$HOME/.config/jps/signing.key" ]; then
    "$JPS" sign "$OUT/pool/$file" >/dev/null
  elif [ "${REQUIRE_SIGN:-0}" = 1 ]; then
    echo "HATA: imza anahtarı yok" >&2; exit 1
  fi
  "$JPS" verify "$OUT/pool/$file" >/dev/null
  size=$(wc -c < "$OUT/pool/$file" | tr -d ' ')
  sha=$(sha256sum "$OUT/pool/$file" | cut -d' ' -f1)
  printf '%s\t%s\t%s\t%s\t%s\tpool/%s\t%s\t%s\n' \
    "$name" "$ver" "$arch" "$size" "$sha" "$file" "$deps" "$desc" >> "$IDX"
}

# 1) apt sarmalayıcıları (packages.list)
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
  emit "$d"
  rm -rf "$d"
done < packages.list

# 2) gerçek dosya taşıyan paketler (real/<ad>/JPS + root)
for src in real/*/; do
  [ -d "${src}JPS" ] || continue
  d="$WORK/real-$(basename "$src")"
  mkdir -p "$d"
  cp -a "${src}." "$d/"
  emit "$d"
  rm -rf "$d"
done

# 3) jubatus-forge (jps'in kendisi, mimariye göre)
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
  ( export JPS_COMPRESSION=gzip; emit "$d" )
  rm -rf "$d"
}
forge_pkg amd64 "${BIN64:-}"
forge_pkg i386 "${BIN32:-}"

n=$(wc -l < "$IDX" | tr -d ' ')
if [ "$n" -lt "$MIN" ]; then
  echo "HATA: yalnızca $n paket üretildi (en az $MIN gerekli)" >&2
  exit 1
fi
LC_ALL=C sort "$IDX" > "$OUT/index"
rm -f "$IDX"
[ -f signing.pub ] && cp signing.pub "$OUT/signing.pub"
echo "✔ $n paket üretildi → $OUT"
