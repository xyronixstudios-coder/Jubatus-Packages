# Jubatus-Packages

`jps` için paket deposu. Her paket gerçek dosya taşıyan, Ed25519 ile imzalı bir `.jps` dosyasıdır.
Paketler Debian deposundan GitHub Actions ile içe aktarılır (amd64: Debian 13, i386: Debian 12) ve `repo` dalına yayınlanır.

    curl -fsSL https://raw.githubusercontent.com/xyronixstudios-coder/Jubatus-Packages/repo/install.sh | sh

    jps search editör          # kütüphaneler için: jps search --all libssl
    sudo jps install jq tmux
    sudo jps autoremove
    jps check                  # kurulu dosyaların sağlama toplamı denetimi
    jps owns /usr/bin/jq

- `packages.list`: aday listesi (boşlukla ayrılmış adlar)
- `scripts/import-debs.sh`: Debian konteynerinde kapanışı çıkarır, .deb'leri jps ağacına çevirir
- `scripts/build-all.sh`: paketleri üretir, imzalar, indeksi yazar
- `real/`: Jubatus'un kendi paketleri
- `forge/`: jps kaynak kodu (jps-64 ve jps-32 buradan derlenir)
- Gerekli secret: `JPS_SIGNING_KEY`
- Elenen aday paketler ve nedenleri: repo dalındaki `skipped-<mimari>.txt`

## Güven

Depo imza anahtarı parmak izi: `79894bcffcff7bfe`

## Servis, kullanıcı ve sistem bağımlılığı

- Paketin `JPS/sysusers` dosyası (`g <grup>` / `u <ad> <grup|-> <home|-> <shell|->`) kurulumda sistem kullanıcısı/grubu oluşturur.
- `JPS/services` dosyası (`birim.service [enable] [start]`) servisi etkinleştirir/başlatır, `jps remove` durdurur.
- `Depends-System: a, b` alanı, `dpkg`'nin kurduğu paketleri bekler (`sysdeps.txt`).
- `overrides/<paket>/` klasörü içe aktarılan Debian paketine elle yazılmış betik, kullanıcı, servis ve ek dosya ekler.
  `allow` dosyası olan paketin Debian bakımcı betikleri çalıştırılmaz, yerine override kullanılır.
- `scripts/show-scripts.sh <paket>`: override yazmadan önce Debian betiklerini gösterir.

## Çoklu depo ve güvenlik

    sudo jps repo add benim https://ornek.com/jps-repo
    jps repo list
    sudo jps repo remove benim
    sudo jps untrust <parmak-izi>

Aynı adlı pakette önce gelen depo kazanır (varsayılan depo en önce). Depo anahtarını güvenilir yapmak,
o depo sahibine kök yetkisiyle paket kurma izni vermektir. Depo sahibi paketleri `jps sign`, dizini `jps sign-file index` ile imzalar.

    jps hold htop      # jps upgrade dokunmaz
    jps unhold htop
    jps verify-file dosya      # dosya.sig'i güvenilir anahtarlarla doğrular (openssl ile de doğrulanır)
