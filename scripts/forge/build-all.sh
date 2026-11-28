#!/bin/bash
# Jubatus Forge: tarif (recipe) derleyici.
# Dağıtımdan bağımsız, STATİK paket ağaçları üretir: <çıktı>/<paket>/{JPS/control,root/...}
# CI'da Alpine konteynerinde (musl) veya Ubuntu çalıştırıcıda (çapraz derleme) çalışır. Yerelde de çalışır.
#   FORGE_ARCH=amd64|i386|all   hedef mimari etiketi (zorunlu)
#   FORGE_PHASES="c gnu rust go fonts snap"   çalışacak aşamalar
#   FORGE_STRICT=1              pinlenmemiş el yazısı tarif kaynağını reddet (CI)
#   FORGE_ONLY="a b c"          yalnızca bu paket adlarını derle
#   FORGE_SHARDS=N FORGE_SHARD=k   katalog girdilerini N parçaya böl (C tarifleri 0. parçada)
#   FORGE_BUDGET_MIN=dk         bu süreden sonra yeni tarife başlama
#   FORGE_RUST_MODE=cross FORGE_RUST_TARGET=<hedef>   Rust'ı 'cross' ile crates.io kaynağından derle
#   FORGE_GOARCH=386            Go'yu çapraz derle
#   FORGE_APPEND=1              mevcut top.list/skipped.txt'e ekle
set -uo pipefail
export LC_ALL=C
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ARCH=${FORGE_ARCH:?FORGE_ARCH gerekli (amd64|i386|all)}
[ $# -ge 1 ] || { echo "kullanım: build-all.sh <çıktı-dizini>" >&2; exit 2; }
mkdir -p "$1"; OUT=$(readlink -f "$1")
PREFIX=/usr/local
WORK=${FORGE_WORK:-/forge/work}
DL=${FORGE_DL:-/forge/dl}
SYSROOT=${FORGE_SYSROOT:-/forge/sysroot}
LOCK="$ROOT/recipes.lock"
JOBS=$(nproc 2>/dev/null || echo 2)
SHARDS=${FORGE_SHARDS:-0}
SHARD=${FORGE_SHARD:-0}
BUDGET=${FORGE_BUDGET_MIN:-0}
START=$SECONDS
UA="jubatus-forge (https://github.com/xyronixstudios-coder/Jubatus-Packages)"
mkdir -p "$WORK" "$DL" "$SYSROOT" "$OUT/_logs"
touch "$LOCK"
if [ -z "${FORGE_APPEND:-}" ]; then : > "$OUT/top.list"; : > "$OUT/skipped.txt"; : > "$OUT/allowed.txt"; fi
touch "$OUT/top.list" "$OUT/skipped.txt" "$OUT/allowed.txt"

export PREFIX JOBS SYSROOT FORCE_UNSAFE_CONFIGURE=1 MAKEINFO=true
export CFLAGS="-O2 -pipe" CXXFLAGS="-O2 -pipe"
export CPPFLAGS="-I$SYSROOT$PREFIX/include"
export LDFLAGS="-static -L$SYSROOT$PREFIX/lib"
export PKG_CONFIG="pkg-config --static"
export PKG_CONFIG_LIBDIR="$SYSROOT$PREFIX/lib/pkgconfig"
export PKG_CONFIG_SYSROOT_DIR="$SYSROOT"

want()        { [ -z "${FORGE_ONLY:-}" ] && return 0; case " $FORGE_ONLY " in *" $1 "*) return 0 ;; esac; return 1; }
mine()        { [ -n "${FORGE_ONLY:-}" ] && return 0; [ "$SHARDS" -le 1 ] && return 0; [ $(( $1 % SHARDS )) -eq "$SHARD" ]; }
out_of_time() { [ "$BUDGET" -gt 0 ] && [ $(( SECONDS - START )) -ge $(( BUDGET * 60 )) ]; }
has_phase()   { case " ${FORGE_PHASES:-c gnu rust go} " in *" $1 "*) return 0 ;; esac; return 1; }
say_ok()      { echo "$1" >> "$OUT/top.list"; echo "  ✔ $1 ${2:-}"; }
say_skip()    { echo "$1: $2" >> "$OUT/skipped.txt"; echo "  ✘ $1: $2"; }
errtail()     { { grep -m1 -iE 'error|cannot |requires |could not|panicked|failed' "$OUT/_logs/$1.log" 2>/dev/null || tail -n 2 "$OUT/_logs/$1.log" 2>/dev/null; } | tr '\n\t' '  ' | cut -c1-200; }
show_fail()   { tail -n 8 "$OUT/_logs/$1.log" 2>/dev/null | cut -c1-200 | sed 's/^/      | /'; }
valid_pkg()   { case "$1" in ''|*[!a-z0-9+.-]*|[!a-z0-9]*) return 1 ;; esac; [ "${#1}" -ge 2 ]; }
nonempty()    { [ -n "$(find "$1" \( -type f -o -type l \) 2>/dev/null | head -n1)" ]; }
tokens()      { sed 's/#.*//' "$1" | tr -s ' \t\r\n' '\n' | sed '/^$/d'; }
font_pkg()    { printf 'nerd-font-%s' "$(printf '%s' "$1" | tr 'A-Z_' 'a-z-')"; }

