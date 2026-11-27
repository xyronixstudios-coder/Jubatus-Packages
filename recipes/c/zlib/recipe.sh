name=zlib
version=1.3.1
desc="zlib sıkıştırma kütüphanesi (yalnızca derleme için)"
src_url=https://github.com/madler/zlib/releases/download/v1.3.1/zlib-1.3.1.tar.gz
lib=yes
build() { ./configure --prefix="$PREFIX" --static && make -j"$JOBS"; }
