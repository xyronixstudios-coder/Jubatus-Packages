//! jps self-update: depodan jps-64 / jps-32 indirir, sha256 doğrular, kendini değiştirir.

use crate::format::sha_hex;
use crate::repo::{fetch, repo_url, vercmp};
use anyhow::{bail, Context, Result};
use std::cmp::Ordering;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

pub fn run(force: bool) -> Result<()> {
    let exe = std::env::current_exe()?.canonicalize()?;
    let dir = exe.parent().context("ikili dizini bulunamadı")?.to_path_buf();
    let bin = if cfg!(target_pointer_width = "64") { "jps-64" } else { "jps-32" };
    let newp = dir.join(".jps-update.bin");
    let sums = dir.join(".jps-update.sums");
    let r = inner(&exe, bin, &newp, &sums, force);
    let _ = fs::remove_file(&newp);
    let _ = fs::remove_file(&sums);
    r
}

fn inner(exe: &Path, bin: &str, newp: &Path, sums: &Path, force: bool) -> Result<()> {
    let cur = env!("CARGO_PKG_VERSION");
    let url = repo_url(Path::new("/"));
    println!("Mevcut sürüm: {cur} — depo: {url}");

    fs::write(newp, b"").with_context(|| {
        format!("{} yazılamadı (sudo jps self-update dene)", newp.parent().unwrap_or(Path::new("/")).display())
    })?;
    fetch(&format!("{url}/{bin}"), newp)?;
    fetch(&format!("{url}/sha256sums"), sums)?;

    let list = fs::read_to_string(sums)?;
    let want = list
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let h = it.next()?;
            let n = it.next()?.trim_start_matches('*');
            (n == bin).then(|| h.to_lowercase())
        })
        .next()
        .with_context(|| format!("sha256sums içinde {bin} yok"))?;
    if sha_hex(&fs::read(newp)?) != want {
        bail!("{bin}: sağlama toplamı uyuşmuyor, güncelleme iptal edildi");
    }

    let sigp = newp.with_extension("sig");
    fetch(&format!("{url}/{bin}.sig"), &sigp)
        .context("imza dosyası indirilemedi (depo imzalı ikili sunmuyor)")?;
    let sigb = fs::read(&sigp)?;
    let _ = fs::remove_file(&sigp);
    let keys = crate::sigfile::trusted_keys();
    if keys.is_empty() {
        bail!("güvenilir anahtar yok: önce 'sudo jps trust-repo' (veya jps trust <anahtar>)");
    }
    match crate::sigfile::verify_bytes(&fs::read(newp)?, &sigb, &keys) {
        Some(k) => println!("✔ ikili imzası geçerli ({})", &sha_hex(&k)[..16]),
        None => bail!("İKİLİ İMZASI GEÇERSİZ: güncelleme iptal edildi"),
    }
    fs::set_permissions(newp, fs::Permissions::from_mode(0o755))?;
    let out = Command::new(newp)
        .arg("--version")
        .output()
        .context("yeni ikili çalıştırılamadı (mimari uyuşmuyor olabilir)")?;
    if !out.status.success() {
        bail!("yeni ikili --version çalıştıramadı");
    }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let newv = text.split_whitespace().last().context("sürüm okunamadı")?.to_string();

    if vercmp(&newv, cur) != Ordering::Greater && !force {
        println!("✔ jps zaten güncel ({cur})");
        return Ok(());
    }
    fs::rename(newp, exe)?; // aynı dizin: atomik değişim
    println!("✔ jps {cur} → {newv}");
    if Path::new("/usr/share/mime/packages/application-x-jps.xml").exists() {
        let _ = Command::new(exe).args(["desktop-install", "--system"]).status();
    }
    Ok(())
}
