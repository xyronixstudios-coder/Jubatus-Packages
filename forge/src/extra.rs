//! Simge, küçük resim, masaüstü entegrasyonu ve Ed25519 imza desteği

use crate::format::*;
use anyhow::{anyhow, bail, Context, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use image::{imageops::FilterType, ImageFormat};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Logo ikili dosyanın içine gömülür: jps nereye giderse simge de onunla gider.
const LOGO: &[u8] = include_bytes!("../assets/jps-logo.png");
const ICON_NAME: &str = "application-x-jps";

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from).context("HOME tanımlı değil")
}

pub fn cfg_dir() -> Result<PathBuf> {
    Ok(home()?.join(".config/jps"))
}

pub fn unhex(s: &str) -> Result<Vec<u8>> {
    let s = s.trim();
    if !s.is_ascii() || s.len() % 2 != 0 {
        bail!("geçersiz hex");
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).context("geçersiz hex"))
        .collect()
}

pub fn write_default_icon(dir: &Path) -> Result<()> {
    fs::write(dir.join("JPS/icon.png"), LOGO)?;
    Ok(())
}

// ───────────────────────── simge ─────────────────────────

pub fn set_icon(file: &Path, png: &Path) -> Result<()> {
    let bytes = fs::read(png).with_context(|| format!("okunamadı: {}", png.display()))?;
    if bytes.len() > 4 * 1024 * 1024 {
        bail!("simge 4 MB'tan büyük olamaz");
    }
    let mut img = image::load_from_memory(&bytes).context("geçerli bir PNG değil")?;
    if img.width() > 256 || img.height() > 256 {
        img = img.resize(256, 256, FilterType::Lanczos3);
    }
    let mut buf: Vec<u8> = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), ImageFormat::Png)?;

    let mut pkg = read_package(file)?;
    if pkg.get("signature").is_some() {
        eprintln!("uyarı: paket değişti, eski imza kaldırıldı. Yeniden imzala: jps sign {}", file.display());
    }
    pkg.sections.retain(|s| s.name != "icon" && s.name != "signature");
    pkg.sections.push(Section { name: "icon".into(), data: buf });
    write_package(file, &pkg.sections)?;
    Ok(())
}

/// Dosya yöneticisi için: paketin kendi simgesini (yoksa varsayılan logoyu) PNG olarak yazar.
pub fn thumbnail(input: &Path, output: &Path, size: u32) -> Result<()> {
    let small = fs::metadata(input).map(|m| m.len() < 512 * 1024 * 1024).unwrap_or(false);
    let png: Vec<u8> = if small {
        match read_package(input) {
            Ok(p) => p.get("icon").map(|d| d.to_vec()),
            Err(_) => None,
        }
        .unwrap_or_else(|| LOGO.to_vec())
    } else {
        LOGO.to_vec()
    };
    let img = image::load_from_memory(&png).or_else(|_| image::load_from_memory(LOGO))?;
    let size = size.clamp(16, 512);
    img.resize(size, size, FilterType::Lanczos3)
        .save_with_format(output, ImageFormat::Png)?;
    Ok(())
}

// ───────────────────────── masaüstü entegrasyonu ─────────────────────────

