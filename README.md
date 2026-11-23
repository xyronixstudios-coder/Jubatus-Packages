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
