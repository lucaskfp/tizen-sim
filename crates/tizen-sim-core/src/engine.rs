//! The Engine manager: the Engine Manifest, the per-user Engine cache, and
//! launching an Engine with a debugging port.

use futures_util::StreamExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;

/// One Engine Manifest entry: where to get the Engine for a Version Profile
/// on one platform.
#[derive(Debug, Clone, Deserialize)]
pub struct EngineSpec {
    pub profile: String,
    pub milestone: u32,
    pub platform: String,
    pub url: String,
    pub sha256: String,
    /// Path of the browser executable inside the archive.
    pub executable: String,
}

/// Progress of an Engine download, in bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadProgress {
    pub received: u64,
    pub total: Option<u64>,
}

/// An Engine ready to launch.
#[derive(Debug, Clone)]
pub struct InstalledEngine {
    pub executable: PathBuf,
    pub freshly_downloaded: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("no Engine for Version Profile {profile} on {platform}")]
    NotInManifest { profile: String, platform: String },
    #[error("this platform ({0}) is not supported")]
    UnsupportedPlatform(String),
    #[error("downloading the Engine from {url} failed: {source}")]
    Download { url: String, source: reqwest::Error },
    #[error(
        "the Engine download from {url} failed its checksum (expected {expected}, got {actual})"
    )]
    ChecksumMismatch {
        url: String,
        expected: String,
        actual: String,
    },
    #[error("unpacking the Engine failed: {0}")]
    Unpack(String),
    #[error("Engine cache I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not start the Engine at {path}: {source}")]
    Spawn {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("the Engine did not open its debugging port within {0:?}")]
    DebugPortTimeout(Duration),
    #[error("the Engine exited during startup ({0})")]
    ExitedDuringStartup(String),
    #[error("another Session is already running this App on this Version Profile (Engine data in use at {})", .0.display())]
    EngineDataInUse(PathBuf),
}

fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> EngineError + '_ {
    move |source| EngineError::Io {
        path: path.to_path_buf(),
        source,
    }
}

#[derive(Deserialize)]
struct Manifest {
    engine: Vec<EngineSpec>,
}

fn manifest() -> &'static [EngineSpec] {
    static MANIFEST: OnceLock<Vec<EngineSpec>> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        toml::from_str::<Manifest>(include_str!("../data/engines.toml"))
            .expect("bundled engines.toml is valid")
            .engine
    })
}

/// The Engine Manifest platform name for the machine we're running on.
pub fn current_platform() -> Result<&'static str, EngineError> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Ok("windows-x64"),
        ("linux", "x86_64") => Ok("linux-x64"),
        (os, arch) => Err(EngineError::UnsupportedPlatform(format!("{os}-{arch}"))),
    }
}

impl EngineSpec {
    /// Looks up the Engine Manifest entry for a Version Profile on a platform.
    pub fn for_profile(profile: &str, platform: &str) -> Result<&'static EngineSpec, EngineError> {
        manifest()
            .iter()
            .find(|e| e.profile == profile && e.platform == platform)
            .ok_or_else(|| EngineError::NotInManifest {
                profile: profile.to_string(),
                platform: platform.to_string(),
            })
    }

    fn install_dir_name(&self) -> String {
        format!("{}-{}-{}", self.profile, self.platform, &self.sha256[..12])
    }
}

/// The per-user Engine cache.
pub struct EngineStore {
    root: PathBuf,
}

const INSTALLED_MARKER: &str = ".installed";

impl EngineStore {
    pub fn new(root: PathBuf) -> EngineStore {
        EngineStore { root }
    }

    /// Returns the installed Engine for `spec`, downloading, verifying and
    /// unpacking it first if it isn't cached yet.
    pub async fn ensure(
        &self,
        spec: &EngineSpec,
        mut on_progress: impl FnMut(DownloadProgress),
    ) -> Result<InstalledEngine, EngineError> {
        // Sessions in one process share the cache; don't download twice.
        static INSTALL_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let _guard = INSTALL_LOCK.lock().await;

        let dir = self.root.join(spec.install_dir_name());
        let executable = dir.join(&spec.executable);
        if dir.join(INSTALLED_MARKER).is_file() {
            return Ok(InstalledEngine {
                executable,
                freshly_downloaded: false,
            });
        }

        tokio::fs::create_dir_all(&self.root)
            .await
            .map_err(io_err(&self.root))?;
        let unique = unique_suffix();
        let archive = self.root.join(format!(".download-{unique}.zip"));
        let staging = self.root.join(format!(".staging-{unique}"));
        self.download_and_unpack(spec, &archive, &staging, &dir, &mut on_progress)
            .await?;
        Ok(InstalledEngine {
            executable,
            freshly_downloaded: true,
        })
    }