fn run(cmd: &str, args: &[&str]) {
    let _ = Command::new(cmd)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn base_dir(system: bool) -> Result<PathBuf> {
    Ok(if system { PathBuf::from("/usr/share") } else { home()?.join(".local/share") })
}

pub fn desktop_install(system: bool) -> Result<()> {
    let base = base_dir(system)?;
    let cur = std::env::current_exe()?.canonicalize()?;
    let exe = if system && Path::new("/usr/local/bin/jps").exists() {
        PathBuf::from("/usr/local/bin/jps")
    } else {
        cur
    };
    let exe_s = exe.to_string_lossy().into_owned();

    // 1) MIME türü
    let mime_dir = base.join("mime/packages");
    fs::create_dir_all(&mime_dir)
        .with_context(|| format!("{} yazılamadı (--system için sudo gerekir)", mime_dir.display()))?;
    fs::write(
        mime_dir.join("application-x-jps.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-jps">
    <comment>Jubatus Forge package</comment>
    <comment xml:lang="tr">Jubatus Forge paketi</comment>
    <icon name="application-x-jps"/>
    <generic-icon name="package-x-generic"/>
    <magic priority="60">
      <match type="string" value="JPSPKG" offset="0"/>
    </magic>
    <glob pattern="*.jps"/>
  </mime-type>
</mime-info>
"#,
    )?;

    // 2) Simgeler
    let logo = image::load_from_memory(LOGO)?;
    for s in [16u32, 22, 24, 32, 48, 64, 128, 256] {
        let d = base.join(format!("icons/hicolor/{s}x{s}/mimetypes"));
        fs::create_dir_all(&d)?;
        logo.resize(s, s, FilterType::Lanczos3)
            .save_with_format(d.join(format!("{ICON_NAME}.png")), ImageFormat::Png)?;
    }

    // 3) Küçük resim üretici: her .jps kendi simgesiyle görünür
    let th = base.join("thumbnailers");
    fs::create_dir_all(&th)?;
    fs::write(
        th.join("jps.thumbnailer"),
        format!(
            "[Thumbnailer Entry]\nTryExec={exe_s}\nExec=\"{exe_s}\" thumbnail %i %o %s\nMimeType=application/x-jps;\n"
        ),
    )?;

    // 4) Çift tıklayınca açan uygulama girdisi
    let apps = base.join("applications");
    fs::create_dir_all(&apps)?;
    fs::write(
        apps.join("jubatus-forge.desktop"),
        format!(
            "[Desktop Entry]\nType=Application\nName=Jubatus Forge\nComment=.jps paketini incele ve kur\nExec=\"{exe_s}\" open %f\nIcon={ICON_NAME}\nTerminal=false\nNoDisplay=true\nMimeType=application/x-jps;\nCategories=System;PackageManager;\n"
        ),
    )?;

    // 5) Önbellekleri tazele
    let b = base.to_string_lossy().into_owned();
    run("update-mime-database", &[&format!("{b}/mime")]);
    run("gtk-update-icon-cache", &["-f", "-t", &format!("{b}/icons/hicolor")]);
    run("update-desktop-database", &[&format!("{b}/applications")]);
    if !system {
        run("xdg-mime", &["default", "jubatus-forge.desktop", "application/x-jps"]);
    }
    println!("✔ masaüstü entegrasyonu kuruldu ({})", base.display());
    println!("  Dosya yöneticisini yeniden başlat: nautilus -q  (veya oturumu kapat/aç)");
    Ok(())
}

pub fn desktop_remove(system: bool) -> Result<()> {
    let base = base_dir(system)?;
    let _ = fs::remove_file(base.join("mime/packages/application-x-jps.xml"));
    let _ = fs::remove_file(base.join("thumbnailers/jps.thumbnailer"));
    let _ = fs::remove_file(base.join("applications/jubatus-forge.desktop"));
    for s in [16u32, 22, 24, 32, 48, 64, 128, 256] {
        let _ = fs::remove_file(base.join(format!("icons/hicolor/{s}x{s}/mimetypes/{ICON_NAME}.png")));
    }
    let b = base.to_string_lossy().into_owned();
    run("update-mime-database", &[&format!("{b}/mime")]);
    run("gtk-update-icon-cache", &["-f", "-t", &format!("{b}/icons/hicolor")]);
    run("update-desktop-database", &[&format!("{b}/applications")]);
    println!("✔ masaüstü entegrasyonu kaldırıldı");
    Ok(())
}

/// Dosya yöneticisinden çift tıklama: incele, doğrula, kurmak ister misin?
pub fn open(file: &Path) -> Result<()> {
    let r = (|| -> Result<()> {
        crate::ops::info(file)?;
        println!();
        crate::ops::verify(file)?;
        print_sig(file)?;
        print!("\nKurulsun mu? (sudo gerekir) [e/H] ");
        std::io::stdout().flush()?;
        let mut a = String::new();
        std::io::stdin().read_line(&mut a)?;
        if a.trim().eq_ignore_ascii_case("e") {
            let exe = std::env::current_exe()?;
            let st = Command::new("sudo").arg(exe).arg("install").arg(file).status()?;
            if !st.success() {
                bail!("kurulum başarısız");
            }
        }
        Ok(())
    })();
    if let Err(e) = &r {
        eprintln!("jps: hata: {e:#}");
    }
    print!("\nKapatmak için Enter...");
    let _ = std::io::stdout().flush();
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
    r
}

// ───────────────────────── imza (Ed25519) ─────────────────────────

fn signed_digest(pkg: &Package) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"JPS-SIG-v1\0");
    for s in pkg.sections.iter().filter(|s| s.name != "signature") {
        h.update([s.name.len() as u8]);
        h.update(s.name.as_bytes());
        h.update(Sha256::digest(&s.data));
    }
    h.finalize().into()
}

