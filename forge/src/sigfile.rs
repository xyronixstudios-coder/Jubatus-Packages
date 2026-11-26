//! Dosya imzalama (saf Ed25519, RFC 8032: openssl ile de doğrulanabilir) ve güvenilir anahtarlar

use crate::extra::{load_key, trusted_dirs, unhex};
use crate::format::sha_hex;
use anyhow::{bail, Context, Result};
use ed25519_dalek::{Signature, Signer, VerifyingKey};
use std::fs;
use std::path::{Path, PathBuf};

pub fn parse_pub(t: &str) -> Option<[u8; 32]> {
    let b = unhex(t.trim()).ok()?;
    b.try_into().ok()
}

pub fn trusted_keys() -> Vec<[u8; 32]> {
    let mut v: Vec<[u8; 32]> = Vec::new();
    for d in trusted_dirs() {
        if let Ok(rd) = fs::read_dir(d) {
            for e in rd.flatten() {
                if let Ok(t) = fs::read_to_string(e.path()) {
                    if let Some(k) = parse_pub(&t) {
                        if !v.contains(&k) {
                            v.push(k);
                        }
                    }
                }
            }
        }
    }
    v
}

pub fn verify_bytes(data: &[u8], sig: &[u8], keys: &[[u8; 32]]) -> Option<[u8; 32]> {
    let sb: [u8; 64] = sig.try_into().ok()?;
    let sig = Signature::from_bytes(&sb);
    for k in keys {
        if let Ok(vk) = VerifyingKey::from_bytes(k) {
            if vk.verify_strict(data, &sig).is_ok() {
                return Some(*k);
            }
        }
    }
    None
}

pub fn sign_file(file: &Path, key: Option<PathBuf>) -> Result<()> {
    let sk = load_key(key)?;
    let data = fs::read(file).with_context(|| format!("okunamadı: {}", file.display()))?;
    let sig = sk.sign(&data);
    let out = PathBuf::from(format!("{}.sig", file.display()));
    fs::write(&out, sig.to_bytes())?;
    println!(
        "✔ imzalandı: {} (parmak izi {})",
        out.display(),
        &sha_hex(&sk.verifying_key().to_bytes())[..16]
    );
    Ok(())
}

pub fn verify_file(file: &Path, sig: Option<PathBuf>, pubkey: Option<String>) -> Result<()> {
    let sigp = sig.unwrap_or_else(|| PathBuf::from(format!("{}.sig", file.display())));
    let data = fs::read(file).with_context(|| format!("okunamadı: {}", file.display()))?;
    let sb = fs::read(&sigp).with_context(|| format!("imza okunamadı: {}", sigp.display()))?;
    let keys = match pubkey {
        Some(h) => vec![parse_pub(&h).context("geçersiz --pubkey (64 hex karakter olmalı)")?],
        None => trusted_keys(),
    };
    if keys.is_empty() {
        bail!("güvenilir anahtar yok (jps trust ... veya --pubkey)");
    }
    match verify_bytes(&data, &sb, &keys) {
        Some(k) => {
            println!("✔ imza geçerli ({})", &sha_hex(&k)[..16]);
            Ok(())
        }
        None => bail!("İMZA GEÇERSİZ: {}", file.display()),
    }
}

pub fn untrust(fpr: &str) -> Result<()> {
    if fpr.len() != 16 || !fpr.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("parmak izi 16 hex karakter olmalı");
    }
    let fpr = fpr.to_lowercase();
    let mut n = 0;
    for d in trusted_dirs() {
        let p = d.join(format!("{fpr}.pub"));
        if p.is_file() {
            fs::remove_file(&p).with_context(|| format!("silinemedi: {} (sudo gerekir)", p.display()))?;
            println!("✔ kaldırıldı: {}", p.display());
            n += 1;
        }
    }
    if n == 0 {
        bail!("bu parmak izi güvenilir listede yok: {fpr}");
    }
    Ok(())
}
