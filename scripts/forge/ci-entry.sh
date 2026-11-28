#!/bin/sh
# Alpine konteyneri İÇİNDE çalışır (CI): derleme araçlarını kurar ve forge'u çalıştırır.
set -eu
OUT=$1
apk add --no-cache bash build-base linux-headers curl ca-certificates coreutils tar xz bzip2 gzip \
  findutils sed grep gawk patch perl python3 pkgconf bison flex m4 git file unzip jq
PHASES="${FORGE_PHASES:-c gnu rust go}"
case " $PHASES " in
  *" rust "*)
    if [ "$(uname -m)" = x86_64 ]; then
      if curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable >/tmp/rustup.log 2>&1; then
        echo "rust: $("$HOME/.cargo/bin/rustc" --version)"
      else
        echo "UYARI: rustup kurulamadı, Rust tarifleri atlanacak:"; tail -n 5 /tmp/rustup.log
      fi
    else
      echo "UYARI: $(uname -m) üzerinde rustup araç zinciri yok; Rust tarifleri atlanacak"
    fi ;;
esac
case " $PHASES " in
  *" go "*)
    if apk add --no-cache go >/tmp/apk-go.log 2>&1; then echo "go: $(go version)"
    else echo "UYARI: go kurulamadı, Go tarifleri atlanacak:"; tail -n 3 /tmp/apk-go.log; fi ;;
esac
export PATH="$HOME/.cargo/bin:$PATH"
mkdir -p "$OUT"
bash scripts/forge/build-all.sh "$OUT" || true
chmod -R a+rX "$OUT"
if [ -n "${HOST_UID:-}" ]; then chown -R "$HOST_UID:$HOST_UID" "$OUT" || true; fi
