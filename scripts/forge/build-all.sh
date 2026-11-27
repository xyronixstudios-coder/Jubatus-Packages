#!/bin/bash
# Jubatus Forge: tarif (recipe) derleyici.
# Dağıtımdan bağımsız, STATİK paket ağaçları üretir: <çıktı>/<paket>/{JPS/control,root/...}
# CI'da Alpine konteynerinde (musl) çalışır. Yerelde de çalışır (statik glibc çıkar, yalnızca deneme içindir).
#   FORGE_ARCH=amd64|i386   hedef mimari etiketi (zorunlu)
#   FORGE_STRICT=1          pinlenmemiş kaynağı reddet (CI)
#   FORGE_ONLY="a b c"      yalnızca bu adları derle (kütüphane bağımlılıklarını da yaz)
#   FORGE_WORK / FORGE_DL / FORGE_SYSROOT   çalışma, indirme önbelleği ve kütüphane kök dizinleri
set -uo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ARCH=${FORGE_ARCH:?FORGE_ARCH gerekli (amd64|i386)}
[ $# -ge 1 ] || { echo "kullanım: build-all.sh <çıktı-dizini>" >&2; exit 2; }
mkdir -p "$1"; OUT=$(readlink -f "$1")
PREFIX=/usr/local
WORK=${FORGE_WORK:-/forge/work}
DL=${FORGE_DL:-/forge/dl}
SYSROOT=${FORGE_SYSROOT:-/forge/sysroot}
LOCK="$ROOT/recipes.lock"
JOBS=$(nproc 2>/dev/null || echo 2)
mkdir -p "$WORK" "$DL" "$SYSROOT" "$OUT/_logs"
touch "$LOCK"
: > "$OUT/top.list"; : > "$OUT/skipped.txt"; : > "$OUT/allowed.txt"

export PREFIX JOBS SYSROOT
export CFLAGS="-O2 -pipe" CXXFLAGS="-O2 -pipe"
export CPPFLAGS="-I$SYSROOT$PREFIX/include"
export LDFLAGS="-static -L$SYSROOT$PREFIX/lib"
export PKG_CONFIG="pkg-config --static"
export PKG_CONFIG_LIBDIR="$SYSROOT$PREFIX/lib/pkgconfig"
export PKG_CONFIG_SYSROOT_DIR="$SYSROOT"

want()      { [ -z "${FORGE_ONLY:-}" ] && return 0; case " $FORGE_ONLY " in *" $1 "*) return 0 ;; esac; return 1; }
say_ok()    { echo "$1" >> "$OUT/top.list"; echo "  ✔ $1 ${2:-}"; }
say_skip()  { echo "$1: $2" >> "$OUT/skipped.txt"; echo "  ✘ $1: $2"; }
errtail()   { tail -n 4 "$OUT/_logs/$1.log" 2>/dev/null | tr '\n\t' '  ' | cut -c1-220; }
valid_pkg() { [[ $1 =~ ^[a-z0-9][a-z0-9+.-]+$ ]]; }
nonempty()  { [ -n "$(find "$1" \( -type f -o -type l \) 2>/dev/null | head -n1)" ]; }

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

fix_links() {       # DESTDIR'e işaret eden mutlak sembolik bağları paket içi yola çevir
  find "$1" -type l | while IFS= read -r l; do
    t=$(readlink "$l")
    case "$t" in "$1"*) ln -sf "${t#"$1"}" "$l" ;; esac
  done
}

prune() {           # çalışma zamanında gerekmeyenleri at, ikilileri küçült
  local r=$1$PREFIX
  rm -rf "$r/include" "$r/share/info" "$r/share/locale" "$r/share/doc" "$r/lib/pkgconfig" \
         "$r/share/aclocal" "$r/share/pkgconfig" "$r/share/gtk-doc"
  find "$r" \( -name '*.a' -o -name '*.la' \) -delete 2>/dev/null || true
  find "$r/bin" "$r/sbin" -type f -exec strip --strip-unneeded {} + 2>/dev/null || true
  find "$1" -mindepth 1 -depth -type d -empty -delete 2>/dev/null || true
}

default_build()   { ./configure --prefix="$PREFIX" $configure_flags && make -j"$JOBS"; }
default_package() { make DESTDIR="$PKGDIR" install; }

run_recipe() {      # $1 = tarif dosyası, $2 = beklenen ad
  (
    set -euo pipefail
    name=""; version=""; desc=""; src_url=""; deps=""; lib=no; sysroot=no; configure_flags=""
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
    want_sha=$(awk -v u="$src_url" '$1==u {print $2; exit}' "$LOCK")
    if [ -z "$want_sha" ]; then
      if [ "${FORGE_STRICT:-0}" = 1 ]; then echo "pinlenmemiş kaynak: $src_url (scripts/forge/pin.sh çalıştır)"; exit 1; fi
      echo "$src_url $got" >> "$LOCK"
    elif [ "$want_sha" != "$got" ]; then
      echo "SHA256 UYUŞMUYOR: $src_url beklenen=$want_sha gelen=$got"; exit 1
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
    for lf in COPYING COPYING.LIB LICENSE LICENSE.txt LICENCE COPYRIGHT NOTICE; do
      if [ -f "$lf" ]; then install -Dm644 "$lf" "$PKGDIR$PREFIX/share/licenses/$name/$lf"; fi
    done
    fix_links "$PKGDIR"
    prune "$PKGDIR"
    nonempty "$PKGDIR" || { echo "paket boş"; exit 1; }
    write_control "$name" "$version" "$deps" "$desc" "$src_url" "$got"
  )
}