write_control() {   # ad sürüm bağımlılıklar açıklama kaynak-url kaynak-sha
  local desc
  desc=$(printf '%s' "$4" | tr '\n\t' '  ')
  {
    echo "Package: $1"
    echo "Version: $2"
    echo "Architecture: $ARCH"
    echo "Maintainer: Jubatus Forge <packages@jubatus.local>"
    echo "Depends: $3"
    echo "Description: $desc"
    echo "Libc: static"
    if [ -n "$5" ]; then echo "Source: $5"; fi
    if [ -n "$6" ]; then echo "Source-SHA256: $6"; fi
  } > "$OUT/$1/JPS/control"
}

fix_links() {
  find "$1" -type l | while IFS= read -r l; do
    t=$(readlink "$l")
    case "$t" in "$1"*) ln -sf "${t#"$1"}" "$l" ;; esac
  done
}

prune() {
  local r=$1$PREFIX
  rm -rf "$r/include" "$r/share/info" "$r/share/locale" "$r/share/doc" "$r/lib/pkgconfig" \
         "$r/share/aclocal" "$r/share/pkgconfig" "$r/share/gtk-doc"
  find "$r" \( -name '*.a' -o -name '*.la' \) -delete 2>/dev/null || true
  find "$r/bin" "$r/sbin" -type f -exec strip --strip-unneeded {} + 2>/dev/null || true
  find "$1" -mindepth 1 -depth -type d -empty -delete 2>/dev/null || true
}

cargo_desc() {      # Cargo.toml yolu -> tek satır açıklama
  local f=$1 d=""
  [ -f "$f" ] || return 0
  d=$(sed -n 's/^description *= *"\(.*\)" *$/\1/p' "$f" | head -n1 | tr -d '\\')
  case "$d" in '"'*) d="" ;; esac
  if [ -z "$d" ]; then
    d=$(awk '/^description *= *"""/{f=1; next} f && /"""/ {exit} f {printf "%s ", $0}' "$f" | sed 's/[[:space:]]*$//; s/\\//g')
  fi
  printf '%s' "$d"
}

default_build()   { ./configure --prefix="$PREFIX" $configure_flags && make -j"$JOBS"; }
default_package() { make DESTDIR="$PKGDIR" install; }

