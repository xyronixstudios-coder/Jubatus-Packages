# Jubatus-Packages

`jps` için paket deposu. Paketler **dağıtımdan bağımsızdır**: upstream kaynaktan (veya Rust/Go modüllerinden)
statik musl olarak derlenir, `/usr/local` altına kurulur, Ed25519 ile imzalanır. Debian/RPM/Arch paketine bağlı değildir.

    curl -fsSL https://raw.githubusercontent.com/xyronixstudios-coder/Jubatus-Packages/repo/install.sh | sh

    jps search editör
    sudo jps install jq tmux ripgrep
    sudo jps upgrade

- `recipes/`: tarifler (C/autotools, Rust ve Go katalogları). Biçim için `recipes/README.md`.
- `scripts/forge/`: derleyici (`build-all.sh`), kaynak pinleme (`pin.sh`), CI giriş betiği.
- `recipes.lock`: kaynak URL → SHA-256.
- `scripts/build-all.sh`: derlenen ağaçları `.jps`'e çevirir, imzalar, dizini yazar.
- `real/`: Jubatus'un kendi paketleri. `forge/`: jps kaynak kodu.
- `legacy/`: eski Debian içe aktarma betikleri (artık CI'da çalışmaz).
- Elenen tarifler ve nedenleri: repo dalındaki `skipped-<mimari>.txt`.

## Güven

Depo imza anahtarı parmak izi: `79894bcffcff7bfe`
Statik ikililer yalnızca çekirdek ve CPU mimarisine (amd64, i386) bağlıdır.

## Servis, kullanıcı, çoklu depo, güvenlik

    sudo jps repo add benim https://ornek.com/jps-repo
    jps repo list
    jps hold htop
    jps verify-file dosya      # dosya.sig, openssl ile de doğrulanır
