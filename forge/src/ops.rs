use crate::format::*;
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{ErrorKind, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Opts {
    pub root: PathBuf,
    pub force: bool,
    pub no_scripts: bool,
}

#[derive(Clone)]
pub struct Entry {
    pub kind: char, // f = dosya, d = dizin, l = sembolik bağ
    pub hash: String,
    pub path: String,
}

fn db_dir(root: &Path) -> PathBuf {
    root.join("var/lib/jps")
}

pub fn host_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "i386",
        "arm" => "armhf",
        o => o,
    }
}

fn manifest_text(entries: &[Entry]) -> String {
    let mut s = String::new();
    for e in entries {
        s.push_str(&format!("{}\t{}\t{}\n", e.kind, e.hash, e.path));
    }
    s
}

fn parse_manifest(text: &str) -> Result<Vec<Entry>> {
    let mut v = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let mut it = line.splitn(3, '\t');
        let (k, h, p) = match (it.next(), it.next(), it.next()) {
            (Some(k), Some(h), Some(p)) => (k, h, p),
            _ => bail!("bozuk manifest satırı"),
        };
        let kind = k
            .chars()
            .next()
            .filter(|c| matches!(c, 'f' | 'd' | 'l'))
            .context("bozuk manifest türü")?;
        if safe_rel(p).is_none() {
            bail!("güvensiz yol: {p}");
        }
        v.push(Entry { kind, hash: h.into(), path: p.into() });
    }
    Ok(v)
}

fn manifest_from_data(data: &[u8]) -> Result<Vec<Entry>> {
    let mut ar = tar::Archive::new(decompress(data)?);
    let mut out = Vec::new();
    for e in ar.entries()? {
        let mut e = e?;
        let p = e.path()?.to_string_lossy().into_owned();
        let rel = safe_rel(&p).with_context(|| format!("arşivde güvensiz yol: {p}"))?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        let path = rel.to_string_lossy().into_owned();
        if path.contains('\n') || path.contains('\t') {
            bail!("dosya adında satır sonu/sekme olamaz: {path:?}");
        }
        let t = e.header().entry_type();
        if t.is_dir() {
            out.push(Entry { kind: 'd', hash: "-".into(), path });
        } else if t.is_symlink() {
            out.push(Entry { kind: 'l', hash: "-".into(), path });
        } else if t.is_file() {
            let mut h = Sha256::new();
            let mut buf = [0u8; 65536];
            loop {
                let n = e.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            out.push(Entry { kind: 'f', hash: hex(&h.finalize()[..]), path });
        } else {
            bail!("desteklenmeyen dosya türü (hardlink/aygıt/fifo): {path}");
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn unpack(data: &[u8], dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    let mut ar = tar::Archive::new(decompress(data)?);
    ar.set_preserve_permissions(true);
    for e in ar.entries()? {
        let mut e = e?;
        let p = e.path()?.to_string_lossy().into_owned();
        let rel = match safe_rel(&p) {
            Some(r) => r,
            None => bail!("güvensiz yol: {p}"),
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        // Var olan dizinlerin izinlerine dokunma (/usr, /etc ...)
        if e.header().entry_type().is_dir() && dest.join(&rel).is_dir() {
            continue;
        }
        if !e.unpack_in(dest)? {
            bail!("güvensiz yol: {p}");
        }
    }
    Ok(())
}

fn control_text(pkg: &Package) -> Result<&str> {
    Ok(std::str::from_utf8(pkg.get("control").context("'control' bölümü yok")?)?)
}

fn load_checked(file: &Path) -> Result<(Package, Control, Vec<Entry>)> {
    let pkg = read_package(file)?;
    let c = Control::parse(control_text(&pkg)?)?;
    let data = pkg.get("data").context("'data' bölümü yok")?;
    let entries = manifest_from_data(data)?;
    let stored = pkg.get("manifest").context("'manifest' bölümü yok")?;
    if manifest_text(&entries).as_bytes() != stored {
        bail!("manifest, data içeriğiyle uyuşmuyor (paket bozuk)");
    }
    Ok((pkg, c, entries))
}

// ───────────────────────── init ─────────────────────────

pub fn init(dir: &Path) -> Result<()> {
    if dir.exists() {
        bail!("{} zaten var", dir.display());
    }
    let name = dir
        .file_name()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| "paket".into());
    fs::create_dir_all(dir.join("JPS"))?;
    fs::create_dir_all(dir.join("root/usr/local/bin"))?;

    fs::write(
        dir.join("JPS/control"),
        format!(
            "Package: {name}\nVersion: 1.0.0\nArchitecture: all\nMaintainer: Adınız <mail@ornek.com>\nDepends:\nDescription: Örnek Jubatus paketi\n"
        ),
    )?;
    let scripts = [
        ("preinst", "# kurulumdan ÖNCE çalışır\n"),
        ("postinst", "echo \"$JPS_PACKAGE $JPS_VERSION kuruldu ($JPS_ACTION)\"\n"),
        ("prerm", "# kaldırmadan ÖNCE çalışır\n"),
        ("postrm", "echo \"$JPS_PACKAGE kaldırıldı\"\n"),
    ];
    for (n, body) in scripts {
        let p = dir.join("JPS").join(n);
        fs::write(&p, format!("#!/bin/sh\nset -e\n{body}"))?;
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755))?;
    }
    let bin = dir.join("root/usr/local/bin").join(format!("{name}-merhaba"));
    fs::write(&bin, "#!/bin/sh\necho \"Merhaba, Jubatus Forge!\"\n")?;
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755))?;
    println!("✔ iskelet hazır: {}", dir.display());
    println!("  JPS/control ve JPS/* betiklerini düzenle, dosyaları root/ altına koy.");
    println!("  Sonra: jps build {}", dir.display());
    Ok(())
}

