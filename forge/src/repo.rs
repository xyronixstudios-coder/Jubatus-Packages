//! Depo desteği: update / search / show / install (ad ile) / upgrade

use crate::extra;
use crate::format::*;
use crate::ops::{self, Opts};
use anyhow::{bail, Context, Result};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const DEFAULT_REPO: &str =
    "https://raw.githubusercontent.com/xyronixstudios-coder/Jubatus-Packages/repo";

pub fn repo_url(root: &Path) -> String {
    if let Ok(u) = std::env::var("JPS_REPO") {
        if !u.trim().is_empty() {
            return u.trim().trim_end_matches('/').to_string();
        }
    }
    if let Ok(t) = fs::read_to_string(root.join("etc/jps/repo.conf")) {
        if let Some(l) = t
            .lines()
            .map(|l| l.trim())
            .find(|l| !l.is_empty() && !l.starts_with('#'))
        {
            return l.trim_end_matches('/').to_string();
        }
    }
    DEFAULT_REPO.to_string()
}

fn state_dir(root: &Path) -> PathBuf {
    root.join("var/lib/jps-repo")
}
fn cache_dir(root: &Path) -> PathBuf {
    root.join("var/cache/jps")
}
fn db_dir(root: &Path) -> PathBuf {
    root.join("var/lib/jps")
}

fn have(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

pub fn fetch(url: &str, dest: &Path) -> Result<()> {
    let tmp = PathBuf::from(format!("{}.part", dest.display()));
    let st = if have("curl") {
        Command::new("curl")
            .args(["-fsSL", "--retry", "3", "--connect-timeout", "20", "-o"])
            .arg(&tmp)
            .arg(url)
            .status()
    } else if have("wget") {
        Command::new("wget").args(["-q", "-O"]).arg(&tmp).arg(url).status()
    } else {
        bail!("curl veya wget gerekli (sudo apt install curl)");
    };
    let st = st.context("indirme komutu başlatılamadı")?;
    if !st.success() {
        let _ = fs::remove_file(&tmp);
        bail!("indirilemedi: {url}");
    }
    fs::rename(&tmp, dest)?;
    Ok(())
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct RepoEntry {
    pub name: String,
    pub version: String,
    pub arch: String,
    pub size: u64,
    pub sha: String,
    pub file: String,
    pub depends: Vec<String>,
    pub desc: String,
}

fn load_index(root: &Path) -> Result<Vec<RepoEntry>> {
    let p = state_dir(root).join("index");
    let t = fs::read_to_string(&p).with_context(|| {
        format!("paket dizini yok: önce 'sudo jps update' çalıştır ({})", p.display())
    })?;
    let host = ops::host_arch();
    let mut v = Vec::new();
    for line in t.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.splitn(8, '\t').collect();
        if f.len() < 8 {
            continue;
        }
        if f[2] != "all" && f[2] != host {
            continue;
        }
        if !valid_name(f[0]) || safe_rel(f[5]).is_none() {
            continue;
        }
        v.push(RepoEntry {
            name: f[0].into(),
            version: f[1].into(),
            arch: f[2].into(),
            size: f[3].parse().unwrap_or(0),
            sha: f[4].into(),
            file: f[5].into(),
            depends: f[6].split(',').filter(|s| !s.is_empty()).map(String::from).collect(),
            desc: f[7].into(),
        });
    }
    Ok(v)
}

fn best<'a>(idx: &'a [RepoEntry], name: &str) -> Option<&'a RepoEntry> {
    idx.iter()
        .filter(|e| e.name == name)
        .max_by(|a, b| vercmp(&a.version, &b.version))
}

pub fn installed_version(root: &Path, name: &str) -> Option<String> {
    if !valid_name(name) {
        return None;
    }
    let t = fs::read_to_string(db_dir(root).join(name).join("control")).ok()?;
    Control::parse(&t).ok().map(|c| c.version)
}

fn installed_list(root: &Path) -> Vec<(String, String)> {
    let mut v = Vec::new();
    if let Ok(rd) = fs::read_dir(db_dir(root)) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if let Some(ver) = installed_version(root, &n) {
                v.push((n, ver));
            }
        }
    }
    v.sort();
    v
}

// ───────────── sürüm karşılaştırma ─────────────

