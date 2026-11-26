//! .jps konteyner formatı
//!
//! Düzen (hepsi little-endian):
//!   MAGIC (8 bayt) = "JPSPKG\x01\n"
//!   bölüm sayısı   (u32)
//!   her bölüm için:
//!     ad uzunluğu (u8) | ad (utf-8) | veri uzunluğu (u64) | sha256 (32 bayt) | veri
//!
//! Standart bölümler: control, manifest, preinst, postinst, prerm, postrm, data (tar.gz)

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};

pub const MAGIC: &[u8; 8] = b"JPSPKG\x01\n";
pub const SCRIPTS: [&str; 4] = ["preinst", "postinst", "prerm", "postrm"];

pub struct Section {
    pub name: String,
    pub data: Vec<u8>,
}

pub struct Package {
    pub sections: Vec<Section>,
}

impl Package {
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.sections
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.data.as_slice())
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn sha_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

pub fn write_package(path: &Path, sections: &[Section]) -> Result<()> {
    let tmp = path.with_extension("jps.tmp");
    {
        let mut w = BufWriter::new(
            File::create(&tmp).with_context(|| format!("oluşturulamadı: {}", tmp.display()))?,
        );
        w.write_all(MAGIC)?;
        w.write_all(&(sections.len() as u32).to_le_bytes())?;
        for s in sections {
            let name = s.name.as_bytes();
            if name.is_empty() || name.len() > 255 {
                bail!("geçersiz bölüm adı: {}", s.name);
            }
            w.write_all(&[name.len() as u8])?;
            w.write_all(name)?;
            w.write_all(&(s.data.len() as u64).to_le_bytes())?;
            w.write_all(&Sha256::digest(&s.data))?;
            w.write_all(&s.data)?;
        }
        w.flush()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

fn take<'a>(buf: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8]> {
    let end = pos
        .checked_add(n)
        .filter(|e| *e <= buf.len())
        .context("dosya kesilmiş veya bozuk (.jps)")?;
    let s = &buf[*pos..end];
    *pos = end;
    Ok(s)
}

/// Dosyayı okur ve TÜM bölüm sağlama toplamlarını doğrular.
pub fn read_package(path: &Path) -> Result<Package> {
    let buf = fs::read(path).with_context(|| format!("okunamadı: {}", path.display()))?;
    let mut pos = 0usize;
    if take(&buf, &mut pos, 8)? != MAGIC {
        bail!("bu bir .jps dosyası değil (magic uyuşmuyor)");
    }
    let count = u32::from_le_bytes(take(&buf, &mut pos, 4)?.try_into()?);
    if count == 0 || count > 64 {
        bail!("geçersiz bölüm sayısı: {count}");
    }
    let mut sections: Vec<Section> = Vec::new();
    for _ in 0..count {
        let nlen = take(&buf, &mut pos, 1)?[0] as usize;
        let name = std::str::from_utf8(take(&buf, &mut pos, nlen)?)
            .context("bölüm adı utf-8 değil")?
            .to_string();
        let len = u64::from_le_bytes(take(&buf, &mut pos, 8)?.try_into()?);
        let len = usize::try_from(len).context("bölüm çok büyük")?;
        let hash = take(&buf, &mut pos, 32)?;
        let data = take(&buf, &mut pos, len)?;
        if Sha256::digest(data).as_slice() != hash {
            bail!("SAĞLAMA HATASI: '{name}' bölümü bozuk veya değiştirilmiş");
        }
        if sections.iter().any(|s| s.name == name) {
            bail!("tekrarlanan bölüm: {name}");
        }
        sections.push(Section { name, data: data.to_vec() });
    }
    if pos != buf.len() {
        bail!("dosya sonunda beklenmeyen fazla veri var");
    }
    Ok(Package { sections })
}

/// Arşiv içi yolu güvenli, göreli yola çevirir. ".." veya mutlak yol -> None.
pub fn safe_rel(p: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in Path::new(p).components() {
        match c {
            Component::Normal(x) => out.push(x),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(out)
}

pub fn valid_name(n: &str) -> bool {
    let mut ch = n.chars();
    match ch.next() {
        Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit() => {}
        _ => return false,
    }
    n.len() >= 2
        && n.len() <= 100
        && n.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "+.-".contains(c))
}

#[derive(Debug, Clone)]
pub struct Control {
    pub package: String,
    pub version: String,
    pub architecture: String,
    pub maintainer: String,
    pub description: String,
    pub depends: Vec<String>,
    pub sysdepends: Vec<String>,
}

impl Control {
    pub fn parse(text: &str) -> Result<Control> {
        let mut m: BTreeMap<String, String> = BTreeMap::new();
        for (i, line) in text.lines().enumerate() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let (k, v) = t
                .split_once(':')
                .with_context(|| format!("control satır {}: 'Anahtar: değer' bekleniyordu", i + 1))?;
            m.insert(k.trim().to_lowercase(), v.trim().to_string());
        }
        let req = |k: &str| -> Result<String> {
            match m.get(k) {
                Some(v) if !v.is_empty() => Ok(v.clone()),
                _ => bail!("control: '{k}' alanı zorunlu"),
            }
        };
        let package = req("package")?;
        let version = req("version")?;
        let architecture = req("architecture")?;
        let maintainer = req("maintainer")?;
        let description = req("description")?;
        if !valid_name(&package) {
            bail!("geçersiz paket adı '{package}' (küçük harf, rakam, + . - ; en az 2 karakter)");
        }
        if !version.chars().next().map_or(false, |c| c.is_ascii_digit())
            || !version
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || ".+-~_".contains(c))
        {
            bail!("geçersiz sürüm '{version}' (rakamla başlamalı)");
        }
        if !architecture
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            bail!("geçersiz mimari '{architecture}'");
        }
        let depends: Vec<String> = m
            .get("depends")
            .map(|d| {
                d.split(',')
                    .filter_map(|x| {
                        x.trim()
                            .split(|c: char| c.is_whitespace() || c == '(')
                            .next()
                            .map(|s| s.trim().to_string())
                    })
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let sysdepends: Vec<String> = m
            .get("depends-system")
            .map(|d| d.split(',').map(|x| x.trim().to_string()).filter(|s| valid_name(s)).collect())
            .unwrap_or_default();
        Ok(Control { package, version, architecture, maintainer, description, depends, sysdepends })
    }
}

/// data bölümünü açar: zstd (28 b5 2f fd) veya eski gzip (1f 8b) paketleri okunur.
pub fn decompress<'a>(data: &'a [u8]) -> Result<Box<dyn Read + 'a>> {
    if data.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        Ok(Box::new(zstd::stream::read::Decoder::new(data)?))
    } else if data.starts_with(&[0x1f, 0x8b]) {
        Ok(Box::new(flate2::read::GzDecoder::new(data)))
    } else {
        bail!("bilinmeyen sıkıştırma biçimi (data bölümü bozuk)")
    }
}

/// Varsayılan zstd seviye 19. JPS_COMPRESSION=gzip eski istemcilerle uyum içindir.
pub fn compress(raw: &[u8]) -> Result<Vec<u8>> {
    if std::env::var("JPS_COMPRESSION").map_or(false, |v| v == "gzip") {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        e.write_all(raw)?;
        return Ok(e.finish()?);
    }
    let lvl: i32 = std::env::var("JPS_ZSTD_LEVEL").ok().and_then(|v| v.parse().ok()).unwrap_or(19);
    Ok(zstd::stream::encode_all(raw, lvl)?)
}