// ───────────────────────── build ─────────────────────────

pub fn build(dir: &Path, out: Option<PathBuf>) -> Result<PathBuf> {
    let meta = dir.join("JPS");
    let root = dir.join("root");
    let ctext = fs::read_to_string(meta.join("control"))
        .with_context(|| format!("{}/JPS/control okunamadı", dir.display()))?;
    let c = Control::parse(&ctext)?;
    if !root.is_dir() {
        bail!("{}/root dizini yok", dir.display());
    }

    let mut gz: Vec<u8> = Vec::new();
    {
        let mut b = tar::Builder::new(&mut gz);
        b.mode(tar::HeaderMode::Deterministic);
        b.follow_symlinks(false);
        b.append_dir_all(".", &root)?;
        b.finish()?;
    }
    let data = compress(&gz)?;
    let entries = manifest_from_data(&data)?;

    let mut sections = vec![
        Section { name: "control".into(), data: ctext.into_bytes() },
        Section { name: "manifest".into(), data: manifest_text(&entries).into_bytes() },
    ];
    for s in SCRIPTS {
        let p = meta.join(s);
        if p.is_file() {
            sections.push(Section { name: s.into(), data: fs::read(&p)? });
        }
    }
    sections.push(Section { name: "data".into(), data });

    let out = out.unwrap_or_else(|| {
        PathBuf::from(format!("{}_{}_{}.jps", c.package, c.version, c.architecture))
    });
    write_package(&out, &sections)?;
    Ok(out)
}

// ───────────────────────── bilgi ─────────────────────────

pub fn info(file: &Path) -> Result<()> {
    let pkg = read_package(file)?;
    let c = Control::parse(control_text(&pkg)?)?;
    println!("Paket       : {}", c.package);
    println!("Sürüm       : {}", c.version);
    println!("Mimari      : {}", c.architecture);
    println!("Bakımcı     : {}", c.maintainer);
    println!(
        "Bağımlılık  : {}",
        if c.depends.is_empty() { "-".to_string() } else { c.depends.join(", ") }
    );
    println!("Açıklama    : {}", c.description);
    println!("\nBölümler:");
    for s in &pkg.sections {
        println!("  {:<10} {:>12} bayt  sha256:{}", s.name, s.data.len(), &sha_hex(&s.data)[..16]);
    }
    Ok(())
}

pub fn contents(file: &Path) -> Result<()> {
    let (_, _, entries) = load_checked(file)?;
    for e in &entries {
        let t = match e.kind {
            'd' => "dizin",
            'l' => "bağ  ",
            _ => "dosya",
        };
        println!("{t}  {}", e.path);
    }
    Ok(())
}

