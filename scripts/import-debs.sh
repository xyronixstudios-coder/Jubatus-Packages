#!/bin/bash
# Docker gerektirmez. Debian/Ubuntu tabanlı herhangi bir makinede (root'suz) çalışır.
# Kullanım: import-debs.sh <amd64|i386> <aday-listesi> <çıktı-dizini>
set -uo pipefail
ARCH=$1; CAND=$(readlink -f "$2"); OUT=$3
export DEBIAN_FRONTEND=noninteractive LC_ALL=C
W=${JPS_WORK:-/tmp/jps-imp-$ARCH}
rm -rf "$W"; mkdir -p "$W"/{state/lists/partial,cache/archives/partial,debs,ctl,cl,sk} "$OUT"
OUT=$(readlink -f "$OUT")
: > "$W/status"

OPTS="-o Dir::State=$W/state -o Dir::State::status=$W/status -o Dir::Cache=$W/cache -o Debug::NoLocking=1 -o APT::Architecture=$ARCH -o APT::Architectures=$ARCH -o APT::Install-Recommends=0 -o APT::Install-Suggests=0 -o Acquire::Languages=none"
printf '#!/bin/sh\nexec apt-get %s "$@"\n' "$OPTS" > "$W/ap"
printf '#!/bin/sh\nexec apt-cache %s "$@"\n' "$OPTS" > "$W/ac"
chmod +x "$W/ap" "$W/ac"
cd "$W"

echo "→ paket dizini indiriliyor ($ARCH)"
./ap update -qq 2>"$W/update.err" || { echo "apt update başarısız:"; tail -n 5 "$W/update.err"; exit 1; }

# taban: sistemde zaten bulunan paketler (paketlenmez)
./ac dumpavail | awk 'BEGIN{RS="";FS="\n"}{n="";e=0;p="";for(i=1;i<=NF;i++){if($i~/^Package: /)n=substr($i,10);if($i~/^Essential: yes/)e=1;if($i~/^Priority: /)p=substr($i,11)} if(e||p=="required"||p=="important")print n}' > base.txt
printf '%s\n' libstdc++6 libgcc-s1 gcc-12-base gcc-14-base zlib1g libssl3 libssl3t64 libzstd1 liblzma5 libbz2-1.0 \
  libacl1 libattr1 libselinux1 libpcre2-8-0 libsystemd0 libcap2 libgmp10 libtinfo6 libncursesw6 libcrypt1 >> base.txt
sort -u base.txt -o base.txt

sed 's/#.*//' "$CAND" | tr -s ' \t' '\n\n' | sed '/^$/d' | sort -u > cand.txt
echo "→ $(wc -l < cand.txt) aday çözümleniyor"

export W
res() {
  p=$1
  if ! out=$("$W/ap" install -s --no-install-recommends "$p" 2>/dev/null); then
    echo "$p: depoda yok veya çözülemedi" > "$W/sk/$p"; return
  fi
  names=$(printf '%s\n' "$out" | awk '/^Inst /{print $2}' | sort -u | grep -vxFf "$W/base.txt" | tr '\n' ' ')
  case " $names " in
    *" $p "*) echo "$p $names" > "$W/cl/$p" ;;
    *) echo "$p: tabanda zaten var veya sanal paket" > "$W/sk/$p" ;;
  esac
}
export -f res
xargs -P 8 -I{} bash -c 'res "$1"' _ {} < cand.txt

cat cl/* 2>/dev/null > closure.txt || true
cat sk/* 2>/dev/null > skipped.txt || true
echo "→ çözüldü: $(wc -l < closure.txt) aday"
[ -s closure.txt ] || { echo "hiç paket çözülemedi"; exit 1; }

cut -d' ' -f2- closure.txt | tr ' ' '\n' | sed '/^$/d' | sort -u > need.txt
echo "→ $(wc -l < need.txt) .deb indiriliyor"
chmod 777 debs
( cd debs && xargs -n 50 "$W/ap" download -qq < ../need.txt ) 2> dl.err \
  || { echo "indirme hatası:"; tail -n 20 dl.err; exit 1; }

# kurulum betiği gerektirenleri bul
: > bad.txt
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

awk 'FILENAME=="bad.txt"{bad[$1]=1;next}
{ why=""; for(i=2;i<=NF;i++) if($i in bad) why=$i;
  if(why=="") print $1; else print $1": "why" kurulum betiği gerektiriyor (desteklenmiyor)" > "/dev/stderr" }' \
  bad.txt closure.txt > top.list 2>> skipped.txt
awk 'FILENAME=="top.list"{top[$1]=1;next} ($1 in top){for(i=1;i<=NF;i++) print $i}' top.list closure.txt \
  | sort -u > final.list

mapdeps() {
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

while IFS= read -r n; do
  f=${DEB[$n]:-}
  [ -n "$f" ] || { echo "$n: .deb bulunamadı" >> skipped.txt; continue; }
  d="$OUT/$n"; rm -rf "$d"; mkdir -p "$d/JPS" "$d/root"
  dpkg-deb -x "$f" "$d/root" || { echo "$n: açılamadı" >> skipped.txt; rm -rf "$d"; continue; }
  for x in bin sbin lib lib32 lib64 libx32; do
    if [ -d "$d/root/$x" ] && [ ! -L "$d/root/$x" ]; then
      mkdir -p "$d/root/usr/$x"; cp -a "$d/root/$x/." "$d/root/usr/$x/"; rm -rf "$d/root/$x"
    fi
  done
  rm -rf "$d/root/usr/share/locale" "$d/root/usr/share/lintian" "$d/root/usr/share/bug"
  if [ -d "$d/root/usr/share/doc" ]; then
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
    echo "Package: $n"; echo "Version: $ver"; echo "Architecture: $arch"
    echo "Maintainer: Jubatus Forge <packages@jubatus.local>"
    echo "Depends: $deps"; echo "Description: $desc"
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
