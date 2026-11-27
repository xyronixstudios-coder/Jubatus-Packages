name=bzip2
version=1.0.8
desc="bzip2 sıkıştırma araçları"
src_url=https://sourceware.org/pub/bzip2/bzip2-1.0.8.tar.gz
build() { make -j"$JOBS" CC=gcc LDFLAGS=-static libbz2.a bzip2 bzip2recover; }
package() {
  make PREFIX="$PKGDIR$PREFIX" LDFLAGS=-static install
  ( cd "$PKGDIR$PREFIX/bin" && ln -sf bzip2 bunzip2 && ln -sf bzip2 bzcat )
}
