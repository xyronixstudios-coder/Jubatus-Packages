#!/bin/sh
# Alpine konteyneri İÇİNDE çalışır (CI): derleme araçlarını kurar ve forge'u çalıştırır.
set -eu
OUT=$1
apk add --no-cache bash build-base linux-headers curl ca-certificates coreutils tar xz bzip2 gzip \
  findutils sed grep gawk patch perl python3 pkgconf bison flex m4 git
apk add --no-cache rust cargo >/dev/null 2>&1 || echo "uyarı: rust/cargo bu mimaride kurulamadı, Rust tarifleri atlanacak"
apk add --no-cache go >/dev/null 2>&1 || echo "uyarı: go bu mimaride kurulamadı, Go tarifleri atlanacak"
mkdir -p "$OUT"
bash scripts/forge/build-all.sh "$OUT" || true
chmod -R a+rX "$OUT"
if [ -n "${HOST_UID:-}" ]; then chown -R "$HOST_UID:$HOST_UID" "$OUT" || true; fi
