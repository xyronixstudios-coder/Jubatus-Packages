name=busybox
version=1.36.1
desc="BusyBox: tek dosyada yüzlerce temel komut (statik; 'busybox ls' gibi çalıştırılır)"
src_url=https://busybox.net/downloads/busybox-1.36.1.tar.bz2
build() {
  make defconfig
  sed -i -e 's/^# CONFIG_STATIC is not set/CONFIG_STATIC=y/' -e 's/^CONFIG_TC=y/# CONFIG_TC is not set/' .config
  yes "" | make oldconfig >/dev/null
  make -j"$JOBS"
}
package() { install -Dm755 busybox "$PKGDIR$PREFIX/bin/busybox"; }