    async fn download_and_unpack(
        &self,
        spec: &EngineSpec,
        archive: &Path,
        staging: &Path,
        dir: &Path,
        on_progress: &mut impl FnMut(DownloadProgress),
    ) -> Result<(), EngineError> {
        // Declared before the archive's file handle so it drops after it:
        // cleans up on success, failure, and cancellation (e.g. Ctrl+C).
        let _cleanup = RemoveOnDrop(vec![archive.to_path_buf(), staging.to_path_buf()]);
        let download_err = |source| EngineError::Download {
            url: spec.url.clone(),
            source,
        };
        let response = reqwest::get(&spec.url)
            .await
            .and_then(|r| r.error_for_status())
            .map_err(download_err)?;
        let total = response.content_length();
        let mut file = tokio::fs::File::create(archive)
            .await
            .map_err(io_err(archive))?;
        let mut hasher = Sha256::new();
        let mut received = 0u64;
        let mut last_reported = 0u64;
        let mut body = response.bytes_stream();
        on_progress(DownloadProgress { received, total });
        while let Some(chunk) = body.next().await {
            let chunk = chunk.map_err(download_err)?;
            hasher.update(&chunk);
            file.write_all(&chunk).await.map_err(io_err(archive))?;
            received += chunk.len() as u64;
            if received - last_reported >= 1 << 20 || Some(received) == total {
                last_reported = received;
                on_progress(DownloadProgress { received, total });
            }
        }
        file.flush().await.map_err(io_err(archive))?;
        drop(file);

        let actual = hex::encode(hasher.finalize());
        if !actual.eq_ignore_ascii_case(&spec.sha256) {
            return Err(EngineError::ChecksumMismatch {
                url: spec.url.clone(),
                expected: spec.sha256.clone(),
                actual,
            });
        }

        let (archive_owned, staging_owned) = (archive.to_path_buf(), staging.to_path_buf());
        tokio::task::spawn_blocking(move || unpack(&archive_owned, &staging_owned))
            .await
            .map_err(|e| EngineError::Unpack(e.to_string()))??;
        grant_sandbox_access(staging).await?;
        tokio::fs::write(staging.join(INSTALLED_MARKER), &spec.sha256)
            .await
            .map_err(io_err(staging))?;
        tokio::fs::rename(staging, dir).await.map_err(io_err(dir))?;
        Ok(())
    }
}

/// Removes temporary download files and directories when dropped.
struct RemoveOnDrop(Vec<PathBuf>);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        for path in &self.0 {
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(path);
            } else {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

fn unique_suffix() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn unpack(archive: &Path, into: &Path) -> Result<(), EngineError> {
    let file = std::fs::File::open(archive).map_err(io_err(archive))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| EngineError::Unpack(e.to_string()))?;
    zip.extract(into)
        .map_err(|e| EngineError::Unpack(e.to_string()))
}

/// Chromium's sandboxed child processes run in an AppContainer and must be
/// able to read the Engine's files. The official installer grants this; a
/// portable build needs it done by hand (finding from the #24 prototype).
#[cfg(windows)]
async fn grant_sandbox_access(dir: &Path) -> Result<(), EngineError> {
    let status = tokio::process::Command::new("icacls")
        .arg(dir)
        .args([
            "/grant",
            "*S-1-15-2-1:(OI)(CI)(RX)",
            "*S-1-15-2-2:(OI)(CI)(RX)",
            "/T",
            "/Q",
        ])
        .stdout(Stdio::null())
        .status()
        .await
        .map_err(io_err(dir))?;
    if status.success() {
        Ok(())
    } else {
        Err(EngineError::Unpack(format!(
            "granting AppContainer read access to {} failed ({status})",
            dir.display()
        )))
    }
}

#[cfg(not(windows))]
async fn grant_sandbox_access(_dir: &Path) -> Result<(), EngineError> {
    Ok(())
}

