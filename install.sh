#!/bin/sh
# Jubatus Forge (jps) kurulum betiği — mimariye göre jps-64 veya jps-32 indirir
set -eu
REPO="${JPS_REPO:-https://raw.githubusercontent.com/xyronixstudios-coder/Jubatus-Packages/repo}"

case "$(uname -m)" in
  x86_64|amd64|i?86|x86) ;;
  *) echo "Desteklenmeyen mimari: $(uname -m) (yalnızca jps-64 ve jps-32 var)" >&2; exit 1 ;;
esac
if [ "$(getconf LONG_BIT 2>/dev/null || echo 64)" = "32" ]; then BIN=jps-32; else BIN=jps-64; fi
echo "Mimari algılandı → $BIN"

get() {
  if command -v curl >/dev/null 2>&1; then curl -fsSL "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then wget -q -O "$2" "$1"
  else echo "curl veya wget gerekli" >&2; exit 1; fi
}

T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
get "$REPO/$BIN" "$T/$BIN"
get "$REPO/sha256sums" "$T/sha256sums"
(cd "$T" && grep " $BIN\$" sha256sums | sha256sum -c -)

SUDO=""
[ "$(id -u)" -eq 0 ] || SUDO="sudo"
$SUDO install -m755 "$T/$BIN" /usr/local/bin/jps
$SUDO env JPS_REPO="$REPO" /usr/local/bin/jps update
$SUDO /usr/local/bin/jps trust-repo
$SUDO /usr/local/bin/jps desktop-install --system || true
echo "✔ jps kuruldu. Dene: jps search editör   |   sudo jps install htop"
