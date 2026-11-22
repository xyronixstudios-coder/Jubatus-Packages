mod extra;
mod format;
mod gui;
mod ops;
mod repo;
mod selfupdate;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "jps", version, about = "Jubatus Forge — .jps paket oluşturucu ve yöneticisi")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Args)]
struct Common {
    /// Kurulum kökü (varsayılan: /)
    #[arg(long, default_value = "/")]
    root: PathBuf,
    /// Bağımlılık/çakışma/mimari denetimlerini atla
    #[arg(long)]
    force: bool,
    /// preinst/postinst/prerm/postrm betiklerini çalıştırma
    #[arg(long)]
    no_scripts: bool,
}

impl Common {
    fn opts(self) -> ops::Opts {
        ops::Opts { root: self.root, force: self.force, no_scripts: self.no_scripts }
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Yeni paket iskeleti oluştur
    Init { dir: PathBuf },
    /// Dizinden .jps paketi üret (JPS/icon.png varsa simge olarak gömer)
    Build {
        dir: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Paket bilgilerini göster
    Info { file: PathBuf },
    /// Paket içeriğini listele
    Contents { file: PathBuf },
    /// Paketin bütünlüğünü ve imzasını doğrula
    Verify { file: PathBuf },
    /// Paketi kurmadan bir dizine çıkar
    Extract { file: PathBuf, dest: PathBuf },
    /// Paket kur: depodan ada göre (jps install htop) veya .jps dosyasından
    Install {
        #[arg(required = true)]
        targets: Vec<String>,
        /// Onay sorma
        #[arg(short, long)]
        yes: bool,
        /// Dosya hedeflerinde yalnızca güvenilir anahtarla imzalıları kabul et
        #[arg(long)]
        require_trusted: bool,
        #[command(flatten)]
        c: Common,
    },
    /// Kurulu paketi kaldır
    Remove {
        name: String,
        #[command(flatten)]
        c: Common,
    },
    /// Kurulu paketleri listele
    List {
        #[arg(long, default_value = "/")]
        root: PathBuf,
    },
    /// Depo paket dizinini indir
    Update {
        #[command(flatten)]
        c: Common,
    },
    /// Kurulu paketleri depodaki son sürüme yükselt
    Upgrade {
        #[arg(short, long)]
        yes: bool,
        #[command(flatten)]
        c: Common,
    },
    /// Depoda ara
    Search {
        term: String,
        #[arg(long, default_value = "/")]
        root: PathBuf,
    },
    /// Depodaki paketin ayrıntılarını göster
    Show {
        name: String,
        #[arg(long, default_value = "/")]
        root: PathBuf,
    },
    /// Depo imza anahtarını sistem genelinde güvenilir yap
    TrustRepo {
        #[command(flatten)]
        c: Common,
    },
    /// İndirme önbelleğini temizle
    Clean {
        #[command(flatten)]
        c: Common,
    },
    /// jps kendisini depodaki son sürüme günceller
    SelfUpdate {
        #[arg(long)]
        force: bool,
    },
    /// Ed25519 imza anahtarı üret
    Keygen,
    /// Paketi imzala
    Sign {
        file: PathBuf,
        #[arg(long)]
        key: Option<PathBuf>,
    },
    /// Bir anahtarı (.pub veya imzalı .jps) güvenilir yap
    Trust {
        file: PathBuf,
        #[arg(long)]
        system: bool,
    },
    /// Pakete PNG simge göm
    SetIcon { file: PathBuf, png: PathBuf },
    /// .jps simgelerini/küçük resimlerini dosya yöneticisine kaydet
    DesktopInstall {
        #[arg(long)]
        system: bool,
    },
    /// Masaüstü entegrasyonunu kaldır
    DesktopRemove {
        #[arg(long)]
        system: bool,
    },
    /// (dahili) dosya yöneticisi küçük resmi üretir
    #[command(hide = true)]
    Thumbnail { input: PathBuf, output: PathBuf, size: u32 },
    /// (dahili) çift tıklamayla açılır (grafik pencere)
    #[command(hide = true)]
    Open { file: PathBuf },
    /// (dahili) terminal tabanlı açılış
    #[command(hide = true)]
    OpenTty { file: PathBuf },
}

fn run() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Init { dir } => {
            ops::init(&dir)?;
            extra::write_default_icon(&dir)
        }
        Cmd::Build { dir, output } => {
            let p = ops::build(&dir, output)?;
            let icon = dir.join("JPS/icon.png");
            if icon.is_file() {
                extra::set_icon(&p, &icon)?;
            }
            println!("✔ paket oluşturuldu: {}", p.display());
            Ok(())
        }
        Cmd::Info { file } => {
            ops::info(&file)?;
            extra::print_sig(&file)
        }
        Cmd::Contents { file } => ops::contents(&file),
        Cmd::Verify { file } => {
            ops::verify(&file)?;
            extra::print_sig(&file)
        }
        Cmd::Extract { file, dest } => ops::extract(&file, &dest),
        Cmd::Install { targets, yes, require_trusted, c } => {
            repo::install(&targets, &c.opts(), yes, require_trusted)
        }
        Cmd::Remove { name, c } => ops::remove(&name, &c.opts()),
        Cmd::List { root } => ops::list(&root),
        Cmd::Update { c } => repo::update(&c.opts()),
        Cmd::Upgrade { yes, c } => repo::upgrade(&c.opts(), yes),
        Cmd::Search { term, root } => repo::search(&root, &term),
        Cmd::Show { name, root } => repo::show(&root, &name),
        Cmd::TrustRepo { c } => repo::trust_repo(&c.opts()),
        Cmd::Clean { c } => repo::clean(&c.opts()),
        Cmd::SelfUpdate { force } => selfupdate::run(force),
        Cmd::Keygen => extra::keygen(),
        Cmd::Sign { file, key } => extra::sign(&file, key),
        Cmd::Trust { file, system } => extra::trust(&file, system),
        Cmd::SetIcon { file, png } => {
            extra::set_icon(&file, &png)?;
            println!("✔ simge gömüldü");
            Ok(())
        }
        Cmd::DesktopInstall { system } => extra::desktop_install(system),
        Cmd::DesktopRemove { system } => extra::desktop_remove(system),
        Cmd::Thumbnail { input, output, size } => extra::thumbnail(&input, &output, size),
        Cmd::Open { file } => gui::open(&file),
        Cmd::OpenTty { file } => extra::open(&file),
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("jps: hata: {e:#}");
        std::process::exit(1);
    }
}
