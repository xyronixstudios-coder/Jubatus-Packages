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

## Güven

Depo imza anahtarı parmak izi: `79894bcffcff7bfe`
Kurulumda `jps trust-repo` çıktısındaki değerle karşılaştır.

## Gerçek paketler (`real/`)

`real/<ad>/JPS/control` + `real/<ad>/root/...` düzenindeki klasörler olduğu gibi pakete çevrilir:
`jubatus-sysinfo`, `jubatus-cleanup`, `jubatus-backup`, `jubatus-netcheck`.
`jubatus-forge` (jps'in kendisi) amd64 ve i386 olarak otomatik eklenir.
Paketler zstd ile sıkıştırılır; eski gzip paketler de okunur.