pub fn keygen() -> Result<()> {
    let dir = cfg_dir()?;
    fs::create_dir_all(&dir)?;
    let kp = dir.join("signing.key");
    if kp.exists() {
        bail!("{} zaten var; üzerine yazmıyorum", kp.display());
    }
    let mut seed = [0u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut seed)?;
    let sk = SigningKey::from_bytes(&seed);
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&kp)?
        .write_all(hex(&seed).as_bytes())?;
    let vk = sk.verifying_key();
    let pubhex = hex(vk.as_bytes());
    fs::write(dir.join("signing.pub"), &pubhex)?;
    println!("✔ anahtar çifti üretildi");
    println!("  gizli anahtar : {}  (kimseyle paylaşma!)", kp.display());
    println!("  genel anahtar : {}", dir.join("signing.pub").display());
    println!("  parmak izi    : {}", &sha_hex(vk.as_bytes())[..16]);
    Ok(())
}

pub fn load_key(path: Option<PathBuf>) -> Result<SigningKey> {
    let p = match path {
        Some(p) => p,
        None => cfg_dir()?.join("signing.key"),
    };
    let t = fs::read_to_string(&p)
        .with_context(|| format!("anahtar okunamadı: {} (önce: jps keygen)", p.display()))?;
    let seed: [u8; 32] = unhex(&t)?
        .try_into()
        .map_err(|_| anyhow!("gizli anahtar 32 bayt olmalı"))?;
    Ok(SigningKey::from_bytes(&seed))
}

pub fn sign(file: &Path, key: Option<PathBuf>) -> Result<()> {
    let sk = load_key(key)?;
    let mut pkg = read_package(file)?;
    pkg.sections.retain(|s| s.name != "signature");
    let digest = signed_digest(&pkg);
    let sig = sk.sign(&digest);
    let mut data = sk.verifying_key().to_bytes().to_vec();
    data.extend_from_slice(&sig.to_bytes());
    pkg.sections.push(Section { name: "signature".into(), data });
    write_package(file, &pkg.sections)?;
    println!(
        "✔ imzalandı: {} (parmak izi {})",
        file.display(),
        &sha_hex(&sk.verifying_key().to_bytes())[..16]
    );
    Ok(())
}

pub enum Sig {
    Missing,
    Bad(String),
    Valid { fp: String, trusted: bool },
}

pub fn trusted_dirs() -> Vec<PathBuf> {
    let mut v = vec![PathBuf::from("/etc/jps/trusted")];
    if let Ok(c) = cfg_dir() {
        v.push(c.join("trusted"));
    }
    v
}

