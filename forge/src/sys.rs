//! Sistem entegrasyonu: sistem kullanıcıları/grupları, systemd servisleri ve
//! sistemden beklenen (dpkg) paketler

use crate::format::*;
use crate::ops::{db_dir, Opts};
use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

fn real_root(o: &Opts) -> bool {
    o.root.as_path() == Path::new("/") && !o.no_scripts
}

// ───────────── sistem (dpkg) bağımlılıkları ─────────────

fn dpkg_installed(root: &Path) -> HashSet<String> {
    let t = fs::read_to_string(root.join("var/lib/dpkg/status"))
        .or_else(|_| fs::read_to_string("/var/lib/dpkg/status"))
        .unwrap_or_default();
    let mut set = HashSet::new();
    for block in t.split("\n\n") {
        let mut name: Option<&str> = None;
        let mut ok = false;
        for l in block.lines() {
            if let Some(v) = l.strip_prefix("Package: ") {
                name = Some(v.trim());
            } else if let Some(v) = l.strip_prefix("Status: ") {
                ok = v.trim().ends_with(" installed");
            }
        }
        if let (Some(n), true) = (name, ok) {
            set.insert(n.to_string());
        }
    }
    set
}

pub fn check_sysdeps(root: &Path, c: &Control, force: bool) -> Result<()> {
    if c.sysdepends.is_empty() {
        return Ok(());
    }
    let sys = dpkg_installed(root);
    let db = db_dir(root);
    let miss: Vec<&str> = c
        .sysdepends
        .iter()
        .map(|s| s.as_str())
        .filter(|d| !sys.contains(*d) && !db.join(*d).join("control").is_file())
        .collect();
    if miss.is_empty() {
        return Ok(());
    }
    let list = miss.join(" ");
    if force {
        eprintln!("uyarı: sistem paketleri eksik: {list} (--force ile devam ediliyor)");
        return Ok(());
    }
    bail!("bu paket sistemde şu paketleri bekliyor: {list}\n  kur: sudo apt install {list}   (veya --force)")
}

// ───────────── kullanıcılar ve gruplar ─────────────

pub struct UserLine {
    pub kind: char,
    pub name: String,
    pub group: String,
    pub home: String,
    pub shell: String,
}

fn ident(s: &str) -> bool {
    let b = s.as_bytes();
    !s.is_empty()
        && s.len() <= 32
        && (b[0].is_ascii_lowercase() || b[0] == b'_')
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn abs(s: &str) -> bool {
    s == "-" || (s.starts_with('/') && !s.contains(".."))
}

/// Biçim:  g <grup>   |   u <kullanıcı> <grup|-> <home|-> <shell|->
pub fn parse_sysusers(t: &str) -> Result<Vec<UserLine>> {
    let mut v = Vec::new();
    for (i, line) in t.lines().enumerate() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = l.split_whitespace().collect();
        let ok = match f[0] {
            "g" => f.len() == 2 && ident(f[1]),
            "u" => f.len() == 5 && ident(f[1]) && (f[2] == "-" || ident(f[2])) && abs(f[3]) && abs(f[4]),
            _ => false,
        };
        if !ok {
            bail!("sysusers satır {}: bozuk: {l}", i + 1);
        }
        if f[0] == "g" {
            v.push(UserLine { kind: 'g', name: f[1].into(), group: "-".into(), home: "-".into(), shell: "-".into() });
        } else {
            v.push(UserLine { kind: 'u', name: f[1].into(), group: f[2].into(), home: f[3].into(), shell: f[4].into() });
        }
    }
    Ok(v)
}

