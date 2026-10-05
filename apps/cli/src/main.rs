use backupduck_cloud_audit as cloud;
use backupduck_core::*;
use backupduck_store::Receiver;
use backupduck_transport::Client;
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File, OpenOptions},
    io::Write,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

#[derive(Parser)]
#[command(version, about = "BackupDuck reference sender and receiver")]
struct Args {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Create a private authentication token file; its contents are never printed.
    InitToken { path: PathBuf },
    /// Run the reference HTTP receiver on loopback. Native/TLS hosts embed the router.
    Serve {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        token_file: PathBuf,
        #[arg(long, default_value = "127.0.0.1:8484")]
        listen: SocketAddr,
        #[arg(long, default_value_t = 32)]
        capacity_gib: u64,
    },
    /// Build a manifest from original files. Supply --paired-video for a motion asset.
    Manifest {
        #[arg(long)]
        source_id: String,
        #[arg(long, default_value = "1")]
        revision: String,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        paired_video: Option<PathBuf>,
        #[arg(long)]
        output: PathBuf,
    },
    /// Send a manifest. Resource filenames are resolved inside --files.
    Send {
        #[arg(long)]
        server: String,
        #[arg(long)]
        token_file: PathBuf,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        files: PathBuf,
    },
    /// Show a receiver's transfer status for an asset ID.
    Status {
        #[arg(long)]
        server: String,
        #[arg(long)]
        token_file: PathBuf,
        id: String,
    },
    /// Check files against Google Photos through its undocumented web RPC.
    CloudAudit {
        #[command(subcommand)]
        command: CloudCommand,
    },
}
#[derive(Subcommand)]
enum CloudCommand {
    /// Look up files by SHA-1 and report whether they count against quota.
    Lookup {
        /// Netscape cookies.txt from a signed-in photos.google.com session.
        #[arg(long)]
        cookies: PathBuf,
        /// Signed-in account index (the N in photos.google.com/u/N/).
        #[arg(long, default_value_t = 0)]
        account: u32,
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Report whether the exported session is still signed in.
    Status {
        /// Netscape cookies.txt from a signed-in photos.google.com session.
        #[arg(long)]
        cookies: PathBuf,
        /// Signed-in account index (the N in photos.google.com/u/N/).
        #[arg(long, default_value_t = 0)]
        account: u32,
    },
}
fn resource(path: &Path, role: ResourceRole) -> Result<Resource> {
    let filename = path
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or_else(|| Error::Invalid("filename must be valid UTF-8".into()))?
        .to_owned();
    let media_type = match path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "heic" | "heif" => "image/heic",
        "png" => "image/png",
        "mov" => "video/quicktime",
        "mp4" => "video/mp4",
        _ => return Err(Error::Unsupported("file type".into())),
    }
    .to_owned();
    Ok(Resource {
        role,
        filename,
        media_type,
        size: path.metadata()?.len(),
        sha256: digest_reader(File::open(path)?)?,
    })
}
fn token(path: PathBuf) -> Result<String> {
    Ok(fs::read_to_string(path)?.trim().to_owned())
}
#[tokio::main]
async fn main() -> Result<()> {
    match Args::parse().command {
        Command::InitToken { path } => {
            let mut bytes = [0; 32];
            getrandom::fill(&mut bytes)
                .map_err(|_| Error::Storage("random source unavailable".into()))?;
            let mut options = OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(path)?;
            for byte in bytes {
                write!(file, "{byte:02x}")?;
            }
            file.write_all(b"\n")?;
            file.sync_all()?;
            println!("Token file created. Keep it private.");
        }
        Command::Serve {
            root,
            token_file,
            listen,
            capacity_gib,
        } => {
            if !listen.ip().is_loopback() {
                return Err(Error::Invalid(
                    "reference HTTP host only binds loopback; remote hosts require TLS".into(),
                ));
            }
            let capacity = capacity_gib
                .checked_mul(1024 * 1024 * 1024)
                .ok_or(Error::Capacity)?;
            let app =
                backupduck_transport::router(Receiver::open(root, capacity)?, &token(token_file)?)?;
            let listener = tokio::net::TcpListener::bind(listen).await?;
            println!("Receiver listening on {}", listener.local_addr()?);
            axum_serve(listener, app).await?;
        }
        Command::Manifest {
            source_id,
            revision,
            file,
            paired_video,
            output,
        } => {
            let video = matches!(
                file.extension()
                    .and_then(|v| v.to_str())
                    .unwrap_or("")
                    .to_lowercase()
                    .as_str(),
                "mov" | "mp4"
            );
            if video && paired_video.is_some() {
                return Err(Error::Invalid(
                    "--file must be a photo when --paired-video is given".into(),
                ));
            }
            let kind = if paired_video.is_some() {
                AssetKind::Motion
            } else if video {
                AssetKind::Video
            } else {
                AssetKind::Photo
            };
            let mut resources = vec![resource(
                &file,
                if video {
                    ResourceRole::Video
                } else {
                    ResourceRole::Photo
                },
            )?];
            if let Some(path) = paired_video {
                let r = resource(&path, ResourceRole::PairedVideo)?;
                if !r.media_type.starts_with("video/") {
                    return Err(Error::Invalid(
                        "--paired-video must be a .mov or .mp4 file".into(),
                    ));
                }
                resources.push(r);
            }
            let asset = Asset {
                version: PROTOCOL_VERSION,
                source_id,
                revision,
                kind,
                metadata: BTreeMap::new(),
                resources,
            };
            let id = asset.id()?;
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(output)?;
            file.write_all(&serde_json::to_vec_pretty(&asset)?)?;
            file.sync_all()?;
            println!("Manifest created: {id}");
        }
        Command::Send {
            server,
            token_file,
            manifest,
            files,
        } => {
            let asset: Asset = serde_json::from_slice(&fs::read(manifest)?)?;
            asset.validate()?;
            let paths = asset
                .resources
                .iter()
                .map(|r| (r.sha256.clone(), files.join(&r.filename)))
                .collect::<BTreeMap<_, _>>();
            let client = Client::new(&server, &token(token_file)?)?;
            let result = client
                .send(&asset, &paths, &AtomicBool::new(false), |s| {
                    let bytes: u64 = s.resources.iter().map(|r| r.offset).sum();
                    eprintln!(
                        "{}: {} bytes confirmed by the receiver; {:?}",
                        s.asset_id, bytes, s.receipt
                    );
                })
                .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::Status {
            server,
            token_file,
            id,
        } => println!(
            "{}",
            serde_json::to_string_pretty(
                &Client::new(&server, &token(token_file)?)?
                    .status(&id)
                    .await?
            )?
        ),
        Command::CloudAudit { command } => cloud_audit(command)
            .await
            .map_err(|e| Error::Transport(format!("cloud audit: {e}")))?,
    }
    Ok(())
}
fn load_cookies(path: &Path) -> cloud::Result<cloud::CookieFile> {
    let file = cloud::CookieFile::load(path)?;
    #[cfg(unix)]
    {
        if !cloud::cookies::check_permissions(path) {
            eprintln!(
                "warning: {} is readable by other users and grants full Google account access; chmod 600 it",
                path.display()
            );
        }
    }
    Ok(file)
}
async fn cloud_audit(command: CloudCommand) -> cloud::Result<()> {
    match command {
        CloudCommand::Status { cookies, account } => {
            let jar = load_cookies(&cookies)?.jar();
            let valid = match cloud::Session::open(jar, account).await {
                Ok(_) => true,
                Err(cloud::Error::SessionExpired) => false,
                Err(e) => return Err(e),
            };
            println!("{}", json!({ "account": account, "session_valid": valid }));
            if !valid {
                std::process::exit(1);
            }
        }
        CloudCommand::Lookup {
            cookies,
            account,
            files,
        } => {
            let hashes = files
                .iter()
                .map(|path| cloud::sha1_hex(File::open(path)?))
                .collect::<cloud::Result<Vec<_>>>()?;
            let session = cloud::Session::open(load_cookies(&cookies)?.jar(), account).await?;
            let mut auditor = cloud::Auditor::new(session, cloud::Throttle::default());
            let lookups = auditor.lookup_hashes(&hashes).await?;
            let keys: Vec<String> = lookups
                .iter()
                .flatten()
                .map(|l| l.media_key.clone())
                .collect();
            let infos = auditor.item_info(&keys).await?;
            let infos: HashMap<&str, &cloud::ItemInfo> = infos
                .iter()
                .flatten()
                .map(|i| (i.media_key.as_str(), i))
                .collect();
            let report = files
                .iter()
                .zip(&hashes)
                .zip(&lookups)
                .map(|((path, sha1), lookup)| {
                    let info = lookup
                        .as_ref()
                        .and_then(|l| infos.get(l.media_key.as_str()).copied());
                    json!({
                        "file": path.display().to_string(),
                        "sha1": sha1,
                        "found": lookup.is_some(),
                        "media_key": lookup.as_ref().map(|l| &l.media_key),
                        "dedup_key": lookup.as_ref().and_then(|l| l.dedup_key.as_ref()),
                        "device_model": lookup.as_ref().and_then(|l| l.device_model.as_ref()),
                        "takes_up_space": info.and_then(|i| i.takes_up_space),
                        "is_original_quality": info.and_then(|i| i.is_original_quality),
                        "verdict": cloud::verdict(lookup.as_ref(), info),
                    })
                })
                .collect();
            println!("{:#}", Value::Array(report));
            eprintln!("{} RPC calls", auditor.calls());
        }
    }
    Ok(())
}
async fn axum_serve(listener: tokio::net::TcpListener, router: axum::Router) -> Result<()> {
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