pub fn verify(file: &Path) -> Result<()> {
    let (pkg, c, entries) = load_checked(file)?;
    println!(
        "✔ {} {} — {} bölüm, {} girdi, tüm sağlama toplamları doğru",
        c.package,
        c.version,
        pkg.sections.len(),
        entries.len()
    );
    Ok(())
}

pub fn extract(file: &Path, dest: &Path) -> Result<()> {
    let (pkg, _, _) = load_checked(file)?;
    unpack(pkg.get("data").context("data yok")?, dest)?;
    println!("✔ çıkarıldı: {}", dest.display());
    Ok(())
}

// ───────────────────────── kurulum ─────────────────────────

fn run_script(name: &str, script: Option<&[u8]>, c: &Control, action: &str, o: &Opts) -> Result<()> {
    let script = match script {
        Some(s) if !o.no_scripts => s,
        _ => return Ok(()),
    };
    let s = std::str::from_utf8(script).context("betik utf-8 değil")?;
    eprintln!("  » {name}");
    let st = Command::new("/bin/sh")
        .arg("-c")
        .arg(s)
        .arg(name)
        .env("JPS_ROOT", &o.root)
        .env("JPS_PACKAGE", &c.package)
        .env("JPS_VERSION", &c.version)
        .env("JPS_ACTION", action)
        .current_dir(&o.root)
        .status()
        .with_context(|| format!("{name} başlatılamadı"))?;
    if !st.success() {
        bail!("{name} başarısız oldu ({st})");
    }
    Ok(())
}

fn claimed(db: &Path, skip: &str) -> Result<HashMap<String, (String, char)>> {
    let mut m = HashMap::new();
    if !db.is_dir() {
        return Ok(m);
    }
    for e in fs::read_dir(db)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().into_owned();
        if name == skip {
            continue;
        }
        let mp = e.path().join("manifest");
        if !mp.is_file() {
            continue;
        }
        for en in parse_manifest(&fs::read_to_string(&mp)?)? {
            if en.kind != 'd' || !m.contains_key(&en.path) {
                m.insert(en.path.clone(), (name.clone(), en.kind));
            }
        }
    }
    Ok(m)
}

fn rm_file(root: &Path, e: &Entry) {
    if let Some(rel) = safe_rel(&e.path) {
        let p = root.join(rel);
        if let Err(err) = fs::remove_file(&p) {
            if err.kind() != ErrorKind::NotFound {
                eprintln!("  uyarı: silinemedi {}: {err}", p.display());
            }
        }
    }
}

pub fn install(file: &Path, o: &Opts) -> Result<()> {
    let (pkg, c, entries) = load_checked(file)?;
    let data = pkg.get("data").context("data yok")?;

    if c.architecture != "all" && c.architecture != host_arch() && !o.force {
        bail!(
            "mimari uyuşmuyor: paket '{}', sistem '{}' (--force ile geç)",
            c.architecture,
            host_arch()
        );
    }

    fs::create_dir_all(&o.root)
        .with_context(|| format!("hedef kök oluşturulamadı: {}", o.root.display()))?;
    let db = db_dir(&o.root);
    fs::create_dir_all(&db).with_context(|| {
        format!("{} yazılamadı (root yetkisi gerekebilir: sudo jps ...)", db.display())
    })?;

    let missing: Vec<&String> = c
        .depends
        .iter()
        .filter(|d| !db.join(d).join("control").is_file())
        .collect();
    if !missing.is_empty() && !o.force {
        bail!(
            "eksik bağımlılıklar: {}",
            missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        );
    }

    let others = claimed(&db, &c.package)?;
    for e in entries.iter().filter(|e| e.kind != 'd') {
        if let Some((owner, k)) = others.get(&e.path) {
            if *k != 'd' && !o.force {
                bail!("çakışma: /{} dosyası '{}' paketine ait (--force ile geç)", e.path, owner);
            }
        }
    }

    let pkgdb = db.join(&c.package);
    let old = if pkgdb.join("manifest").is_file() {
        Some(parse_manifest(&fs::read_to_string(pkgdb.join("manifest"))?)?)
    } else {
        None
    };
    let action = if old.is_some() { "upgrade" } else { "install" };
    eprintln!("{} {} ({}) kuruluyor...", c.package, c.version, action);

    run_script("preinst", pkg.get("preinst"), &c, action, o)?;

    // Kayıt önce yazılır: yarım kalan kurulum bile `jps remove` ile temizlenebilir.
    fs::create_dir_all(&pkgdb)?;
    fs::write(pkgdb.join("control"), control_text(&pkg)?)?;
    fs::write(pkgdb.join("manifest"), manifest_text(&entries))?;
    for s in SCRIPTS {
        match pkg.get(s) {
            Some(d) => fs::write(pkgdb.join(s), d)?,
            None => {
                let _ = fs::remove_file(pkgdb.join(s));
            }
        }
    }

    unpack(data, &o.root)?;

    if let Some(old) = &old {
        let newset: HashSet<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        for e in old.iter().filter(|e| e.kind != 'd' && !newset.contains(e.path.as_str())) {
            rm_file(&o.root, e);
        }
    }

    run_script("postinst", pkg.get("postinst"), &c, action, o)?;
    println!("✔ {} {} kuruldu", c.package, c.version);
    Ok(())
}

