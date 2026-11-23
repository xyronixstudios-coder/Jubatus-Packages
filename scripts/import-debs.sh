#!/bin/bash
# Debian konteyneri İÇİNDE çalışır. Adayların bağımlılık kapanışını indirir ve jps paket ağaçlarına çevirir.
# Kullanım: import-debs.sh <mimari> <aday-listesi> <çıktı-dizini>
set -uo pipefail
ARCH=$1; CAND=$2; OUT=$3
export DEBIAN_FRONTEND=noninteractive LC_ALL=C
W=/tmp/imp
rm -rf "$W"; mkdir -p "$W/debs" "$W/ctl" "$OUT"
cd "$W"
apt-get update -qq >/dev/null 2>&1 || { echo "apt update başarısız"; exit 1; }
: > skipped.txt; : > closure.txt; : > bad.txt

# 1) aday başına bağımlılık kapanışı (tabanda olmayan YENİ paketler)
sed 's/#.*//' "$CAND" | tr -s ' \t' '\n\n' | sed '/^$/d' | sort -u > cand.txt
while IFS= read -r p; do
  if ! sim=$(apt-get install -s --no-install-recommends "$p" 2>/dev/null); then
    echo "$p: depoda yok veya çözülemedi" >> skipped.txt; continue
  fi
  names=$(printf '%s\n' "$sim" | awk '/^Inst / && $3 ~ /^\(/ {print $2}' | tr '\n' ' ')
  case " $names " in
    *" $p "*) echo "$p $names" >> closure.txt ;;
    *) echo "$p: tabanda zaten var veya sanal paket" >> skipped.txt ;;
  esac
done < cand.txt

# 2) indir
cut -d' ' -f2- closure.txt | tr ' ' '\n' | sed '/^$/d' | sort -u > need.txt
[ -s need.txt ] || { echo "hiç paket çözülemedi"; exit 1; }
chmod 777 debs
( cd debs && xargs -n 40 apt-get download -qq < ../need.txt ) 2> dl.err \
  || { echo "indirme hatası:"; tail -n 20 dl.err; exit 1; }

# 3) kurulum betiği gerektirenleri bul (kullanıcı/grup, debconf, dpkg durumu)
declare -A DEB
for f in debs/*.deb; do
  n=$(dpkg-deb -f "$f" Package)
  DEB[$n]=$f
  rm -rf ctl/x; mkdir -p ctl/x; dpkg-deb -e "$f" ctl/x 2>/dev/null
  if cat ctl/x/preinst ctl/x/postinst ctl/x/prerm ctl/x/postrm 2>/dev/null \
     | grep -Eq 'adduser|addgroup|useradd|groupadd|confmodule|db_get|dpkg-statoverride|dpkg-divert|update-ca-certificates|setcap'; then
    echo "$n" >> bad.txt
  fi
done

# 4) elemeler
awk 'FILENAME=="bad.txt"{bad[$1]=1;next}
{ why=""; for(i=2;i<=NF;i++) if($i in bad) why=$i;
  if(why=="") print $1; else print $1": "why" kurulum betiği gerektiriyor (desteklenmiyor)" > "/dev/stderr" }' \
  bad.txt closure.txt > top.list 2>> skipped.txt
awk 'FILENAME=="top.list"{top[$1]=1;next} ($1 in top){for(i=1;i<=NF;i++) print $i}' top.list closure.txt \
  | sort -u > final.list

mapdeps() {  # $1 ham Depends, $2 paketin kendisi
  local raw=$1 self=$2 out="" part alt n
  local -a parts alts
  IFS=',' read -ra parts <<< "$raw"
  for part in "${parts[@]}"; do
    IFS='|' read -ra alts <<< "$part"
    for alt in "${alts[@]}"; do
      n=$(printf '%s' "$alt" | sed -e 's/([^)]*)//g' -e 's/\[[^]]*\]//g' -e 's/<[^>]*>//g' -e 's/:[a-z0-9-]*//' -e 's/[[:space:]]//g')
      [ -n "$n" ] && [ "$n" != "$self" ] || continue
      if grep -qxF -- "$n" "$W/final.list"; then
        case ",$out," in *",$n,"*) ;; *) out="$out${out:+,}$n" ;; esac
        break
      fi
    done
  done
  printf '%s' "$out"
}