fn tokens(s: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut num = false;
    for c in s.chars() {
        if c.is_ascii_digit() {
            if !cur.is_empty() && !num {
                v.push(std::mem::take(&mut cur));
            }
            num = true;
            cur.push(c);
        } else if c.is_ascii_alphabetic() {
            if !cur.is_empty() && num {
                v.push(std::mem::take(&mut cur));
            }
            num = false;
            cur.push(c);
        } else if !cur.is_empty() {
            v.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        v.push(cur);
    }
    v
}

fn is_num(s: &str) -> bool {
    s.as_bytes().first().map_or(false, |b| b.is_ascii_digit())
}

pub fn vercmp(a: &str, b: &str) -> Ordering {
    let (ta, tb) = (tokens(a), tokens(b));
    for i in 0..ta.len().max(tb.len()) {
        match (ta.get(i), tb.get(i)) {
            (Some(x), Some(y)) => {
                let o = match (is_num(x), is_num(y)) {
                    (true, true) => {
                        let (x, y) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                        x.len().cmp(&y.len()).then(x.cmp(y))
                    }
                    (false, false) => x.cmp(y),
                    (true, false) => Ordering::Greater,
                    (false, true) => Ordering::Less,
                };
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(x), None) => return if is_num(x) { Ordering::Greater } else { Ordering::Less },
            (None, Some(y)) => return if is_num(y) { Ordering::Less } else { Ordering::Greater },
            (None, None) => {}
        }
    }
    Ordering::Equal
}

// ───────────── komutlar ─────────────

pub fn update(o: &Opts) -> Result<()> {
    let url = repo_url(&o.root);
    let dir = state_dir(&o.root);
    fs::create_dir_all(&dir)
        .with_context(|| format!("{} yazılamadı (sudo gerekir)", dir.display()))?;
    eprintln!("Depo: {url}");
    fetch(&format!("{url}/index"), &dir.join("index"))?;
    if fetch(&format!("{url}/signing.pub"), &dir.join("signing.pub")).is_err() {
        eprintln!("uyarı: signing.pub indirilemedi");
    }
    let n = load_index(&o.root)?.len();
    println!("✔ dizin güncellendi: {n} paket ({})", ops::host_arch());
    Ok(())
}

pub fn trust_repo(o: &Opts) -> Result<()> {
    let p = state_dir(&o.root).join("signing.pub");
    if !p.is_file() {
        bail!("önce: sudo jps update");
    }
    extra::trust(&p, true)
}

pub fn clean(o: &Opts) -> Result<()> {
    let d = cache_dir(&o.root);
    if d.is_dir() {
        fs::remove_dir_all(&d)?;
    }
    println!("✔ indirme önbelleği temizlendi");
    Ok(())
}

pub fn search(root: &Path, term: &str) -> Result<()> {
    let idx = load_index(root)?;
    let t = term.to_lowercase();
    let mut v: Vec<&RepoEntry> = idx
        .iter()
        .filter(|e| e.name.contains(&t) || e.desc.to_lowercase().contains(&t))
        .collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    if v.is_empty() {
        println!("Eşleşen paket yok.");
    }
    for e in v {
        let k = if installed_version(root, &e.name).is_some() { " [kurulu]" } else { "" };
        println!("{:<26} {:<10} {}{}", e.name, e.version, e.desc, k);
    }
    Ok(())
}

pub fn show(root: &Path, name: &str) -> Result<()> {
    let idx = load_index(root)?;
    let e = best(&idx, name).with_context(|| format!("depoda yok: {name}"))?;
    println!("Paket       : {}", e.name);
    println!("Sürüm       : {}", e.version);
    println!("Mimari      : {}", e.arch);
    println!("Boyut       : {} bayt", e.size);
    println!(
        "Bağımlılık  : {}",
        if e.depends.is_empty() { "-".to_string() } else { e.depends.join(", ") }
    );
    println!("Dosya       : {}", e.file);
    println!("SHA-256     : {}", e.sha);
    println!("Açıklama    : {}", e.desc);
    match installed_version(root, name) {
        Some(v) => println!("Kurulu      : {v}"),
        None => println!("Kurulu      : hayır"),
    }
    Ok(())
}

// ───────────── plan / kurulum ─────────────

enum Item {
    Repo(RepoEntry),
    File(PathBuf, Control),
}

struct Planner<'a> {
    root: &'a Path,
    idx: &'a [RepoEntry],
    planned: Vec<Item>,
    seen: HashSet<String>,
    quiet: bool,
}

impl<'a> Planner<'a> {
    fn add_name(&mut self, name: &str, explicit: bool) -> Result<()> {
        if self.seen.contains(name) {
            return Ok(());
        }
        let idx = self.idx;
        let inst = installed_version(self.root, name);
        let e = best(idx, name);
        if let Some(iv) = &inst {
            let newer = explicit && e.map_or(false, |x| vercmp(iv, &x.version) == Ordering::Less);
            if !newer {
                if explicit && !self.quiet {
                    println!("{name} zaten en yeni sürümde ({iv})");
                }
                self.seen.insert(name.to_string());
                return Ok(());
            }
        }
        let e = match e {
            Some(e) => e.clone(),
            None => bail!("paket bulunamadı: {name} (depo dizini güncel mi? sudo jps update)"),
        };
        self.seen.insert(name.to_string()); // döngü koruması
        for d in &e.depends {
            self.add_name(d, false)?;
        }
        self.planned.push(Item::Repo(e));
        Ok(())
    }