pub fn remove(name: &str, o: &Opts) -> Result<()> {
    if !valid_name(name) {
        bail!("geçersiz paket adı: {name}");
    }
    let db = db_dir(&o.root);
    let pkgdb = db.join(name);
    if !pkgdb.join("control").is_file() {
        bail!("'{name}' kurulu değil");
    }
    let c = Control::parse(&fs::read_to_string(pkgdb.join("control"))?)?;

    if !o.force {
        let mut users = Vec::new();
        for e in fs::read_dir(&db)? {
            let e = e?;
            if e.file_name().to_string_lossy() == name {
                continue;
            }
            if let Ok(t) = fs::read_to_string(e.path().join("control")) {
                if let Ok(oc) = Control::parse(&t) {
                    if oc.depends.iter().any(|d| d == name) {
                        users.push(oc.package);
                    }
                }
            }
        }
        if !users.is_empty() {
            bail!("'{name}' şu paketler tarafından gerekiyor: {} (--force ile geç)", users.join(", "));
        }
    }

    eprintln!("{} {} kaldırılıyor...", c.package, c.version);
    let rd = |n: &str| fs::read(pkgdb.join(n)).ok();
    let (prerm, postrm) = (rd("prerm"), rd("postrm"));
    run_script("prerm", prerm.as_deref(), &c, "remove", o)?;

    let entries = parse_manifest(&fs::read_to_string(pkgdb.join("manifest"))?)?;
    for e in entries.iter().filter(|e| e.kind != 'd') {
        rm_file(&o.root, e);
    }
    let others = claimed(&db, name)?;
    let mut dirs: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.kind == 'd' && !others.contains_key(&e.path))
        .collect();
    dirs.sort_by(|a, b| b.path.len().cmp(&a.path.len()));
    for d in dirs {
        if let Some(rel) = safe_rel(&d.path) {
            let _ = fs::remove_dir(o.root.join(rel)); // doluysa sessizce atlanır
        }
    }

    run_script("postrm", postrm.as_deref(), &c, "remove", o)?;
    fs::remove_dir_all(&pkgdb)?;
    println!("✔ {} kaldırıldı", c.package);
    Ok(())
}

pub fn list(root: &Path) -> Result<()> {
    let db = db_dir(root);
    let mut names: Vec<String> = Vec::new();
    if db.is_dir() {
        for e in fs::read_dir(&db)? {
            let e = e?;
            if e.path().join("control").is_file() {
                names.push(e.file_name().to_string_lossy().into_owned());
            }
        }
    }
    names.sort();
    if names.is_empty() {
        println!("Kurulu paket yok.");
        return Ok(());
    }
    for n in names {
        let t = fs::read_to_string(db.join(&n).join("control"))?;
        if let Ok(c) = Control::parse(&t) {
            println!("{:<24} {:<14} {:<8} {}", c.package, c.version, c.architecture, c.description);
        }
    }
    Ok(())
}
