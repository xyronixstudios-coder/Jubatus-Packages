//! zenity ile grafik kurulum penceresi (çift tıklama)

use crate::extra::{self, Sig};
use crate::format::*;
use crate::repo;
use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn have_zenity() -> bool {
    Command::new("zenity")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn have_display() -> bool {
    std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

fn msg(kind: &str, text: &str) {
    let _ = Command::new("zenity")
        .arg(kind)
        .args(["--title", "Jubatus Forge", "--width", "480", "--text", text])
        .status();
}

fn tail_of(p: &Path) -> String {
    let t = fs::read(p).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let lines: Vec<&str> = t.lines().collect();
    let from = lines.len().saturating_sub(20);
    lines[from..].join("\n")
}

pub fn open(file: &Path) -> Result<()> {
    let file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    if !have_display() {
        return extra::open(&file);
    }
    if !have_zenity() {
        return in_terminal(&file);
    }
    gui(&file)
}

fn in_terminal(file: &Path) -> Result<()> {
    let exe = std::env::current_exe()?;
    let terms: [(&str, &str); 5] = [
        ("x-terminal-emulator", "-e"),
        ("gnome-terminal", "--"),
        ("konsole", "-e"),
        ("xfce4-terminal", "-x"),
        ("xterm", "-e"),
    ];
    for (t, flag) in terms {
        if Command::new(t).arg(flag).arg(&exe).arg("open-tty").arg(file).status().is_ok() {
            return Ok(());
        }
    }
    Ok(())
}

fn gui(file: &Path) -> Result<()> {
    let pkg = match read_package(file) {
        Ok(p) => p,
        Err(e) => {
            msg(
                "--error",
                &format!("<b>Geçersiz veya bozuk .jps dosyası</b>\n\n{}", esc(&format!("{e:#}"))),
            );
            return Ok(());
        }
    };
    let c = match pkg.get("control").and_then(|b| std::str::from_utf8(b).ok()).map(Control::parse) {
        Some(Ok(c)) => c,
        _ => {
            msg("--error", "<b>Paket bilgisi okunamadı</b>");
            return Ok(());
        }
    };

    let (sig_line, bad) = match extra::sig_status(&pkg) {
        Sig::Missing => ("⚠ İmzasız paket — yalnızca kaynağına güveniyorsan kur".to_string(), false),
        Sig::Bad(m) => (format!("✘ {}", esc(&m)), true),
        Sig::Valid { fp, trusted: true } => (format!("✔ Güvenilir imza ({fp})"), false),
        Sig::Valid { fp, trusted: false } => (
            format!("⚠ İmza geçerli, ancak anahtar güvenilir listede değil ({fp})"),
            false,
        ),
    };
    if bad {
        msg("--error", &format!("<b>Kurulum engellendi</b>\n\n{sig_line}"));
        return Ok(());
    }

    let root = Path::new("/");
    let missing: Vec<String> = c
        .depends
        .iter()
        .filter(|d| repo::installed_version(root, d).is_none())
        .cloned()
        .collect();
    let kb = pkg.get("data").map(|d| d.len()).unwrap_or(0) / 1024;
    let n = pkg
        .get("manifest")
        .map(|m| m.iter().filter(|&&b| b == b'\n').count())
        .unwrap_or(0);

    let mut text = format!(
        "<b><big>{}</big></b>  <i>{}</i>\n{}\n\n",
        esc(&c.package),
        esc(&c.version),
        esc(&c.description)
    );
    text.push_str(&format!(
        "Mimari: {}\nBakımcı: {}\nİçerik: {} girdi, {} KB\n",
        esc(&c.architecture),
        esc(&c.maintainer),
        n,
        kb
    ));
    if let Some(v) = repo::installed_version(root, &c.package) {
        text.push_str(&format!("Kurulu sürüm: {}\n", esc(&v)));
    }
    if !missing.is_empty() {
        text.push_str(&format!(
            "Eksik bağımlılıklar: {} (depodan kurulacak)\n",
            esc(&missing.join(", "))
        ));
    }
    text.push_str(&format!(
        "\n{sig_line}\n\n<small>Kurulum betikleri yönetici yetkisiyle çalışır.</small>"
    ));

    let ok = Command::new("zenity")
        .args([
            "--question",
            "--title",
            "Jubatus Forge — Paket Kur",
            "--width",
            "480",
            "--ok-label",
            "Kur",
            "--cancel-label",
            "İptal",
            "--text",
            text.as_str(),
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return Ok(());
    }

    let exe = std::env::current_exe()?;
    let log = PathBuf::from(format!("/tmp/jps-gui-{}.log", std::process::id()));
    let logf = fs::File::create(&log)?;
    let logf2 = logf.try_clone()?;
    let mut prog = Command::new("zenity")
        .args([
            "--progress",
            "--pulsate",
            "--no-cancel",
            "--title",
            "Jubatus Forge",
            "--text",
            "Kuruluyor, lütfen bekleyin...",
            "--width",
            "380",
        ])
        .stdin(Stdio::piped())
        .spawn()
        .ok();
    let st = Command::new("pkexec")
        .arg(&exe)
        .args(["install", "--yes"])
        .arg(file)
        .stdout(logf)
        .stderr(logf2)
        .status();
    if let Some(p) = prog.as_mut() {
        let _ = p.kill();
        let _ = p.wait();
    }
    let tail = tail_of(&log);
    let _ = fs::remove_file(&log);
    match st {
        Ok(s) if s.success() => msg(
            "--info",
            &format!("<b>✔ {} {} kuruldu</b>", esc(&c.package), esc(&c.version)),
        ),
        Ok(s) if matches!(s.code(), Some(126) | Some(127)) => {
            msg("--warning", "Kurulum iptal edildi veya yetki verilmedi.")
        }
        Ok(_) => msg("--error", &format!("<b>Kurulum başarısız</b>\n\n<tt>{}</tt>", esc(&tail))),
        Err(e) => msg("--error", &format!("pkexec başlatılamadı: {}", esc(&e.to_string()))),
    }
    Ok(())
}
