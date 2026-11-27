//! Depo desteği: çoklu depo, imzalı dizin, update / search / show / install / upgrade, hold

use crate::extra;
use crate::format::*;
use crate::ops::{self, Opts};
use crate::sigfile;
use anyhow::{bail, Context, Result};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const DEFAULT_REPO: &str =
    "https://raw.githubusercontent.com/xyronixstudios-coder/Jubatus-Packages/repo";

#[derive(Clone, Debug)]
pub struct Repo {
    pub name: String,
    pub url: String,
    pub id: String,
}

fn mk_repo(name: &str, url: &str) -> Repo {
    let url = url.trim().trim_end_matches('/').to_string();
    let id = sha_hex(url.as_bytes())[..12].to_string();
    Repo { name: name.to_string(), url, id }
}

fn env_repo() -> Option<String> {
    std::env::var("JPS_REPO")
        .ok()
        .map(|u| u.trim().trim_end_matches('/').to_string())
        .filter(|u| !u.is_empty())
}

fn first_url(p: &Path) -> Option<String> {
    let t = fs::read_to_string(p).ok()?;
    t.lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .and_then(|l| l.split_whitespace().next())
        .map(|u| u.to_string())
}

/// Varsayılan depo (self-update ve install.sh bunu kullanır)
pub fn repo_url(root: &Path) -> String {
    if let Some(u) = env_repo() {
        return u;
    }
    first_url(&root.join("etc/jps/repo.conf"))
        .map(|u| u.trim_end_matches('/').to_string())
        .unwrap_or_else(|| DEFAULT_REPO.to_string())
}

/// Öncelik sırası: varsayılan depo, sonra /etc/jps/repo.d/*.list (ada göre). JPS_REPO tanımlıysa yalnızca o.
pub fn repos(root: &Path) -> Vec<Repo> {
    let mut v = vec![mk_repo("default", &repo_url(root))];
    if env_repo().is_some() {
        return v;
    }
    let dir = root.join("etc/jps/repo.d");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().map_or(false, |x| x == "list"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    for p in files {
        let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if let Some(u) = first_url(&p) {
            let r = mk_repo(&name, &u);
            if !v.iter().any(|x| x.id == r.id) {
                v.push(r);
            }
        }
    }
    v
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

#[cfg(feature = "net")]
pub fn fetch(url: &str, dest: &Path) -> Result<()> {
    let tmp = PathBuf::from(format!("{}.part", dest.display()));
    if let Err(e) = fetch_to(url, &tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    fs::rename(&tmp, dest)?;
    Ok(())
}

#[cfg(feature = "net")]
fn fetch_to(url: &str, tmp: &Path) -> Result<()> {
    if let Some(p) = url.strip_prefix("file://") {
        fs::copy(p, tmp).with_context(|| format!("okunamadı: {p}"))?;
        return Ok(());
    }
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        bail!("desteklenmeyen adres: {url}");
    }
    let mut ab = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(20))
        .timeout_read(std::time::Duration::from_secs(60))
        .redirects(8)
        .user_agent(concat!("jps/", env!("CARGO_PKG_VERSION")));
    for k in ["JPS_PROXY", "https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"] {
        if let Ok(p) = std::env::var(k) {
            if !p.is_empty() {
                if let Ok(px) = ureq::Proxy::new(&p) {
                    ab = ab.proxy(px);
                }
                break;
            }
        }
    }
    let agent = ab.build();
    let mut last = String::new();
    for attempt in 0..3u64 {
        match agent.get(url).call() {
            Ok(resp) => {
                let mut rd = resp.into_reader();
                let mut f = fs::File::create(tmp)?;
                std::io::copy(&mut rd, &mut f)?;
                return Ok(());
            }
            Err(ureq::Error::Status(code, _)) if (400..500).contains(&code) => {
                bail!("indirilemedi: {url} (HTTP {code})")
            }
            Err(e) => {
                last = e.to_string();
                std::thread::sleep(std::time::Duration::from_secs(1 + attempt));
            }
        }
    }
    bail!("indirilemedi: {url}: {last}")
}

#[cfg(not(feature = "net"))]
pub fn fetch(url: &str, dest: &Path) -> Result<()> {
    let tmp = PathBuf::from(format!("{}.part", dest.display()));
    let has = |c: &str| {
        std::process::Command::new(c)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
    };
    let st = if has("curl") {
        std::process::Command::new("curl")
            .args(["-fsSL", "--retry", "3", "--connect-timeout", "20", "-o"])
            .arg(&tmp)
            .arg(url)
            .status()
    } else if has("wget") {
        std::process::Command::new("wget").args(["-q", "-O"]).arg(&tmp).arg(url).status()
    } else {
        bail!("curl veya wget gerekli (bu derleme yerleşik indiriciyi içermiyor)");
    };
    let st = st.context("indirme komutu başlatılamadı")?;
    if !st.success() {
        let _ = fs::remove_file(&tmp);
        bail!("indirilemedi: {url}");
    }
    fs::rename(&tmp, dest)?;
    Ok(())
}

fn fp(k: &[u8; 32]) -> String {
    sha_hex(k)[..16].to_string()
}

fn confirm(q: &str, default_yes: bool) -> bool {
    print!("{q}");
    let _ = std::io::stdout().flush();
    let mut a = String::new();
    let _ = std::io::stdin().read_line(&mut a);
    let a = a.trim().to_lowercase();
    if a.is_empty() {
        default_yes
    } else {
        a == "e" || a == "evet" || a == "y" || a == "yes"
    }
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
    pub top: bool,
    pub url: String,
}

fn parse_index(t: &str, url: &str) -> Vec<RepoEntry> {
    let host = ops::host_arch();
    let mut v = Vec::new();
    for line in t.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.splitn(9, '\t').collect();
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
            top: f.get(8).map_or(true, |s| *s == "top"),
            url: url.to_string(),
        });
    }
    v
}