run_cargo() {       # ad crate açıklama
  (
    set -euo pipefail
    exec > "$OUT/_logs/$1.log" 2>&1
    unset PKG_CONFIG PKG_CONFIG_LIBDIR PKG_CONFIG_SYSROOT_DIR CPPFLAGS LDFLAGS
    T="$OUT/$1"; rm -rf "$T"; mkdir -p "$T/root$PREFIX" "$T/JPS"
    HOST=$(rustc -vV | sed -n 's/^host: //p')
    export CARGO_HOME="$WORK/cargo-home" RUSTFLAGS="-C target-feature=+crt-static"
    export CARGO_PROFILE_RELEASE_STRIP=symbols CARGO_PROFILE_RELEASE_DEBUG=0
    timeout 2700 cargo install --locked --target "$HOST" --target-dir "$WORK/cargo-target/$1" \
      --root "$T/root$PREFIX" -j "$JOBS" "$2"
    rm -rf "$WORK/cargo-target/$1"
    ver=$(sed -n 's/^"[^ ]* \([^ ]*\) (.*$/\1/p' "$T/root$PREFIX/.crates.toml" | head -n1)
    rm -f "$T/root$PREFIX/.crates.toml" "$T/root$PREFIX/.crates2.json"
    [ -n "$ver" ] || { echo "sürüm okunamadı"; exit 1; }
    nonempty "$T/root" || { echo "paket boş"; exit 1; }
    write_control "$1" "$ver" "" "$3" "crates.io:$2" ""
  )
}

run_go() {          # ad modül açıklama
  (
    set -euo pipefail
    exec > "$OUT/_logs/$1.log" 2>&1
    unset PKG_CONFIG PKG_CONFIG_LIBDIR PKG_CONFIG_SYSROOT_DIR CPPFLAGS LDFLAGS
    T="$OUT/$1"; rm -rf "$T"; mkdir -p "$T/root$PREFIX/bin" "$T/JPS"
    export GOPATH="$WORK/gopath" GOCACHE="$WORK/gocache" GOBIN="$T/root$PREFIX/bin" CGO_ENABLED=0
    timeout 2400 go install -trimpath -ldflags='-s -w' "$2@latest"
    bin=$(ls "$GOBIN" | head -n1)
    [ -n "$bin" ] || { echo "ikili üretilmedi"; exit 1; }
    ver=$(go version -m "$GOBIN/$bin" | awk '$1=="mod"{print $3; exit}')
    ver=${ver#v}
    [ -n "$ver" ] || { echo "sürüm okunamadı"; exit 1; }
    write_control "$1" "$ver" "" "$3" "go:$2" ""
  )
}

echo "== Forge ($ARCH): C tarifleri =="
while IFS= read -r rn; do
  case "$rn" in ''|'#'*) continue ;; esac
  want "$rn" || continue
  rf="$ROOT/recipes/c/$rn/recipe.sh"
  [ -f "$rf" ] || { say_skip "$rn" "tarif yok"; continue; }
  echo "→ $rn"
  run_recipe "$rf" "$rn" </dev/null; rc=$?
  if [ $rc -eq 0 ]; then
    if [ -f "$OUT/$rn/JPS/control" ]; then say_ok "$rn" "$(sed -n 's/^Version: //p' "$OUT/$rn/JPS/control")"; else echo "  ✔ $rn (kütüphane, paketlenmez)"; fi
  else
    say_skip "$rn" "derleme başarısız: $(errtail "$rn")"; rm -rf "$OUT/$rn"
  fi
done < "$ROOT/recipes/order.list"

echo "== Forge ($ARCH): Rust (cargo) =="
while IFS='|' read -r name crate desc; do
  case "$name" in ''|'#'*) continue ;; esac
  want "$name" || continue
  valid_pkg "$name" || { say_skip "$name" "geçersiz paket adı"; continue; }
  command -v cargo >/dev/null 2>&1 || { say_skip "$name" "cargo yok"; continue; }
  echo "→ $name ($crate)"
  run_cargo "$name" "$crate" "$desc" </dev/null; rc=$?
  if [ $rc -eq 0 ]; then say_ok "$name" "$(sed -n 's/^Version: //p' "$OUT/$name/JPS/control")"
  else say_skip "$name" "derleme başarısız: $(errtail "$name")"; rm -rf "$OUT/$name"; fi
done < "$ROOT/recipes/catalog/cargo.list"

echo "== Forge ($ARCH): Go =="
while IFS='|' read -r name mod desc; do
  case "$name" in ''|'#'*) continue ;; esac
  want "$name" || continue
  valid_pkg "$name" || { say_skip "$name" "geçersiz paket adı"; continue; }
  command -v go >/dev/null 2>&1 || { say_skip "$name" "go yok"; continue; }
  echo "→ $name ($mod)"
  run_go "$name" "$mod" "$desc" </dev/null; rc=$?
  if [ $rc -eq 0 ]; then say_ok "$name" "$(sed -n 's/^Version: //p' "$OUT/$name/JPS/control")"
  else say_skip "$name" "derleme başarısız: $(errtail "$name")"; rm -rf "$OUT/$name"; fi
done < "$ROOT/recipes/catalog/go.list"

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

echo "== Özet ($ARCH): $(wc -l < "$OUT/top.list") paket, $(wc -l < "$OUT/skipped.txt") atlanan =="
exit 0
