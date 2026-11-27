#!/bin/sh
# Alpine konteyneri İÇİNDE çalışır (CI): derleme araçlarını kurar ve forge'u çalıştırır.
set -eu
OUT=$1
apk add --no-cache bash build-base linux-headers curl ca-certificates coreutils tar xz bzip2 gzip \
  findutils sed grep gawk patch perl python3 pkgconf bison flex m4 git
if apk add --no-cache rust cargo >/tmp/apk-rust.log 2>&1; then echo "rust: $(rustc --version)"
else echo "UYARI: rust/cargo bu mimaride kurulamadı, Rust tarifleri atlanacak:"; tail -n 3 /tmp/apk-rust.log; fi
if apk add --no-cache go >/tmp/apk-go.log 2>&1; then echo "go: $(go version)"
else echo "UYARI: go bu mimaride kurulamadı, Go tarifleri atlanacak:"; tail -n 3 /tmp/apk-go.log; fi
mkdir -p "$OUT"
bash scripts/forge/build-all.sh "$OUT" || true
chmod -R a+rX "$OUT"
if [ -n "${HOST_UID:-}" ]; then chown -R "$HOST_UID:$HOST_UID" "$OUT" || true; fi