fn getent(db: &str, name: &str) -> bool {
    Command::new("getent")
        .arg(db)
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn run_ok(cmd: &str, args: &[&str]) -> Result<()> {
    let st = Command::new(cmd)
        .args(args)
        .status()
        .with_context(|| format!("{cmd} başlatılamadı (shadow-utils kurulu mu?)"))?;
    if !st.success() {
        bail!("{cmd} başarısız oldu: {}", args.join(" "));
    }
    Ok(())
}

pub fn apply_users(pkg: &Package, o: &Opts) -> Result<()> {
    let t = match pkg.get("sysusers") {
        Some(b) => String::from_utf8_lossy(b).into_owned(),
        None => return Ok(()),
    };
    let lines = parse_sysusers(&t)?;
    if lines.is_empty() {
        return Ok(());
    }
    if !real_root(o) {
        eprintln!("  » sysusers atlandı (sandbox/--no-scripts)");
        return Ok(());
    }
    for u in &lines {
        if u.kind == 'g' {
            if !getent("group", &u.name) {
                run_ok("groupadd", &["--system", u.name.as_str()])?;
                eprintln!("  » grup oluşturuldu: {}", u.name);
            }
            continue;
        }
        let grp = if u.group == "-" { u.name.as_str() } else { u.group.as_str() };
        if !getent("group", grp) {
            run_ok("groupadd", &["--system", grp])?;
        }
        if !getent("passwd", &u.name) {
            let home = if u.home == "-" { "/nonexistent" } else { u.home.as_str() };
            let shell = if u.shell == "-" { "/usr/sbin/nologin" } else { u.shell.as_str() };
            run_ok(
                "useradd",
                &["--system", "--gid", grp, "--home-dir", home, "--shell", shell, "--no-create-home", u.name.as_str()],
            )?;
            eprintln!("  » kullanıcı oluşturuldu: {}", u.name);
        }
    }
    Ok(())
}

// ───────────── servisler ─────────────

pub struct Svc {
    pub unit: String,
    pub enable: bool,
    pub start: bool,
}

fn unit_ok(u: &str) -> bool {
    u.len() <= 128
        && [".service", ".socket", ".timer", ".path"].iter().any(|s| u.ends_with(*s))
        && u.chars().all(|c| c.is_ascii_alphanumeric() || "-_.@:".contains(c))
}

/// Biçim:  <birim.service> [enable] [start]
pub fn parse_services(t: &str) -> Result<Vec<Svc>> {
    let mut v = Vec::new();
    for (i, line) in t.lines().enumerate() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let mut it = l.split_whitespace();
        let unit = it.next().unwrap_or("");
        if !unit_ok(unit) {
            bail!("services satır {}: geçersiz birim adı: {unit}", i + 1);
        }
        let (mut enable, mut start) = (false, false);
        for f in it {
            match f {
                "enable" => enable = true,
                "start" => start = true,
                _ => bail!("services satır {}: bilinmeyen bayrak: {f}", i + 1),
            }
        }
        v.push(Svc { unit: unit.to_string(), enable, start });
    }
    Ok(v)
}

fn systemd_up() -> bool {
    Path::new("/run/systemd/system").is_dir()
}

fn sysctl(args: &[&str]) -> bool {
    Command::new("systemctl")
        .args(args)
        .stdout(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn apply_services(pkg: &Package, o: &Opts) -> Result<()> {
    let t = match pkg.get("services") {
        Some(b) => String::from_utf8_lossy(b).into_owned(),
        None => return Ok(()),
    };
    let svcs = parse_services(&t)?;
    if svcs.is_empty() {
        return Ok(());
    }
    if !real_root(o) {
        eprintln!("  » servisler atlandı (sandbox/--no-scripts)");
        return Ok(());
    }
    if !systemd_up() {
        eprintln!("  uyarı: systemd çalışmıyor; servisler etkinleştirilmedi");
        return Ok(());
    }
    sysctl(&["daemon-reload"]);
    for s in &svcs {
        if s.enable {
            eprintln!("  » etkinleştir: {}", s.unit);
            if !sysctl(&["enable", s.unit.as_str()]) {
                eprintln!("  uyarı: enable başarısız: {}", s.unit);
            }
        }
        if s.start {
            eprintln!("  » başlat: {}", s.unit);
            if !sysctl(&["start", s.unit.as_str()]) {
                eprintln!("  uyarı: start başarısız: {}", s.unit);
            }
        }
    }
    Ok(())
}

pub fn stop_services(pkgdb: &Path, o: &Opts) {
    if !real_root(o) || !systemd_up() {
        return;
    }
    let t = match fs::read_to_string(pkgdb.join("services")) {
        Ok(t) => t,
        Err(_) => return,
    };
    if let Ok(svcs) = parse_services(&t) {
        for s in svcs.iter().filter(|s| s.enable || s.start) {
            eprintln!("  » durdur: {}", s.unit);
            if s.enable {
                sysctl(&["disable", "--now", s.unit.as_str()]);
            } else {
                sysctl(&["stop", s.unit.as_str()]);
            }
        }
    }
}

pub fn after_remove(pkgdb: &Path, o: &Opts) {
    if real_root(o) && pkgdb.join("services").is_file() && systemd_up() {
        sysctl(&["daemon-reload"]);
    }
    if let Ok(t) = fs::read_to_string(pkgdb.join("sysusers")) {
        if let Ok(v) = parse_sysusers(&t) {
            let names: Vec<&str> = v.iter().filter(|u| u.kind == 'u').map(|u| u.name.as_str()).collect();
            if !names.is_empty() {
                eprintln!(
                    "  not: sistem kullanıcıları silinmedi: {} (gerekirse: sudo userdel <ad>)",
                    names.join(", ")
                );
            }
        }
    }
}

/// Paketin sysusers/services bölümlerini kayıt dizinine yazar (remove için gerekir).
pub fn store(pkg: &Package, pkgdb: &Path) -> Result<()> {
    for s in ["sysusers", "services"] {
        match pkg.get(s) {
            Some(d) => fs::write(pkgdb.join(s), d)?,
            None => {
                let _ = fs::remove_file(pkgdb.join(s));
            }
        }
    }
    Ok(())
}