/// Tüm depoların birleşik dizini. Aynı adlı pakette önceliği yüksek (önce gelen) depo kazanır.
fn load_index(root: &Path) -> Result<Vec<RepoEntry>> {
    let mut out: Vec<RepoEntry> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut any = false;
    for r in repos(root) {
        let p = state_dir(root).join(&r.id).join("index");
        let t = match fs::read_to_string(&p) {
            Ok(t) => t,
            Err(_) => continue,
        };
        any = true;
        let ents = parse_index(&t, &r.url);
        let names: HashSet<String> = ents.iter().map(|e| e.name.clone()).collect();
        for e in ents {
            if !seen.contains(&e.name) {
                out.push(e);
            }
        }
        seen.extend(names);
    }
    if !any {
        bail!("paket dizini yok: önce 'sudo jps update' çalıştır");
    }
    Ok(out)
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

// ───────────── dizin güncelleme (imzalı) ─────────────

fn check_index(data: &[u8], sig: Option<&[u8]>, repo_pub: Option<[u8; 32]>) -> Result<String> {
    let keys = sigfile::trusted_keys();
    let rp_trusted = repo_pub.map_or(false, |k| keys.contains(&k));
    match sig {
        Some(s) if s.len() == 64 => {
            if let Some(k) = sigfile::verify_bytes(data, s, &keys) {
                return Ok(format!("✔ dizin imzası geçerli ({})", fp(&k)));
            }
            if let Some(rp) = repo_pub {
                if sigfile::verify_bytes(data, s, &[rp]).is_some() {
                    return Ok(format!(
                        "uyarı: dizin imzası geçerli ama anahtar ({}) güvenilir listede değil",
                        fp(&rp)
                    ));
                }
            }
            bail!("dizin imzası GEÇERSİZ (dizin değiştirilmiş olabilir)")
        }
        Some(_) => bail!("dizin imzası bozuk"),
        None => {
            if rp_trusted {
                bail!("dizin imzası yok ama bu deponun anahtarı güvenilir: imzasız dizin kabul edilmez (depo sahibi: jps sign-file index)");
            }
            Ok("uyarı: dizin imzasız (anahtar güvenilir değil)".to_string())
        }
    }
}

fn update_one(root: &Path, r: &Repo) -> Result<usize> {
    let dir = state_dir(root).join(&r.id);
    fs::create_dir_all(&dir)
        .with_context(|| format!("{} yazılamadı (sudo gerekir)", dir.display()))?;
    let tmp_idx = dir.join("index.new");
    let tmp_sig = dir.join("index.sig.new");
    let tmp_pub = dir.join("signing.pub.new");
    let _ = fs::remove_file(&tmp_sig);
    let _ = fs::remove_file(&tmp_pub);
    let cleanup = || {
        let _ = fs::remove_file(&tmp_idx);
        let _ = fs::remove_file(&tmp_sig);
        let _ = fs::remove_file(&tmp_pub);
    };
    if let Err(e) = fetch(&format!("{}/index", r.url), &tmp_idx) {
        cleanup();
        return Err(e);
    }
    let have_sig = fetch(&format!("{}/index.sig", r.url), &tmp_sig).is_ok();
    let have_pub = fetch(&format!("{}/signing.pub", r.url), &tmp_pub).is_ok();
    let data = fs::read(&tmp_idx)?;
    let sig = if have_sig { fs::read(&tmp_sig).ok() } else { None };
    let rp = if have_pub {
        fs::read_to_string(&tmp_pub).ok().and_then(|t| sigfile::parse_pub(&t))
    } else {
        None
    };
    let msg = match check_index(&data, sig.as_deref(), rp) {
        Ok(m) => m,
        Err(e) => {
            cleanup();
            return Err(e);
        }
    };
    eprintln!("  {msg}");
    fs::rename(&tmp_idx, dir.join("index"))?;
    if have_sig {
        fs::rename(&tmp_sig, dir.join("index.sig"))?;
    } else {
        let _ = fs::remove_file(dir.join("index.sig"));
    }
    if have_pub {
        fs::rename(&tmp_pub, dir.join("signing.pub"))?;
    }
    let t = String::from_utf8_lossy(&data).into_owned();
    Ok(parse_index(&t, &r.url).len())
}

pub fn update(o: &Opts) -> Result<()> {
    fs::create_dir_all(state_dir(&o.root))
        .with_context(|| format!("{} yazılamadı (sudo gerekir)", state_dir(&o.root).display()))?;
    let mut fail = 0usize;
    for r in repos(&o.root) {
        eprintln!("Depo [{}]: {}", r.name, r.url);
        match update_one(&o.root, &r) {
            Ok(n) => println!("✔ {}: {n} paket ({})", r.name, ops::host_arch()),
            Err(e) => {
                fail += 1;
                eprintln!("✘ {}: {e:#}", r.name);
            }
        }
    }
    if fail > 0 {
        bail!("{fail} depo güncellenemedi (başarılı olanlar kaydedildi)");
    }
    Ok(())
}

pub fn trust_repo(o: &Opts) -> Result<()> {
    let r = mk_repo("default", &repo_url(&o.root));
    let p = state_dir(&o.root).join(&r.id).join("signing.pub");
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

// ───────────── depo yönetimi ─────────────

pub fn repo_add(root: &Path, name: &str, url: &str, yes: bool) -> Result<()> {
    if !valid_name(name) || name == "default" || name == "env" {
        bail!("geçersiz depo adı: {name} (küçük harf, rakam, - . +)");
    }
    let url = url.trim().trim_end_matches('/');
    let scheme_ok = url.starts_with("https://") || url.starts_with("http://") || url.starts_with("file://");
    if !scheme_ok || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        bail!("geçersiz adres (https://... olmalı): {url}");
    }
    let list = root.join("etc/jps/repo.d").join(format!("{name}.list"));
    if list.exists() {
        bail!("'{name}' zaten tanımlı (önce: jps repo remove {name})");
    }
    fs::create_dir_all(state_dir(root))
        .with_context(|| format!("{} yazılamadı (sudo gerekir)", state_dir(root).display()))?;
    let tmp = state_dir(root).join(".add.pub");
    fetch(&format!("{url}/signing.pub"), &tmp).context("depo anahtarı (signing.pub) indirilemedi")?;
    let text = fs::read_to_string(&tmp).unwrap_or_default();
    let key = match sigfile::parse_pub(&text) {
        Some(k) => k,
        None => {
            let _ = fs::remove_file(&tmp);
            bail!("signing.pub geçerli bir Ed25519 anahtarı değil");
        }
    };
    println!("Depo     : {name}");
    println!("Adres    : {url}");
    println!("Anahtar  : {}", fp(&key));
    if url.starts_with("http://") {
        println!("UYARI    : şifresiz bağlantı (http)");
    }
    println!("Bu anahtarı güvenilir yapmak, depo sahibine sistemine kök yetkisiyle çalışan paket kurma izni vermektir.");
    println!("Parmak izini depo sahibinden başka bir kanaldan doğrula.");
    if !yes && !confirm("Eklensin mi? [e/H] ", false) {
        let _ = fs::remove_file(&tmp);
        println!("İptal edildi.");
        return Ok(());
    }
    extra::trust(&tmp, true)?;
    let _ = fs::remove_file(&tmp);
    let dir = root.join("etc/jps/repo.d");
    fs::create_dir_all(&dir).with_context(|| format!("{} yazılamadı (sudo gerekir)", dir.display()))?;
    fs::write(&list, format!("{url}\n"))?;
    println!("✔ depo eklendi. Şimdi: sudo jps update");
    Ok(())
}

pub fn repo_list(root: &Path) -> Result<()> {
    let keys = sigfile::trusted_keys();
    for (i, r) in repos(root).iter().enumerate() {
        let st = state_dir(root).join(&r.id);
        let n = fs::read_to_string(st.join("index")).ok().map(|t| parse_index(&t, &r.url).len());
        let key = fs::read_to_string(st.join("signing.pub")).ok().and_then(|t| sigfile::parse_pub(&t));
        let ks = match key {
            Some(k) => format!("{} ({})", fp(&k), if keys.contains(&k) { "güvenilir" } else { "GÜVENİLİR DEĞİL" }),
            None => "bilinmiyor".to_string(),
        };
        let ns = n.map_or("henüz yok (sudo jps update)".to_string(), |n| n.to_string());
        println!("{}. {:<12} {}", i + 1, r.name, r.url);
        println!("   anahtar: {ks}   paket: {ns}");
    }
    println!("\nÖncelik yukarıdan aşağıya: aynı adlı pakette ilk depo kazanır.");
    Ok(())
}

pub fn repo_remove(root: &Path, name: &str) -> Result<()> {
    if name == "default" {
        bail!("varsayılan depo kaldırılamaz");
    }
    if !valid_name(name) {
        bail!("geçersiz depo adı: {name}");
    }
    let list = root.join("etc/jps/repo.d").join(format!("{name}.list"));
    if !list.is_file() {
        bail!("böyle bir depo yok: {name}");
    }
    let r = first_url(&list).map(|u| mk_repo(name, &u));
    let key = r
        .as_ref()
        .and_then(|r| fs::read_to_string(state_dir(root).join(&r.id).join("signing.pub")).ok())
        .and_then(|t| sigfile::parse_pub(&t));
    fs::remove_file(&list)?;
    if let Some(r) = r {
        let _ = fs::remove_dir_all(state_dir(root).join(&r.id));
    }
    println!("✔ depo kaldırıldı: {name}");
    if let Some(k) = key {
        println!("Anahtarı hâlâ güvenilir listede: {}", fp(&k));
        println!("  artık gerekmiyorsa: sudo jps untrust {}", fp(&k));
    }
    Ok(())
}

// ───────────── arama ─────────────

pub fn search(root: &Path, term: &str, all: bool) -> Result<()> {
    let idx = load_index(root)?;
    let t = term.to_lowercase();
    let mut v: Vec<&RepoEntry> = idx
        .iter()
        .filter(|e| (all || e.top) && (e.name.contains(&t) || e.desc.to_lowercase().contains(&t)))
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
    println!("Depo        : {}", e.url);
    println!("Dosya       : {}", e.file);
    println!("SHA-256     : {}", e.sha);
    println!("Açıklama    : {}", e.desc);
    match installed_version(root, name) {
        Some(v) => println!("Kurulu      : {v}"),
        None => println!("Kurulu      : hayır"),
    }
    if is_held(root, name) {
        println!("Tutuluyor   : evet (jps unhold {name})");
    }
    Ok(())
}

// ───────────── plan / kurulum ─────────────

enum Item {
    Repo(RepoEntry, bool),
    File(PathBuf, Control),
}

struct Planner<'a> {
    root: &'a Path,
    idx: &'a [RepoEntry],
    planned: Vec<Item>,
    seen: HashSet<String>,
    quiet: bool,
    keep_auto: bool,
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
            let wants = explicit && e.map_or(false, |x| vercmp(iv, &x.version) == Ordering::Less);
            let held = wants && is_held(self.root, name);
            if held && !self.quiet {
                println!("{name} tutuluyor ({iv}); yükseltme atlandı (jps unhold {name})");
            }
            let newer = wants && !held;
            if !newer {
                if explicit && !self.keep_auto {
                    let _ = fs::remove_file(db_dir(self.root).join(name).join("auto"));
                }
                if explicit && !self.quiet && !held {
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
        self.seen.insert(name.to_string());
        for d in &e.depends {
            self.add_name(d, false)?;
        }
        let auto = !explicit || (self.keep_auto && is_auto(self.root, name));
        self.planned.push(Item::Repo(e, auto));
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
        keep_auto: false,
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
        keep_auto: true,
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
            Item::Repo(e, _) => {
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
    if !yes && !confirm("Devam edilsin mi? [E/h] ", true) {
        println!("İptal edildi.");
        return Ok(());
    }
    let cache = cache_dir(&o.root);
    fs::create_dir_all(&cache)
        .with_context(|| format!("{} yazılamadı (sudo gerekir)", cache.display()))?;

    for it in plan {
        match it {
            Item::Repo(e, auto) => {
                let fname = Path::new(&e.file).file_name().context("geçersiz dosya adı")?.to_owned();
                let dest = cache.join(fname);
                let cached_ok = dest.is_file()
                    && fs::read(&dest).map(|d| sha_hex(&d) == e.sha).unwrap_or(false);
                if !cached_ok {
                    eprintln!("↓ {} {}", e.name, e.version);
                    fetch(&format!("{}/{}", e.url, e.file), &dest)?;
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
                let had = installed_version(&o.root, &e.name).is_some();
                ops::install(&dest, o)?;
                mark_auto(&o.root, &e.name, auto, had);
            }
            Item::File(p, _) => {
                extra::check_install(&p, trusted_files)?;
                ops::install(&p, o)?;
            }
        }
    }
    Ok(())
}

fn is_auto(root: &Path, name: &str) -> bool {
    db_dir(root).join(name).join("auto").is_file()
}

fn is_held(root: &Path, name: &str) -> bool {
    valid_name(name) && db_dir(root).join(name).join("hold").is_file()
}

fn mark_auto(root: &Path, name: &str, auto: bool, had: bool) {
    let m = db_dir(root).join(name).join("auto");
    if auto {
        if !had {
            let _ = fs::write(&m, b"");
        }
    } else {
        let _ = fs::remove_file(&m);
    }
}