run_recipe() {      # $1 = tarif dosyası, $2 = beklenen ad
  (
    set -euo pipefail
    name=""; version=""; desc=""; src_url=""; deps=""; lib=no; sysroot=no; configure_flags=""; pin=yes
    . "$1"
    : "${name:?}" "${version:?}" "${desc:?}" "${src_url:?}"
    [ "$name" = "$2" ] || { echo "tarif adı klasörle uyuşmuyor"; exit 1; }
    exec > "$OUT/_logs/$name.log" 2>&1
    d="$WORK/$name-$version"; rm -rf "$d"; mkdir -p "$d"
    f="$DL/$(basename "$src_url")"
    if [ ! -s "$f" ]; then
      curl -fsSL --retry 3 --connect-timeout 20 -o "$f.part" "$src_url"
      mv "$f.part" "$f"
    fi
    got=$(sha256sum "$f" | cut -d' ' -f1)
    if [ "$pin" = no ]; then
      echo "pinsiz kaynak (HTTPS ile alındı): $src_url sha256=$got"
    else
      want_sha=$(awk -v u="$src_url" '$1==u {print $2; exit}' "$LOCK")
      if [ -z "$want_sha" ]; then
        if [ "${FORGE_STRICT:-0}" = 1 ]; then echo "pinlenmemiş kaynak: $src_url (scripts/forge/pin.sh çalıştır)"; exit 1; fi
        echo "$src_url $got" >> "$LOCK"
      elif [ "$want_sha" != "$got" ]; then
        echo "SHA256 UYUŞMUYOR: $src_url beklenen=$want_sha gelen=$got"; exit 1
      fi
    fi
    tar -xf "$f" -C "$d" --strip-components=1
    cd "$d"
    PKGDIR="$OUT/$name/root"; export PKGDIR
    rm -rf "$OUT/$name"; mkdir -p "$PKGDIR" "$OUT/$name/JPS"
    if declare -F build >/dev/null; then build; else default_build; fi
    if [ "$sysroot" = yes ] || [ "$lib" = yes ]; then
      make DESTDIR="$SYSROOT" install
      if declare -F post_install >/dev/null; then post_install "$SYSROOT"; fi
    fi
    if [ "$lib" = yes ]; then rm -rf "$OUT/$name"; exit 0; fi
    if declare -F package >/dev/null; then package; else default_package; fi
    if declare -F post_install >/dev/null; then post_install "$PKGDIR"; fi
    if [ "$name" = gawk ]; then rm -f "$PKGDIR$PREFIX/bin/awk"; fi
    for lf in COPYING COPYING.LIB LICENSE LICENSE.txt LICENCE COPYRIGHT NOTICE; do
      if [ -f "$lf" ]; then install -Dm644 "$lf" "$PKGDIR$PREFIX/share/licenses/$name/$lf"; fi
    done
    fix_links "$PKGDIR"
    prune "$PKGDIR"
    nonempty "$PKGDIR" || { echo "paket boş"; exit 1; }
    write_control "$name" "$version" "$deps" "$desc" "$src_url" "$got"
  )
}

