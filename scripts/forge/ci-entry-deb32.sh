#!/bin/bash
# Debian i386 konteyneri İÇİNDE çalışır: i686-gnu ana makine araç zinciri + i686-unknown-linux-musl hedefiyle
# statik 32-bit Rust ikilileri üretir (Alpine x86'da güncel Rust araç zinciri yok).
set -eu
OUT=$1
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq --no-install-recommends curl ca-certificates gcc g++ make musl-tools pkg-config git perl \
  xz-utils bzip2 gzip tar findutils sed grep gawk coreutils file >/dev/null
if ! curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable >/tmp/rustup.log 2>&1; then
  echo "UYARI: rustup kurulamadı, i386 Rust tarifleri atlanacak:"; tail -n 5 /tmp/rustup.log; exit 0
fi
export PATH="$HOME/.cargo/bin:$PATH"
echo "rust: $(rustc --version)"
rustup target add i686-unknown-linux-musl
export CC_i686_unknown_linux_musl=musl-gcc
mkdir -p "$OUT"
bash scripts/forge/build-all.sh "$OUT" || true
chmod -R a+rX "$OUT"
if [ -n "${HOST_UID:-}" ]; then chown -R "$HOST_UID:$HOST_UID" "$OUT" || true; fi