# 5) paket ağaçlarını üret
while IFS= read -r n; do
  f=${DEB[$n]:-}
  [ -n "$f" ] || { echo "$n: .deb bulunamadı" >> skipped.txt; continue; }
  d="$OUT/$n"; rm -rf "$d"; mkdir -p "$d/JPS" "$d/root"
  dpkg-deb -x "$f" "$d/root" || { echo "$n: açılamadı" >> skipped.txt; rm -rf "$d"; continue; }
  for x in bin sbin lib lib32 lib64 libx32; do   # usr-merge: /bin -> /usr/bin
    if [ -d "$d/root/$x" ] && [ ! -L "$d/root/$x" ]; then
      mkdir -p "$d/root/usr/$x"; cp -a "$d/root/$x/." "$d/root/usr/$x/"; rm -rf "$d/root/$x"
    fi
  done
  rm -rf "$d/root/usr/share/locale" "$d/root/usr/share/lintian" "$d/root/usr/share/bug"
  if [ -d "$d/root/usr/share/doc" ]; then   # yalnızca lisans (copyright) kalır
    find "$d/root/usr/share/doc" -type f ! -name copyright -delete
    find "$d/root/usr/share/doc" -type l -delete
  fi
  find "$d/root" -mindepth 1 -depth -type d -empty -delete

  ver=$(dpkg-deb -f "$f" Version | sed 's/^[0-9]*://; s/[^A-Za-z0-9.+~_-]/./g')
  arch=$(dpkg-deb -f "$f" Architecture); [ "$arch" = all ] || arch=$ARCH
  desc=$(dpkg-deb -f "$f" Description | head -n1 | tr -d '\r' | sed 's/[[:space:]]*$//')
  [ -n "$desc" ] || desc="Debian paketi $n"
  raw="$(dpkg-deb -f "$f" Pre-Depends 2>/dev/null | tr '\n' ' '), $(dpkg-deb -f "$f" Depends 2>/dev/null | tr '\n' ' ')"
  deps=$(mapdeps "$raw" "$n")
  {
    echo "Package: $n"
    echo "Version: $ver"
    echo "Architecture: $arch"
    echo "Maintainer: Jubatus Forge <packages@jubatus.local>"
    echo "Depends: $deps"
    echo "Description: $desc"
  } > "$d/JPS/control"

  post=""
  if find "$d/root" \( -name '*.so' -o -name '*.so.*' \) 2>/dev/null | head -n1 | grep -q .; then
    post="${post}command -v ldconfig >/dev/null 2>&1 && ldconfig || true\n"
  fi
  if [ -d "$d/root/usr/share/fonts" ]; then
    post="${post}command -v fc-cache >/dev/null 2>&1 && fc-cache -f >/dev/null 2>&1 || true\n"
  fi
  if [ -n "$post" ]; then
    { echo '#!/bin/sh'; echo '[ "$JPS_ROOT" = "/" ] || exit 0'; printf '%b' "$post"; } > "$d/JPS/postinst"
    cp "$d/JPS/postinst" "$d/JPS/postrm"
    chmod 755 "$d/JPS/postinst" "$d/JPS/postrm"
  fi
done < final.list

cp top.list final.list skipped.txt "$OUT/"
chmod -R a+rX "$OUT"
echo "✔ $ARCH: $(wc -l < top.list) aday, $(wc -l < final.list) toplam paket (kütüphaneler dahil), $(wc -l < skipped.txt) atlanan"