fn is_trusted(pubhex: &str) -> bool {
    for d in trusted_dirs() {
        if let Ok(rd) = fs::read_dir(d) {
            for e in rd.flatten() {
                if let Ok(t) = fs::read_to_string(e.path()) {
                    if t.trim().eq_ignore_ascii_case(pubhex) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

pub fn sig_status(pkg: &Package) -> Sig {
    let data = match pkg.get("signature") {
        Some(d) => d,
        None => return Sig::Missing,
    };
    if data.len() != 96 {
        return Sig::Bad("imza bölümü bozuk".into());
    }
    let pk: [u8; 32] = match data[..32].try_into() {
        Ok(x) => x,
        Err(_) => return Sig::Bad("genel anahtar bozuk".into()),
    };
    let sb: [u8; 64] = match data[32..].try_into() {
        Ok(x) => x,
        Err(_) => return Sig::Bad("imza bozuk".into()),
    };
    let vk = match VerifyingKey::from_bytes(&pk) {
        Ok(v) => v,
        Err(_) => return Sig::Bad("geçersiz genel anahtar".into()),
    };
    let sig = Signature::from_bytes(&sb);
    let digest = signed_digest(pkg);
    if vk.verify(&digest, &sig).is_err() {
        return Sig::Bad("İMZA GEÇERSİZ: paket imzalandıktan sonra değiştirilmiş".into());
    }
    Sig::Valid { fp: sha_hex(&pk)[..16].to_string(), trusted: is_trusted(&hex(&pk)) }
}

pub fn print_sig(file: &Path) -> Result<()> {
    let pkg = read_package(file)?;
    match sig_status(&pkg) {
        Sig::Missing => println!("İmza        : yok (imzasız paket)"),
        Sig::Bad(m) => println!("İmza        : ✘ {m}"),
        Sig::Valid { fp, trusted: true } => println!("İmza        : ✔ geçerli ve GÜVENİLİR ({fp})"),
        Sig::Valid { fp, trusted: false } => {
            println!("İmza        : ✔ geçerli ama anahtar güvenilir listede değil ({fp})")
        }
    }
    Ok(())
}

pub fn check_install(file: &Path, require_trusted: bool) -> Result<()> {
    let pkg = read_package(file)?;
    match sig_status(&pkg) {
        Sig::Missing => {
            if require_trusted {
                bail!("paket imzasız (--require-trusted)");
            }
            eprintln!("uyarı: paket imzasız");
        }
        Sig::Bad(m) => bail!("{m}"),
        Sig::Valid { fp, trusted } => {
            if trusted {
                eprintln!("✔ güvenilir imza ({fp})");
            } else if require_trusted {
                bail!("imza geçerli ama anahtar ({fp}) güvenilir değil; önce: jps trust <dosya>");
            } else {
                eprintln!("uyarı: imza geçerli ama anahtar ({fp}) güvenilir listede değil");
            }
        }
    }
    Ok(())
}

/// `jps trust anahtar.pub` veya `jps trust paket.jps` (içindeki imza anahtarını güvenilir yapar)
pub fn trust(file: &Path, system: bool) -> Result<()> {
    let raw = fs::read(file)?;
    let pubhex = if raw.starts_with(MAGIC) {
        let p = read_package(file)?;
        let s = p.get("signature").context("pakette imza yok")?;
        if s.len() != 96 {
            bail!("imza bölümü bozuk");
        }
        hex(&s[..32])
    } else {
        String::from_utf8(raw)?.trim().to_string()
    };
    let bytes = unhex(&pubhex)?;
    if bytes.len() != 32 {
        bail!("genel anahtar 32 bayt (64 hex) olmalı");
    }
    let dir = if system { PathBuf::from("/etc/jps/trusted") } else { cfg_dir()?.join("trusted") };
    fs::create_dir_all(&dir).with_context(|| format!("{} yazılamadı", dir.display()))?;
    let fp = &sha_hex(&bytes)[..16];
    fs::write(dir.join(format!("{fp}.pub")), &pubhex)?;
    println!("✔ güvenilir yapıldı: {fp} → {}", dir.display());
    Ok(())
}
