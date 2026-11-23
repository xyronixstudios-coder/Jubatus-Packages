//! Kurulu paket denetimi: check, files, owns, autoremove

use crate::format::*;
use crate::ops::{self, db_dir, parse_manifest, Opts};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::Path;

fn pkg_names(root: &Path) -> Vec<String> {
    let mut v = Vec::new();
    if let Ok(rd) = fs::read_dir(db_dir(root)) {
        for e in rd.flatten() {
            if e.path().join("manifest").is_file() {
                v.push(e.file_name().to_string_lossy().into_owned());
            }
        }
    }
    v.sort();
    v
}

pub fn files(root: &Path, name: &str) -> Result<()> {
    if !valid_name(name) {
        bail!("geçersiz paket adı: {name}");
    }
    let t = fs::read_to_string(db_dir(root).join(name).join("manifest"))
        .with_context(|| format!("'{name}' kurulu değil"))?;
    for e in parse_manifest(&t)? {
        if e.kind != 'd' {
            println!("/{}", e.path);
        }
    }
    Ok(())
}

pub fn owns(root: &Path, path: &str) -> Result<()> {
    let rel = safe_rel(path.trim_start_matches('/'))
        .context("geçersiz yol")?
        .to_string_lossy()
        .into_owned();
    let mut found = false;
    for n in pkg_names(root) {
        let t = fs::read_to_string(db_dir(root).join(&n).join("manifest"))?;
        for e in parse_manifest(&t)? {
            if e.path == rel {
                println!("{n}: /{}", e.path);
                found = true;
            }
        }
    }
    if !found {
        bail!("bu dosya hiçbir jps paketine ait değil: {path}");
    }
    Ok(())
}

pub fn check(root: &Path, name: Option<&str>) -> Result<()> {
    let names = match name {
        Some(n) => {
            if !valid_name(n) {
                bail!("geçersiz paket adı: {n}");
            }
            vec![n.to_string()]
        }
        None => pkg_names(root),
    };
    let (mut bad, mut total) = (0usize, 0usize);
    for n in &names {
        let t = fs::read_to_string(db_dir(root).join(n).join("manifest"))
            .with_context(|| format!("'{n}' kurulu değil"))?;
        for e in parse_manifest(&t)?.into_iter().filter(|e| e.kind == 'f') {
            total += 1;
            let rel = match safe_rel(&e.path) {
                Some(r) => r,
                None => continue,
            };
            match fs::read(root.join(rel)) {
                Err(_) => {
                    println!("EKSİK      {n}: /{}", e.path);
                    bad += 1;
                }
                Ok(b) => {
                    if sha_hex(&b) != e.hash {
                        if e.path.starts_with("etc/") {
                            println!("AYARLI     {n}: /{} (kullanıcı değiştirmiş, sorun sayılmaz)", e.path);
                        } else {
                            println!("DEĞİŞMİŞ   {n}: /{}", e.path);
                            bad += 1;
                        }
                    }
                }
            }
        }
    }
    if bad > 0 {
        bail!("{bad} dosya sorunlu ({} paket, {total} dosya denetlendi)", names.len());
    }
    println!("✔ {} paket, {total} dosya: hepsi sağlam", names.len());
    Ok(())
}

pub fn autoremove(o: &Opts, yes: bool) -> Result<()> {
    let db = db_dir(&o.root);
    let mut deps: HashMap<String, Vec<String>> = HashMap::new();
    let mut auto: HashSet<String> = HashSet::new();
    for n in pkg_names(&o.root) {
        let dir = db.join(&n);
        let c = Control::parse(&fs::read_to_string(dir.join("control"))?)?;
        deps.insert(n.clone(), c.depends);
        if dir.join("auto").is_file() {
            auto.insert(n);
        }
    }
    let mut keep: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = deps.keys().filter(|k| !auto.contains(*k)).cloned().collect();
    while let Some(n) = stack.pop() {
        if !keep.insert(n.clone()) {
            continue;
        }
        if let Some(d) = deps.get(&n) {
            for x in d {
                if deps.contains_key(x) {
                    stack.push(x.clone());
                }
            }
        }
    }
    let mut rm: Vec<String> = auto.iter().filter(|n| !keep.contains(*n)).cloned().collect();
    rm.sort();
    if rm.is_empty() {
        println!("Kaldırılacak gereksiz paket yok.");
        return Ok(());
    }
    println!("Artık gerekmeyen paketler:");
    for n in &rm {
        println!("  {n}");
    }
    if !yes {
        print!("Kaldırılsın mı? [E/h] ");
        std::io::stdout().flush()?;
        let mut a = String::new();
        std::io::stdin().read_line(&mut a)?;
        let a = a.trim().to_lowercase();
        if !(a.is_empty() || a == "e" || a == "evet" || a == "y") {
            println!("İptal edildi.");
            return Ok(());
        }
    }
    for n in rm {
        ops::remove(&n, &Opts { root: o.root.clone(), force: true, no_scripts: o.no_scripts })?;
    }
    Ok(())
}
