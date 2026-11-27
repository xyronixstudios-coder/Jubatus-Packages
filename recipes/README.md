# Forge tarifleri

- `c/<ad>/recipe.sh`: kabuk betiği. Değişkenler: `name version desc src_url deps lib sysroot configure_flags`.
  İsteğe bağlı fonksiyonlar: `build`, `package` (`$PKGDIR`'e kurar), `post_install <kök>`.
  Varsayılan: `./configure --prefix=/usr/local $configure_flags && make`, `make DESTDIR=$PKGDIR install`.
  `lib=yes`: yalnızca derleme için kütüphane, paketlenmez. `sysroot=yes`: paketlenir, başkalarının derlemesi için de kurulur.
- `catalog/cargo.list`, `catalog/go.list`: satır başına bir paket (`ad|kaynak|açıklama`).
- `order.list`: C tariflerinin derleme sırası.
- `recipes.lock`: kaynak URL ve SHA-256 (`scripts/forge/pin.sh` üretir).
