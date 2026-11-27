name=ncurses
version=6.5
desc="Terminal kütüphanesi: terminfo veritabanı, tput, clear, infocmp, tic"
src_url=https://ftp.gnu.org/gnu/ncurses/ncurses-6.5.tar.gz
sysroot=yes
deps=""
build() {
  ./configure --prefix="$PREFIX" --without-shared --with-normal --without-debug --without-ada \
    --without-cxx-binding --without-manpages --enable-widec --enable-overwrite --enable-pc-files \
    --with-pkg-config-libdir="$PREFIX/lib/pkgconfig" --disable-stripping \
    --with-terminfo-dirs="/etc/terminfo:/lib/terminfo:/usr/share/terminfo:$PREFIX/share/terminfo" \
    --with-default-terminfo-dir="$PREFIX/share/terminfo"
  make -j"$JOBS"
}
post_install() {   # $1: hedef kök (sysroot veya paket)
  if [ -d "$1$PREFIX/lib" ]; then
    ( cd "$1$PREFIX/lib" && for l in ncurses form panel menu; do ln -sf "lib${l}w.a" "lib${l}.a"; done )
  fi
  if [ -d "$1$PREFIX/lib/pkgconfig" ]; then
    ( cd "$1$PREFIX/lib/pkgconfig" && for l in ncurses form panel menu; do ln -sf "${l}w.pc" "${l}.pc"; done )
  fi
}