run_cargo() {       # ad crate  (yerel cargo install, statik)
  (
    set -euo pipefail
    exec > "$OUT/_logs/$1.log" 2>&1
    unset PKG_CONFIG PKG_CONFIG_LIBDIR PKG_CONFIG_SYSROOT_DIR CPPFLAGS LDFLAGS
    T="$OUT/$1"; rm -rf "$T"; mkdir -p "$T/root$PREFIX" "$T/JPS"
    HOST=${FORGE_RUST_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}
    export CARGO_HOME="$WORK/cargo-home" RUSTFLAGS="-C target-feature=+crt-static" CARGO_NET_RETRY=10 CARGO_HTTP_TIMEOUT=60
    export CARGO_PROFILE_RELEASE_STRIP=symbols CARGO_PROFILE_RELEASE_DEBUG=0
    if ! timeout 2700 cargo install --locked --target "$HOST" --target-dir "$WORK/cargo-target/$1" \
         --root "$T/root$PREFIX" -j "$JOBS" "$2"; then
      if grep -qiE 'Cargo\.lock|--locked' "$OUT/_logs/$1.log"; then
        echo "--- --locked olmadan yeniden deneniyor"
        timeout 2700 cargo install --target "$HOST" --target-dir "$WORK/cargo-target/$1" \
          --root "$T/root$PREFIX" -j "$JOBS" "$2"
      else
        exit 1
      fi
    fi
    rm -rf "$WORK/cargo-target/$1"
    ver=$(sed -n 's/^"[^ ]* \([^ ]*\) (.*$/\1/p' "$T/root$PREFIX/.crates.toml" | head -n1)
    rm -f "$T/root$PREFIX/.crates.toml" "$T/root$PREFIX/.crates2.json"
    ver=${ver%%+*}
    [ -n "$ver" ] || { echo "sürüm okunamadı"; exit 1; }
    nonempty "$T/root" || { echo "paket boş"; exit 1; }
    src=$(ls -d "$CARGO_HOME"/registry/src/*/"$2-$ver"* 2>/dev/null | head -n1 || true)
    d=""
    if [ -n "$src" ]; then d=$(cargo_desc "$src/Cargo.toml"); fi
    [ -n "$d" ] || d="$2 (Rust)"
    write_control "$1" "$ver" "" "$d" "crates.io:$2" ""
  )
}

run_cargo_cross() { # ad crate  ('cross' ile crates.io kaynağından çapraz derleme)
  (
    set -euo pipefail
    exec > "$OUT/_logs/$1.log" 2>&1
    TGT=${FORGE_RUST_TARGET:?FORGE_RUST_TARGET gerekli}
    T="$OUT/$1"; rm -rf "$T"; mkdir -p "$T/root$PREFIX/bin" "$T/JPS"
    ver=$(curl -fsSL --retry 3 -H "User-Agent: $UA" "https://crates.io/api/v1/crates/$2" | jq -r '.crate.max_stable_version // .crate.max_version')
    { [ -n "$ver" ] && [ "$ver" != null ]; } || { echo "crates.io'da sürüm bulunamadı"; exit 1; }
    d="$WORK/cx/$1"; rm -rf "$d"; mkdir -p "$d"
    curl -fsSL --retry 3 -H "User-Agent: $UA" "https://static.crates.io/crates/$2/$2-$ver.crate" | tar -xz -C "$d" --strip-components=1
    cd "$d"
    lock=""; if [ -f Cargo.lock ]; then lock="--locked"; fi
    if ! timeout 2700 cross build --release $lock --target "$TGT" --config 'profile.release.strip="symbols"'; then
      if [ -n "$lock" ]; then
        echo "--- --locked olmadan yeniden deneniyor"
        timeout 2700 cross build --release --target "$TGT" --config 'profile.release.strip="symbols"'
      else
        exit 1
      fi
    fi
    find "target/$TGT/release" -maxdepth 1 -type f -perm -u+x ! -name '*.so' ! -name '*.d' ! -name '*.rlib' | while IFS= read -r f; do
      install -Dm755 "$f" "$T/root$PREFIX/bin/$(basename "$f")"
    done
    nonempty "$T/root" || { echo "ikili üretilmedi"; exit 1; }
    ver=${ver%%+*}
    dsc=$(cargo_desc "Cargo.toml")
    [ -n "$dsc" ] || dsc="$2 (Rust)"
    write_control "$1" "$ver" "" "$dsc" "crates.io:$2" ""
    cd /; rm -rf "$d"
  )
}

go_name() {         # modül yolundan paket adı türet
  local m=${1%%@*} b
  m=${m%/}; b=${m##*/}
  case "$b" in v[0-9]*) m=${m%/*}; b=${m##*/} ;; esac
  printf '%s' "$b" | tr 'A-Z_' 'a-z-'
}

