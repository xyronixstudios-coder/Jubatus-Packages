# Jubatus-Packages

`jps` için paket deposu. Paketler GitHub Actions ile derlenir, imzalanır ve `repo` dalına yayınlanır.

    curl -fsSL https://raw.githubusercontent.com/xyronixstudios-coder/Jubatus-Packages/repo/install.sh | sh

    jps search editör
    sudo jps install htop
    sudo jps upgrade

- `packages.list`: paket kataloğu (ad|açıklama|bağımlılıklar|apt adı)
- `scripts/build-all.sh`: paketleri üretir, indeksi yazar (en az 150 paket şartı)
- `forge/`: jps kaynak kodu (jps-64 ve jps-32 buradan derlenir)
- Gerekli secret: `JPS_SIGNING_KEY` (~/.config/jps/signing.key içeriği)