/// How to start an Engine.
pub struct LaunchOptions {
    pub user_data_dir: PathBuf,
    pub headless: bool,
    /// The Session's network proxy, which all of the Engine's traffic goes
    /// through, loopback included (ADR-0003).
    pub network_proxy: Option<std::net::SocketAddr>,
    /// Turns off the same-origin policy, so CORS doesn't block requests
    /// (ADR-0003).
    pub disable_web_security: bool,
}

/// A started Engine with its debugging endpoint.
pub struct RunningEngine {
    pub child: tokio::process::Child,
    pub engine_version: String,
    pub debug_port: u16,
    /// WebSocket URL of the Engine's initial page target.
    pub page_ws_url: String,
    /// The Engine's own bundled inspector (DevTools), attached to that page.
    /// Served by the Engine, so it speaks its protocol whatever its age.
    pub devtools_url: String,
}

/// First launches of a freshly unpacked Engine can be slow (antivirus scans),
/// so startup gets a generous budget.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(90);

pub async fn launch(
    executable: &Path,
    options: &LaunchOptions,
) -> Result<RunningEngine, EngineError> {
    tokio::fs::create_dir_all(&options.user_data_dir)
        .await
        .map_err(io_err(&options.user_data_dir))?;
    let debug_port = free_port().map_err(io_err(executable))?;

    let mut command = tokio::process::Command::new(executable);
    command
        .arg(format!(
            "--user-data-dir={}",
            options.user_data_dir.display()
        ))
        .arg(format!("--remote-debugging-port={debug_port}"))
        // The Engine's bundled inspector, served from the debugging port,
        // may connect back to it (refused without this since Chromium M111).
        .arg(format!(
            "--remote-allow-origins=http://127.0.0.1:{debug_port}"
        ))
        .args(["--no-first-run", "--no-default-browser-check"]);
    if let Some(proxy) = options.network_proxy {
        command
            .arg(format!("--proxy-server=http://{proxy}"))
            // Loopback skips proxies unless told otherwise (Chromium M72+).
            .arg("--proxy-bypass-list=<-loopback>")
            // The Engine's own requests would show up in the network log as
            // the App's: background updates, the Web Store extension, the
            // search engine preconnect...
            .args([
                "--disable-background-networking",
                "--disable-component-update",
                "--disable-extensions",
                "--disable-features=PreconnectToSearch",
            ])
            // ...and Google account checks, which go to the proxy itself,
            // where they are dropped.
            .arg(format!("--gaia-url=http://{proxy}/"));
    }
    if options.disable_web_security {
        command.arg("--disable-web-security");
    }
    if options.headless {
        command.args(["--headless=new", "about:blank"]);
    } else {
        command.arg("--app=about:blank");
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    containment::die_with_parent(&mut command);
    let mut child = command.spawn().map_err(|source| EngineError::Spawn {
        path: executable.to_path_buf(),
        source,
    })?;
    containment::tie_to_this_process(&child);

    let http = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{debug_port}");
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().ok().flatten() {
            // Chromium hands a launch over to the instance already using the data
            // directory and exits, which is how a clash shows up.
            if engine_data_in_use(&options.user_data_dir) {
                return Err(EngineError::EngineDataInUse(options.user_data_dir.clone()));
            }
            return Err(EngineError::ExitedDuringStartup(status.to_string()));
        }
        if started.elapsed() > STARTUP_TIMEOUT {
            let _ = child.kill().await;
            return Err(EngineError::DebugPortTimeout(STARTUP_TIMEOUT));
        }
        if let Some((engine_version, page_ws_url)) = discover(&http, &base).await {
            let devtools_url = devtools_url(debug_port, &page_ws_url);
            return Ok(RunningEngine {
                child,
                engine_version,
                debug_port,
                page_ws_url,
                devtools_url,
            });
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn discover(http: &reqwest::Client, base: &str) -> Option<(String, String)> {
    let get = |path: &str| {
        let url = format!("{base}{path}");
        async move {
            http.get(url)
                .timeout(Duration::from_secs(2))
                .send()
                .await
                .ok()?
                .json::<serde_json::Value>()
                .await
                .ok()
        }
    };
    let version = get("/json/version").await?;
    let targets = get("/json/list").await?;
    let page = targets.as_array()?.iter().find(|t| t["type"] == "page")?;
    Some((
        version["Browser"].as_str()?.to_string(),
        page["webSocketDebuggerUrl"].as_str()?.to_string(),
    ))
}

/// The bundled inspector's address for a page target. `/json/list` gives it
/// as `devtoolsFrontendUrl`, but with `localhost`, which isn't the origin the
/// Engine allows (see `launch`).
fn devtools_url(debug_port: u16, page_ws_url: &str) -> String {
    let page = page_ws_url.rsplit('/').next().unwrap_or_default();
    format!(
        "http://127.0.0.1:{debug_port}/devtools/inspector.html?ws=127.0.0.1:{debug_port}/devtools/page/{page}"
    )
}

/// Opens `url` in a window of its own, in a separate instance of the Engine
/// at `executable` with its own data directory: the bundled inspector then
/// runs on the Engine it came from, outside the App's network proxy. The
/// window closes when the returned process is dropped.
pub async fn open_window(
    executable: &Path,
    user_data_dir: &Path,
    url: &str,
) -> Result<tokio::process::Child, EngineError> {
    tokio::fs::create_dir_all(user_data_dir)
        .await
        .map_err(io_err(user_data_dir))?;
    let mut command = tokio::process::Command::new(executable);
    command
        .arg(format!("--user-data-dir={}", user_data_dir.display()))
        .args(["--no-first-run", "--no-default-browser-check"])
        .arg(format!("--app={url}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    containment::die_with_parent(&mut command);
    let child = command.spawn().map_err(|source| EngineError::Spawn {
        path: executable.to_path_buf(),
        source,
    })?;
    containment::tie_to_this_process(&child);
    Ok(child)
}

/// Whether a running Engine holds the lock on its data directory.
fn engine_data_in_use(user_data_dir: &Path) -> bool {
    #[cfg(windows)]
    {
        // Chromium keeps `lockfile` open without sharing while it runs.
        use std::os::windows::fs::OpenOptionsExt;
        let lockfile = user_data_dir.join("lockfile");
        lockfile.exists()
            && std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&lockfile)
                .is_err()
    }
    #[cfg(not(windows))]
    {
        // `SingletonLock` is a symlink to `<hostname>-<pid>`.
        std::fs::read_link(user_data_dir.join("SingletonLock"))
            .ok()
            .and_then(|target| {
                let target = target.to_string_lossy().into_owned();
                target.rsplit('-').next()?.parse::<i32>().ok()
            })
            .is_some_and(|pid| unsafe { libc::kill(pid, 0) } == 0)
    }
}

/// Makes an Engine die with the process that started it, even when that
/// process is killed without a chance to clean up.
mod containment {
    #[cfg(windows)]
    pub fn die_with_parent(_command: &mut tokio::process::Command) {}

    /// Puts the Engine in a Job Object that kills its members when the last
    /// handle closes, i.e. when this process exits for any reason. Chromium's
    /// own child processes inherit the job.
    #[cfg(windows)]
    pub fn tie_to_this_process(child: &tokio::process::Child) {
        use std::sync::OnceLock;
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::System::JobObjects::*;
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
        };

        struct Job(HANDLE);
        // The handle is only used to assign processes; it lives for the whole process.
        unsafe impl Send for Job {}
        unsafe impl Sync for Job {}

        static JOB: OnceLock<Option<Job>> = OnceLock::new();
        let job = JOB.get_or_init(|| unsafe {
            let job = CreateJobObjectW(None, None).ok()?;
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of_val(&limits) as u32,
            )
            .ok()?;
            Some(Job(job))
        });
        let (Some(job), Some(pid)) = (job, child.id()) else {
            tracing::warn!("could not tie the Engine to this process; it may outlive a crash");
            return;
        };
        unsafe {
            match OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid) {
                Ok(process) => {
                    if let Err(error) = AssignProcessToJobObject(job.0, process) {
                        tracing::warn!(%error, "could not add the Engine to the job object");
                    }
                    let _ = CloseHandle(process);
                }
                Err(error) => tracing::warn!(%error, "could not open the Engine process"),
            }
        }
    }

    /// Asks the kernel to SIGKILL the Engine when its parent dies, and keeps
    /// it out of the terminal's process group so Ctrl+C reaches only the
    /// owner, which then stops the Session in order.
    #[cfg(unix)]
    pub fn die_with_parent(command: &mut tokio::process::Command) {
        command.process_group(0);
        unsafe {
            command.pre_exec(|| {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    #[cfg(unix)]
    pub fn tie_to_this_process(_child: &tokio::process::Child) {}
}

fn free_port() -> std::io::Result<u16> {
    Ok(std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port())
}
