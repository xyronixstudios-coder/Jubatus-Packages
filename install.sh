#!/bin/sh
# Jubatus Forge (jps) kurulum betiği
# Mimariye göre jps-64 veya jps-32 indirir, Ed25519 imzasını openssl ile doğrular, sha256'yı denetler.
set -eu
REPO="${JPS_REPO:-https://raw.githubusercontent.com/xyronixstudios-coder/Jubatus-Packages/repo}"
PUBHEX="da9b024d6ba3e9172a5eccfd247a7ab1f265c42ce8947d3de5f8b17ed29fd06a"
PUBB64="MCowBQYDK2VwAyEA2psCTWuj6RcqXsz9JHp6sfJlxCzolH095fixftKf0Go="
FPR="79894bcffcff7bfe"

case "$(uname -m)" in
  x86_64|amd64|i?86|x86) ;;
  *) echo "Desteklenmeyen mimari: $(uname -m) (yalnızca jps-64 ve jps-32 var)" >&2; exit 1 ;;
esac
BITS=$(getconf LONG_BIT 2>/dev/null || true)
if [ -z "$BITS" ]; then
  case "$(uname -m)" in i?86|x86) BITS=32 ;; *) BITS=64 ;; esac
fi
if [ "$BITS" = "32" ]; then BIN=jps-32; else BIN=jps-64; fi
echo "Mimari algılandı → $BIN"

get() {
  if command -v curl >/dev/null 2>&1; then curl -fsSL "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then wget -q -O "$2" "$1"
  else echo "curl veya wget gerekli" >&2; exit 1; fi
}

T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
get "$REPO/$BIN" "$T/$BIN"
get "$REPO/$BIN.sig" "$T/$BIN.sig"
get "$REPO/sha256sums" "$T/sha256sums"
(cd "$T" && grep " $BIN\$" sha256sums | sha256sum -c -)

if command -v openssl >/dev/null 2>&1; then
  printf -- '-----BEGIN PUBLIC KEY-----\n%s\n-----END PUBLIC KEY-----\n' "$PUBB64" > "$T/pub.pem"
  if openssl pkeyutl -verify -pubin -inkey "$T/pub.pem" -rawin -in "$T/$BIN" -sigfile "$T/$BIN.sig" >/dev/null 2>&1; then
    echo "✔ imza doğrulandı (anahtar parmak izi: $FPR)"
  else
    echo "HATA: imza GEÇERSİZ veya openssl 3.0'dan eski. İndirilen dosyaya güvenilmiyor." >&2
    exit 1
  fi
elif [ "${JPS_SKIP_VERIFY:-0}" = 1 ]; then
  echo "uyarı: openssl yok, imza doğrulanmadı (JPS_SKIP_VERIFY=1)" >&2
else
  echo "HATA: imza doğrulaması için openssl (3.0+) gerekli: sudo apt install openssl" >&2
  exit 1
fi

printf '%s' "$PUBHEX" > "$T/signing.pub"
SUDO=""
if [ "$(id -u)" -ne 0 ]; then
  if command -v sudo >/dev/null 2>&1; then SUDO="sudo"
  elif command -v doas >/dev/null 2>&1; then SUDO="doas"
  else echo "HATA: root değilsin ve sudo/doas yok" >&2; exit 1; fi
fi
$SUDO install -m755 "$T/$BIN" /usr/local/bin/jps
$SUDO /usr/local/bin/jps trust "$T/signing.pub" --system
$SUDO env JPS_REPO="$REPO" /usr/local/bin/jps update
$SUDO /usr/local/bin/jps desktop-install --system || true
echo "✔ jps kuruldu. Dene: jps search editör   |   sudo jps install htop"