run_go() {          # ad modül
  (
    set -euo pipefail
    exec > "$OUT/_logs/$1.log" 2>&1
    unset PKG_CONFIG PKG_CONFIG_LIBDIR PKG_CONFIG_SYSROOT_DIR CPPFLAGS LDFLAGS
    T="$OUT/$1"; rm -rf "$T"; mkdir -p "$T/root$PREFIX/bin" "$T/JPS"
    BINDIR="$T/root$PREFIX/bin"
    export GOPATH="$WORK/gopath" GOCACHE="$WORK/gocache" CGO_ENABLED=0 GOTOOLCHAIN=auto GOPROXY=https://proxy.golang.org,direct
    if [ -n "${FORGE_GOARCH:-}" ]; then
      # çapraz derlemede GOBIN tanımlı olamaz: ikili $GOPATH/bin/linux_<arch>/ altına düşer
      unset GOBIN
      export GOOS=linux GOARCH="$FORGE_GOARCH"
      rm -rf "$GOPATH/bin/linux_$FORGE_GOARCH"
    else
      export GOBIN="$BINDIR"
    fi
    ok=""
    for try in 1 2 3; do
      if timeout 2400 go install -trimpath -ldflags='-s -w' "$2@latest"; then ok=1; break; fi
      tail -n 20 "$OUT/_logs/$1.log" | grep -qiE 'dial tcp|i/o timeout|lookup |connection|TLS|unexpected EOF' || break
      echo "--- ağ hatası, yeniden deneniyor ($try)"; sleep $((try * 5))
    done
    [ -n "$ok" ] || exit 1
    if [ -n "${FORGE_GOARCH:-}" ]; then
      src="$GOPATH/bin/linux_$FORGE_GOARCH"
      [ -d "$src" ] || { echo "çapraz derlenmiş ikili dizini yok: $src"; exit 1; }
      cp "$src"/* "$BINDIR/"
      rm -rf "$src"
    fi
    bin=$(find "$BINDIR" -maxdepth 1 -type f | head -n1)
    [ -n "$bin" ] || { echo "ikili üretilmedi"; exit 1; }
    ver=$(go version -m "$bin" | awk '$1=="mod"{print $3; exit}')
    ver=${ver#v}; ver=${ver%%+*}
    case "$ver" in [0-9]*) ;; *) echo "geçersiz sürüm: $ver"; exit 1 ;; esac
    write_control "$1" "$ver" "" "Go aracı: $2" "go:$2" ""
  )
}

run_font() {        # zip-adı etiket
  local pkg; pkg=$(font_pkg "$1")
  (
    set -euo pipefail
    exec > "$OUT/_logs/$pkg.log" 2>&1
    ver=${2#v}
    url="https://github.com/ryanoasis/nerd-fonts/releases/download/$2/$1.zip"
    f="$DL/nf-$ver-$1.zip"
    if [ ! -s "$f" ]; then curl -fsSL --retry 3 --connect-timeout 20 -o "$f.part" "$url"; mv "$f.part" "$f"; fi
    got=$(sha256sum "$f" | cut -d' ' -f1)
    d="$WORK/nf/$pkg"; rm -rf "$d"; mkdir -p "$d"
    unzip -q -o "$f" -d "$d"
    T="$OUT/$pkg"; rm -rf "$T"
    mkdir -p "$T/JPS" "$T/root$PREFIX/share/fonts/nerd-fonts/$1" "$T/root$PREFIX/share/licenses/$pkg"
    find "$d" -type f \( -iname '*.ttf' -o -iname '*.otf' \) -exec cp {} "$T/root$PREFIX/share/fonts/nerd-fonts/$1/" \;
    find "$d" -maxdepth 1 -type f \( -iname 'LICENSE*' -o -iname 'OFL*' -o -iname 'README*' \) -exec cp {} "$T/root$PREFIX/share/licenses/$pkg/" \;
    [ -n "$(find "$T/root$PREFIX/share/fonts/nerd-fonts/$1" -type f | head -n1)" ] || { echo "yazı tipi dosyası yok"; exit 1; }
    printf '#!/bin/sh\n[ "$JPS_ROOT" = "/" ] || exit 0\ncommand -v fc-cache >/dev/null 2>&1 && fc-cache -f %s/share/fonts >/dev/null 2>&1 || true\n' "$PREFIX" > "$T/JPS/postinst"
    cp "$T/JPS/postinst" "$T/JPS/postrm"; chmod 755 "$T/JPS/postinst" "$T/JPS/postrm"
    write_control "$pkg" "$ver" "" "Nerd Fonts: $1 (programlama yazı tipi, simge yamalı)" "$url" "$got"
    rm -rf "$d"
  )
}

run_snap() {        # ad depo hedef açıklama
  (
    set -euo pipefail
    exec > "$OUT/_logs/$1.log" 2>&1
    url="https://github.com/$2/archive/HEAD.tar.gz"
    f="$DL/snap-$1.tar.gz"
    curl -fsSL --retry 3 --connect-timeout 20 -o "$f" "$url"
    got=$(sha256sum "$f" | cut -d' ' -f1)
    T="$OUT/$1"; rm -rf "$T"
    dest="$T/root$PREFIX/$3/$1"
    mkdir -p "$T/JPS" "$dest"
    tar -xzf "$f" -C "$dest" --strip-components=1
    rm -rf "$dest/.git" "$dest/.github"
    nonempty "$T/root" || { echo "paket boş"; exit 1; }
    write_control "$1" "0.0.$(date -u +%Y%m%d)" "" "$4" "$url" "$got"
  )
}

gnu_recipe() {      # ad bayraklar açıklama -> $WORK/gnu/<ad>.sh
  local base="https://ftp.gnu.org/gnu/$1" file ver
  file=$(curl -fsSL --retry 3 --connect-timeout 20 "$base/" 2>/dev/null \
    | grep -o "href=\"$1-[0-9][0-9.]*\.tar\.\(xz\|gz\|bz2\)\"" | sed 's/^href="//; s/"$//' | sort -V | tail -n1)
  [ -n "$file" ] || return 1
  ver=${file#"$1"-}; ver=${ver%.tar.*}
  mkdir -p "$WORK/gnu"
  {
    echo "name=$1"; echo "version=$ver"; printf 'desc=%q\n' "$3"
    echo "src_url=$base/$file"; echo "pin=no"; printf 'configure_flags=%q\n' "$2"
  } > "$WORK/gnu/$1.sh"
}

finish() {          # ad rc  -> sonucu kaydet
  if [ "$2" -eq 0 ]; then
    say_ok "$1" "$(sed -n 's/^Version: //p' "$OUT/$1/JPS/control")"
  else
    say_skip "$1" "derleme başarısız: $(errtail "$1")"; show_fail "$1"; rm -rf "$OUT/$1"
  fi
}

# ───────────── 1) el yazısı C tarifleri (yalnızca 0. parça) ─────────────
if has_phase c && { [ -n "${FORGE_ONLY:-}" ] || [ "$SHARDS" -le 1 ] || [ "$SHARD" -eq 0 ]; }; then
  echo "== Forge ($ARCH): C tarifleri =="
  while IFS= read -r rn; do
    case "$rn" in ''|'#'*) continue ;; esac
    want "$rn" || continue
    rf="$ROOT/recipes/c/$rn/recipe.sh"
    [ -f "$rf" ] || { say_skip "$rn" "tarif yok"; continue; }
    if out_of_time; then say_skip "$rn" "zaman bütçesi doldu"; continue; fi
    echo "→ $rn"
    run_recipe "$rf" "$rn" </dev/null; rc=$?
    if [ $rc -eq 0 ] && [ ! -f "$OUT/$rn/JPS/control" ]; then echo "  ✔ $rn (kütüphane, paketlenmez)"; else finish "$rn" $rc; fi
  done < "$ROOT/recipes/order.list"
fi

# ───────────── 2) GNU araçları ─────────────
if has_phase gnu; then
  echo "== Forge ($ARCH, parça $SHARD/$SHARDS): GNU =="
  i=0
  while IFS='|' read -r name flags desc; do
    case "$name" in ''|'#'*) continue ;; esac
    i=$((i+1))
    want "$name" || continue
    mine "$((i+7))" || continue
    valid_pkg "$name" || { say_skip "$name" "geçersiz paket adı"; continue; }
    if out_of_time; then say_skip "$name" "zaman bütçesi doldu"; continue; fi
    echo "→ $name (GNU)"
    if ! gnu_recipe "$name" "$flags" "$desc"; then say_skip "$name" "ftp.gnu.org'da sürüm bulunamadı"; continue; fi
    run_recipe "$WORK/gnu/$name.sh" "$name" </dev/null; rc=$?
    finish "$name" $rc
  done < <(grep -v '^[[:space:]]*#' "$ROOT/recipes/catalog/gnu.list")
fi

# ───────────── 3) Rust ─────────────
if has_phase rust; then
  echo "== Forge ($ARCH, parça $SHARD/$SHARDS): Rust (${FORGE_RUST_MODE:-yerel}) =="
  i=0
  while IFS= read -r tok; do
    i=$((i+1))
    if [[ "$tok" == *=* ]]; then name=${tok%%=*}; crate=${tok#*=}; else crate=$tok; name=$(printf '%s' "$tok" | tr 'A-Z_' 'a-z-'); fi
    want "$name" || continue
    mine "$i" || continue
    valid_pkg "$name" || { say_skip "$name" "geçersiz paket adı"; continue; }
    if out_of_time; then say_skip "$name" "zaman bütçesi doldu"; continue; fi
    if [ "${FORGE_RUST_MODE:-native}" = cross ]; then
      command -v cross >/dev/null 2>&1 || { say_skip "$name" "cross yok"; continue; }
      echo "→ $name ($crate, çapraz)"
      run_cargo_cross "$name" "$crate" </dev/null; rc=$?
    else
      command -v cargo >/dev/null 2>&1 || { say_skip "$name" "cargo yok"; continue; }
      echo "→ $name ($crate)"
      run_cargo "$name" "$crate" </dev/null; rc=$?
    fi
    finish "$name" $rc
  done < <(tokens "$ROOT/recipes/catalog/cargo.list")
fi

# ───────────── 4) Go ─────────────
if has_phase go; then
  echo "== Forge ($ARCH, parça $SHARD/$SHARDS): Go${FORGE_GOARCH:+ (GOARCH=$FORGE_GOARCH)} =="
  i=0
  while IFS= read -r tok; do
    i=$((i+1))
    if [[ "$tok" == *=* ]]; then name=${tok%%=*}; mod=${tok#*=}; else mod=$tok; name=$(go_name "$tok"); fi
    want "$name" || continue
    mine "$i" || continue
    valid_pkg "$name" || { say_skip "$name" "geçersiz paket adı ($mod)"; continue; }
    command -v go >/dev/null 2>&1 || { say_skip "$name" "go yok"; continue; }
    if out_of_time; then say_skip "$name" "zaman bütçesi doldu"; continue; fi
    echo "→ $name ($mod)"
    run_go "$name" "$mod" </dev/null; rc=$?
    finish "$name" $rc
  done < <(tokens "$ROOT/recipes/catalog/go.list")
fi

# ───────────── 5) Yazı tipleri (mimariden bağımsız) ─────────────
if has_phase fonts; then
  echo "== Forge ($ARCH): Nerd Fonts =="
  NF_TAG=$(curl -fsSI -A "$UA" https://github.com/ryanoasis/nerd-fonts/releases/latest | tr -d '\r' | sed -n 's#^[Ll]ocation: .*/tag/##p' | head -n1 || true)
  if [ -z "$NF_TAG" ]; then
    say_skip "nerd-fonts" "sürüm etiketi bulunamadı"
  else
    echo "  etiket: $NF_TAG"
    i=0
    while IFS= read -r z; do
      i=$((i+1)); pkg=$(font_pkg "$z")
      want "$pkg" || continue
      mine "$i" || continue
      valid_pkg "$pkg" || { say_skip "$pkg" "geçersiz paket adı"; continue; }
      if out_of_time; then say_skip "$pkg" "zaman bütçesi doldu"; continue; fi
      echo "→ $pkg"
      run_font "$z" "$NF_TAG" </dev/null; rc=$?
      finish "$pkg" $rc
    done < <(tokens "$ROOT/recipes/catalog/fonts.list")
  fi
fi

# ───────────── 6) Eklenti anlık görüntüleri (mimariden bağımsız) ─────────────
if has_phase snap; then
  echo "== Forge ($ARCH): eklentiler =="
  i=0
  while IFS='|' read -r name repo dest desc; do
    case "$name" in ''|'#'*) continue ;; esac
    i=$((i+1))
    want "$name" || continue
    mine "$i" || continue
    valid_pkg "$name" || { say_skip "$name" "geçersiz paket adı"; continue; }
    echo "→ $name ($repo)"
    run_snap "$name" "$repo" "$dest" "$desc" </dev/null; rc=$?
    finish "$name" $rc
  done < <(grep -v '^[[:space:]]*#' "$ROOT/recipes/catalog/snap.list")
fi

# bağımlılığı derlenememiş paketleri ele (yayınlanan depo tutarlı kalsın)
changed=1
while [ "$changed" = 1 ]; do
  changed=0
  while IFS= read -r p; do
    [ -n "$p" ] || continue
    for dep in $(sed -n 's/^Depends: *//p' "$OUT/$p/JPS/control" | tr ',' ' '); do
      if ! grep -qxF "$dep" "$OUT/top.list"; then
        say_skip "$p" "bağımlılık yok: $dep"
        grep -vxF "$p" "$OUT/top.list" > "$OUT/top.list.tmp" || true
        mv "$OUT/top.list.tmp" "$OUT/top.list"
        rm -rf "$OUT/$p"
        changed=1
        break
      fi
    done
  done < <(cat "$OUT/top.list")
done

echo "== Özet ($ARCH, parça $SHARD/$SHARDS): $(wc -l < "$OUT/top.list") paket, $(wc -l < "$OUT/skipped.txt") atlanan, $(( SECONDS - START )) sn =="
exit 0