    fn add_file(&mut self, path: &Path) -> Result<()> {
        let pkg = read_package(path)?;
        let c = Control::parse(std::str::from_utf8(pkg.get("control").context("control yok")?)?)?;
        if !self.seen.insert(c.package.clone()) {
            return Ok(());
        }
        let deps = c.depends.clone();
        for d in &deps {
            self.add_name(d, false)?;
        }
        self.planned.push(Item::File(path.to_path_buf(), c));
        Ok(())
    }
}

fn is_file_target(t: &str) -> bool {
    t.ends_with(".jps") || t.contains('/') || Path::new(t).is_file()
}

pub fn install(targets: &[String], o: &Opts, yes: bool, trusted_files: bool) -> Result<()> {
    let has_names = targets.iter().any(|t| !is_file_target(t));
    let idx = if has_names {
        load_index(&o.root)?
    } else {
        load_index(&o.root).unwrap_or_default()
    };
    let mut pl = Planner {
        root: &o.root,
        idx: &idx,
        planned: Vec::new(),
        seen: HashSet::new(),
        quiet: false,
    };
    for t in targets {
        if is_file_target(t) {
            pl.add_file(Path::new(t))?;
        } else {
            pl.add_name(t, true)?;
        }
    }
    let plan = pl.planned;
    if plan.is_empty() {
        return Ok(());
    }
    execute(plan, o, yes, trusted_files)
}

pub fn upgrade(o: &Opts, yes: bool) -> Result<()> {
    let idx = load_index(&o.root)?;
    let mut pl = Planner {
        root: &o.root,
        idx: &idx,
        planned: Vec::new(),
        seen: HashSet::new(),
        quiet: true,
    };
    for (name, _) in installed_list(&o.root) {
        if best(&idx, &name).is_some() {
            pl.add_name(&name, true)?;
        }
    }
    let plan = pl.planned;
    if plan.is_empty() {
        println!("Her şey güncel.");
        return Ok(());
    }
    execute(plan, o, yes, false)
}

fn execute(plan: Vec<Item>, o: &Opts, yes: bool, trusted_files: bool) -> Result<()> {
    println!("Kurulacak / yükseltilecek paketler:");
    let mut total = 0u64;
    for it in &plan {
        match it {
            Item::Repo(e) => {
                total += e.size;
                println!("  {:<28} {:<12} {}", e.name, e.version, e.desc);
            }
            Item::File(p, c) => {
                println!("  {:<28} {:<12} (dosya: {})", c.package, c.version, p.display())
            }
        }
    }
    if total > 0 {
        println!("İndirilecek: {} KB", total / 1024 + 1);
    }
    if !yes {
        print!("Devam edilsin mi? [E/h] ");
        std::io::stdout().flush()?;
        let mut a = String::new();
        std::io::stdin().read_line(&mut a)?;
        let a = a.trim().to_lowercase();
        if !(a.is_empty() || a == "e" || a == "evet" || a == "y") {
            println!("İptal edildi.");
            return Ok(());
        }
    }
    let cache = cache_dir(&o.root);
    fs::create_dir_all(&cache)
        .with_context(|| format!("{} yazılamadı (sudo gerekir)", cache.display()))?;
    let url = repo_url(&o.root);

    for it in plan {
        match it {
            Item::Repo(e) => {
                let fname = Path::new(&e.file).file_name().context("geçersiz dosya adı")?.to_owned();
                let dest = cache.join(fname);
                let cached_ok = dest.is_file()
                    && fs::read(&dest).map(|d| sha_hex(&d) == e.sha).unwrap_or(false);
                if !cached_ok {
                    eprintln!("↓ {} {}", e.name, e.version);
                    fetch(&format!("{url}/{}", e.file), &dest)?;
                }
                let data = fs::read(&dest)?;
                if sha_hex(&data) != e.sha {
                    let _ = fs::remove_file(&dest);
                    bail!("{}: indirilen dosya dizindeki sağlama toplamıyla uyuşmuyor", e.name);
                }
                let pkg = read_package(&dest)?;
                let c = Control::parse(std::str::from_utf8(pkg.get("control").context("control yok")?)?)?;
                if c.package != e.name || c.version != e.version {
                    bail!("{}: paket içeriği dizin kaydıyla uyuşmuyor", e.name);
                }
                extra::check_install(&dest, true)?;
                ops::install(&dest, o)?;
            }
            Item::File(p, _) => {
                extra::check_install(&p, trusted_files)?;
                ops::install(&p, o)?;
            }
        }
    }
    Ok(())
}
