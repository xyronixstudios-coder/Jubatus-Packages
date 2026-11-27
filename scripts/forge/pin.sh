#!/bin/bash
# Tarif kaynaklarını indirir ve SHA-256 değerlerini recipes.lock'a yazar ("ilk kullanımda güven").
# Yerelde çalıştır, indirilemeyen/yanlış adresleri düzelt, recipes.lock'u incele ve commit'le.
# CI (FORGE_STRICT=1) pinlenmemiş kaynağı reddeder, pinli kaynakta hash uyuşmazsa durur.
set -uo pipefail
cd "$(dirname "$0")/../.."
LOCK=recipes.lock; touch "$LOCK"
DL=${FORGE_DL:-/tmp/forge-dl}; mkdir -p "$DL"
new=0; had=0; bad=0
for r in recipes/c/*/recipe.sh; do
  rn=$(basename "$(dirname "$r")")
  url=$( ( . "$r" >/dev/null 2>&1; printf '%s' "${src_url:-}" ) )
  if [ -z "$url" ]; then echo "✘ $rn: src_url yok"; bad=$((bad+1)); continue; fi
  if awk -v u="$url" '$1==u{f=1} END{exit !f}' "$LOCK"; then echo "= $rn (pinli)"; had=$((had+1)); continue; fi
  f="$DL/$(basename "$url")"
  if ! curl -fsSL --retry 3 --connect-timeout 20 -o "$f" "$url"; then
    echo "✘ $rn: indirilemedi: $url"; bad=$((bad+1)); continue
  fi
  sum=$(sha256sum "$f" | cut -d' ' -f1)
  echo "$url $sum" >> "$LOCK"; echo "+ $rn $sum"; new=$((new+1))
done
echo "yeni: $new, mevcut: $had, hatalı: $bad"
[ "$bad" -eq 0 ]
