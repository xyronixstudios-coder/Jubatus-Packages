name=libevent
version=2.1.12
desc="libevent olay kütüphanesi (yalnızca derleme için)"
src_url=https://github.com/libevent/libevent/releases/download/release-2.1.12-stable/libevent-2.1.12-stable.tar.gz
lib=yes
configure_flags="--disable-shared --enable-static --disable-openssl --disable-samples --disable-libevent-regress"
