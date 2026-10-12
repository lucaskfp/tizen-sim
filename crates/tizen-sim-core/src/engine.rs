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
    /// Whether the Engine can run headless on this platform.
    #[serde(default = "default_true")]
    pub headless: bool,
    /// Whether the Engine runs with Chromium's sandbox. Old Linux builds
    /// need a setuid-root helper for it, which a per-user download can't
    /// have, so they run without (`--no-sandbox`).
    #[serde(default = "default_true")]
    pub sandbox: bool,
    /// Prefixes of dynamic symbols the executable exports that are renamed
    /// on install, so system libraries don't bind to them (see
    /// `elf_exports`). Linux only.
    #[serde(default)]
    pub unexport: Vec<String>,
}

fn default_true() -> bool {
    true
}

/// Progress of an Engine download, in bytes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

/// The Engine Manifest: where to get the Engine for each Version Profile on
/// each platform. Bundled as data (`data/engines.toml`), so updating a
/// download needs no code change.
#[derive(Debug, Deserialize)]
pub struct EngineManifest {
    #[serde(rename = "engine")]
    engines: Vec<EngineSpec>,
}

impl EngineManifest {
    pub fn parse(toml: &str) -> Result<EngineManifest, toml::de::Error> {
        toml::from_str(toml)
    }

    /// The manifest bundled with the simulator.
    pub fn bundled() -> &'static EngineManifest {
        static MANIFEST: OnceLock<EngineManifest> = OnceLock::new();
        MANIFEST.get_or_init(|| {
            EngineManifest::parse(include_str!("../data/engines.toml"))
                .expect("bundled engines.toml is valid")
        })
    }

    /// Looks up the entry for a Version Profile on a platform.
    pub fn for_profile(&self, profile: &str, platform: &str) -> Result<&EngineSpec, EngineError> {
        self.engines
            .iter()
            .find(|e| e.profile == profile && e.platform == platform)
            .ok_or_else(|| EngineError::NotInManifest {
                profile: profile.to_string(),
                platform: platform.to_string(),
            })
    }
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
    /// Looks up the bundled Engine Manifest entry for a Version Profile on a
    /// platform.
    pub fn for_profile(profile: &str, platform: &str) -> Result<&'static EngineSpec, EngineError> {
        EngineManifest::bundled().for_profile(profile, platform)
    }

    /// The Version Profile's pinned Engine on this platform, if any. A
    /// developer's own build of the same milestone (`--engine-path`) shares
    /// its needs: whether it has a headless mode, whether it runs with
    /// Chromium's sandbox.
    pub fn pinned(profile: &str) -> Option<&'static EngineSpec> {
        current_platform()
            .and_then(|platform| EngineSpec::for_profile(profile, platform))
            .ok()
    }

    fn install_dir_name(&self) -> String {
        format!("{}-{}-{}", self.profile, self.platform, &self.sha256[..12])
    }
}

/// An Engine in the cache.
#[derive(Debug, Clone)]
pub struct CachedEngine {
    pub profile: String,
    pub platform: String,
    pub dir: PathBuf,
    /// Disk space used, in bytes.
    pub size: u64,
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

    /// The Engines in the cache, including ones for downloads the Engine
    /// Manifest no longer lists.
    pub fn list(&self) -> Result<Vec<CachedEngine>, EngineError> {
        let entries = match std::fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(error) => return Err(io_err(&self.root)(error)),
        };
        let mut cached = vec![];
        for entry in entries {
            let dir = entry.map_err(io_err(&self.root))?.path();
            if !dir.join(INSTALLED_MARKER).is_file() {
                continue;
            }
            // Named by `install_dir_name`: profile-platform-checksum.
            let name = dir.file_name().unwrap_or_default().to_string_lossy();
            let Some((profile, rest)) = name.split_once('-') else {
                continue;
            };
            let Some((platform, _)) = rest.rsplit_once('-') else {
                continue;
            };
            cached.push(CachedEngine {
                profile: profile.to_string(),
                platform: platform.to_string(),
                size: dir_size(&dir),
                dir,
            });
        }
        cached.sort_by(|a, b| (&a.profile, &a.platform).cmp(&(&b.profile, &b.platform)));
        Ok(cached)
    }

    /// Removes every cached Engine of a Version Profile, returning them.
    pub fn remove(&self, profile: &str) -> Result<Vec<CachedEngine>, EngineError> {
        let removed: Vec<CachedEngine> = self
            .list()?
            .into_iter()
            .filter(|engine| engine.profile == profile)
            .collect();
        for engine in &removed {
            // Moved aside first: renaming fails as a whole while the Engine is
            // running, where deleting in place would leave half an Engine.
            let doomed = self.root.join(format!(".removing-{}", unique_suffix()));
            std::fs::rename(&engine.dir, &doomed).map_err(io_err(&engine.dir))?;
            std::fs::remove_dir_all(&doomed).map_err(io_err(&doomed))?;
        }
        Ok(removed)
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
        if !spec.unexport.is_empty() {
            let (executable, prefixes) = (staging.join(&spec.executable), spec.unexport.clone());
            tokio::task::spawn_blocking(move || rename_exports(&executable, &prefixes))
                .await
                .map_err(|e| EngineError::Unpack(e.to_string()))??;
        }
        grant_sandbox_access(staging).await?;
        tokio::fs::write(staging.join(INSTALLED_MARKER), &spec.sha256)
            .await
            .map_err(io_err(staging))?;
        match tokio::fs::rename(staging, dir).await {
            Ok(()) => Ok(()),
            // Another process installed the same Engine meanwhile; theirs is
            // as good as ours, which is cleaned up with the staging directory.
            Err(_) if dir.join(INSTALLED_MARKER).is_file() => Ok(()),
            Err(error) => Err(io_err(dir)(error)),
        }
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

fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => dir_size(&entry.path()),
            Ok(_) => entry.metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
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

/// Applies the Engine Manifest's `unexport` to the unpacked executable.
fn rename_exports(executable: &Path, prefixes: &[String]) -> Result<(), EngineError> {
    let mut elf = std::fs::read(executable).map_err(io_err(executable))?;
    crate::elf_exports::unexport(&mut elf, prefixes).map_err(|e| {
        EngineError::Unpack(format!(
            "could not rename the exports of {}: {e}",
            executable.display()
        ))
    })?;
    std::fs::write(executable, elf).map_err(io_err(executable))
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

/// Where an Engine draws the App.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineWindow {
    /// An app window on the desktop.
    Shown,
    /// No window at all.
    Headless,
    /// A window placed off the screen, for an Engine without a headless mode
    /// (see `EngineSpec::headless`). It still draws, so screenshots work.
    OffScreen,
}

/// How to start an Engine.
pub struct LaunchOptions {
    pub user_data_dir: PathBuf,
    pub window: EngineWindow,
    /// Sent in place of the Engine's own user agent.
    pub user_agent: Option<String>,
    /// The Session's network proxy, which all of the Engine's traffic goes
    /// through, loopback included (ADR-0003).
    pub network_proxy: Option<std::net::SocketAddr>,
    /// Turns off the same-origin policy, so CORS doesn't block requests
    /// (ADR-0003).
    pub disable_web_security: bool,
    /// Whether to keep Chromium's sandbox on (see `EngineSpec::sandbox`).
    pub sandbox: bool,
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
    /// The private display an off-screen Engine draws on, if it has one;
    /// it stops with the Engine.
    display: Option<virtual_display::VirtualDisplay>,
}

impl RunningEngine {
    /// The number of the private X display the Engine draws on, if it has
    /// one (an off-screen Engine on Linux with Xvfb installed).
    pub fn display(&self) -> Option<u32> {
        self.display
            .as_ref()
            .map(virtual_display::VirtualDisplay::number)
    }
}

/// Where an off-screen Engine window goes: far beyond any monitor, but not
/// at Windows' minimized-window position (-32000), which would hide the page.
const OFF_SCREEN: (i32, i32) = (-20000, -20000);

/// First launches of a freshly unpacked Engine can be slow (antivirus scans),
/// so startup gets a generous budget.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(90);

/// How long a launch waits for an Engine going away (e.g. its CLI killed) to
/// let go of the data directory, before taking it for one still running.
const DATA_RELEASE_TIMEOUT: Duration = Duration::from_secs(3);

/// Every Engine window starts without Chromium's first-run prompts, and
/// without the info bar warning about our flags (`--test-type`), which
/// would take room from the App.
const QUIET_START: [&str; 3] = [
    "--no-first-run",
    "--no-default-browser-check",
    "--test-type",
];

pub async fn launch(
    executable: &Path,
    options: &LaunchOptions,
) -> Result<RunningEngine, EngineError> {
    tokio::fs::create_dir_all(&options.user_data_dir)
        .await
        .map_err(io_err(&options.user_data_dir))?;
    // Launched while another Engine holds the data directory, Chromium hands
    // over to it and exits.
    let waiting = Instant::now();
    while engine_data_in_use(&options.user_data_dir) {
        if waiting.elapsed() > DATA_RELEASE_TIMEOUT {
            return Err(EngineError::EngineDataInUse(options.user_data_dir.clone()));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
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
        .args(QUIET_START)
        // A window behind others (or embedded in the GUI, ADR-0006) must keep
        // drawing: otherwise Page.captureScreenshot never answers. Hiding the
        // App minimizes and freezes it explicitly, so it is unaffected.
        .args([
            "--disable-backgrounding-occluded-windows",
            "--disable-renderer-backgrounding",
        ])
        // AVPlay's Video Plane plays decodable streams in a <video> (ADR-0004),
        // which a TV plays without a user gesture. Engines before the policy
        // (M66) ignore the flag.
        .arg("--autoplay-policy=no-user-gesture-required");
    if let Some(proxy) = options.network_proxy {
        command
            .arg(format!("--proxy-server=http://{proxy}"))
            // Loopback skips proxies unless told otherwise (Chromium M72+).
            .arg("--proxy-bypass-list=<-loopback>")
            // The Engine's own requests would show up in the network log as
            // the App's: background updates, the Web Store extension, the
            // search engine preconnect, IP Protection's token requests
            // (phosphor-pa.googleapis.com, measured on M120), the phishing
            // detection model (ssl.gstatic.com, measured on M63)...
            .args([
                "--disable-background-networking",
                "--disable-client-side-phishing-detection",
                "--disable-component-update",
                "--disable-extensions",
                "--disable-features=PreconnectToSearch,EnableIpPrivacyProxy",
            ])
            // ...and Google account checks, which go to the proxy itself,
            // where they are dropped.
            .arg(format!("--gaia-url=http://{proxy}/"));
    }
    if options.disable_web_security {
        command.arg("--disable-web-security");
    }
    if !options.sandbox {
        command.arg("--no-sandbox");
    }
    if let Some(user_agent) = &options.user_agent {
        command.arg(format!("--user-agent={user_agent}"));
    }
    let mut display = None;
    match options.window {
        EngineWindow::Headless => {
            command.args(["--headless=new", "about:blank"]);
        }
        EngineWindow::Shown | EngineWindow::OffScreen => {
            if options.window == EngineWindow::OffScreen {
                command.arg(format!(
                    "--window-position={},{}",
                    OFF_SCREEN.0, OFF_SCREEN.1
                ));
                display = virtual_display::start().await;
                if let Some(display) = &display {
                    display.use_for(&mut command);
                }
            }
            // An app window: no tabs or address bar. `--app=about:blank`
            // gets an ordinary browser window instead.
            command.arg("--app=data:text/html,");
        }
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    containment::die_with_parent(&mut command);
    let mut child = command.spawn().map_err(|source| EngineError::Spawn {
        path: executable.to_path_buf(),
        source,
    })?;
    containment::tie_to_this_process(&child);
    let stderr_tail = child.stderr.take().map(last_line_of);

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
            // What it said last, e.g. the dynamic loader naming a missing
            // shared library.
            let tail = match stderr_tail {
                Some(reader) => tokio::time::timeout(Duration::from_secs(1), reader)
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .flatten(),
                None => None,
            };
            return Err(EngineError::ExitedDuringStartup(match tail {
                Some(line) => format!("{status}: {line}"),
                None => status.to_string(),
            }));
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
                display,
            });
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Reads the Engine's error output to its end, so Chromium never blocks on
/// a full pipe, and returns its last non-empty line.
fn last_line_of(stderr: tokio::process::ChildStderr) -> tokio::task::JoinHandle<Option<String>> {
    use tokio::io::AsyncBufReadExt;
    tokio::spawn(async move {
        let mut stderr = tokio::io::BufReader::new(stderr);
        let (mut line, mut last) = (Vec::new(), None);
        // Bytes, not text: output that isn't UTF-8 must not stop the reading.
        while stderr
            .read_until(b'\n', &mut line)
            .await
            .is_ok_and(|n| n > 0)
        {
            let text = String::from_utf8_lossy(&line);
            if !text.trim().is_empty() {
                last = Some(text.trim().to_string());
            }
            line.clear();
        }
        last
    })
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
    sandbox: bool,
    user_data_dir: &Path,
    url: &str,
) -> Result<tokio::process::Child, EngineError> {
    tokio::fs::create_dir_all(user_data_dir)
        .await
        .map_err(io_err(user_data_dir))?;
    let mut command = tokio::process::Command::new(executable);
    command
        .arg(format!("--user-data-dir={}", user_data_dir.display()))
        .args(QUIET_START)
        .arg(format!("--app={url}"))
        .args((!sandbox).then_some("--no-sandbox"))
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

/// The native handle of the Engine's top-level window (an `HWND`), found by
/// the process that owns it; `None` for a headless Engine or one whose
/// window isn't up.
#[cfg(windows)]
pub fn window_handle(pid: u32) -> Option<u64> {
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GW_OWNER, GetWindow, GetWindowThreadProcessId, IsWindowVisible,
    };
    use windows::core::BOOL;

    struct Search {
        pid: u32,
        found: Option<u64>,
    }
    unsafe extern "system" fn visit(window: HWND, search: LPARAM) -> BOOL {
        let search = unsafe { &mut *(search.0 as *mut Search) };
        let mut owner_pid = 0;
        unsafe { GetWindowThreadProcessId(window, Some(&mut owner_pid)) };
        let top_level = unsafe { GetWindow(window, GW_OWNER) }.is_err();
        if owner_pid == search.pid && top_level && unsafe { IsWindowVisible(window) }.as_bool() {
            search.found = Some(window.0 as u64);
            return false.into();
        }
        true.into()
    }
    let mut search = Search { pid, found: None };
    // Stopping the enumeration early reports an error; the result is in `search`.
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut search as *mut Search as isize)) };
    search.found
}

/// Embedding the Engine window is Windows-only (ADR-0006).
#[cfg(not(windows))]
pub fn window_handle(_pid: u32) -> Option<u64> {
    None
}

/// Minimizes or restores an Engine window through the OS, for Engines whose
/// protocol can't (no `Browser` domain, e.g. M47). Neither
/// takes focus from the developer's window. False if it couldn't.
#[cfg(windows)]
pub fn set_window_minimized(window: u64, minimized: bool) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, ShowWindow,
    };
    let command = if minimized {
        SW_SHOWMINNOACTIVE
    } else {
        SW_SHOWNOACTIVATE
    };
    // The return value is the previous visibility, not success.
    let _ = unsafe { ShowWindow(HWND(window as *mut _), command) };
    true
}

/// Minimizes or restores the X11 window of the Engine with process id `pid`
/// on X display `display` (the one this process uses when `None`), for
/// Engines whose protocol can't (M47 on Linux is X11-only, under XWayland on
/// Wayland desktops). Neither takes focus from the developer's window. Blocks
/// while the window manager restores the window. False if it couldn't.
#[cfg(unix)]
pub fn set_x11_window_minimized(display: Option<u32>, pid: u32, minimized: bool) -> bool {
    match x11_window::set_minimized(display, pid, minimized) {
        Ok(done) => done,
        Err(error) => {
            tracing::warn!(%error, "could not minimize or restore the Engine's X11 window");
            false
        }
    }
}

/// An Engine window on X11. Chromium of M47's age reports its page hidden
/// only when `_NET_WM_STATE` gains `_NET_WM_STATE_HIDDEN`, which a window
/// manager may set when it iconifies the window; tizen-sim sets it itself,
/// and without a window manager (Xvfb) unmaps the window in its place.
#[cfg(unix)]
mod x11_window {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        Atom, AtomEnum, ClientMessageEvent, ConnectionExt, EventMask, MapState, PropMode, Window,
    };
    use x11rb::rust_connection::RustConnection;
    use x11rb::wrapper::ConnectionExt as _;

    type Error = Box<dyn std::error::Error>;

    /// ICCCM's `IconicState`.
    const ICONIC_STATE: u32 = 3;
    /// EWMH's source indication for a request from a pager.
    const FROM_PAGER: u32 = 2;

    struct Atoms {
        net_wm_pid: Atom,
        net_wm_state: Atom,
        net_wm_state_hidden: Atom,
        net_wm_user_time: Atom,
        net_active_window: Atom,
        net_supporting_wm_check: Atom,
        wm_change_state: Atom,
    }

    impl Atoms {
        fn intern(conn: &RustConnection) -> Result<Atoms, Error> {
            let atom = |name: &str| -> Result<Atom, Error> {
                Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
            };
            Ok(Atoms {
                net_wm_pid: atom("_NET_WM_PID")?,
                net_wm_state: atom("_NET_WM_STATE")?,
                net_wm_state_hidden: atom("_NET_WM_STATE_HIDDEN")?,
                net_wm_user_time: atom("_NET_WM_USER_TIME")?,
                net_active_window: atom("_NET_ACTIVE_WINDOW")?,
                net_supporting_wm_check: atom("_NET_SUPPORTING_WM_CHECK")?,
                wm_change_state: atom("WM_CHANGE_STATE")?,
            })
        }
    }

    pub fn set_minimized(display: Option<u32>, pid: u32, minimized: bool) -> Result<bool, Error> {
        let name = display.map(|number| format!(":{number}"));
        let (conn, screen) = x11rb::connect(name.as_deref())?;
        let root = conn.setup().roots[screen].root;
        let atoms = Atoms::intern(&conn)?;
        let Some(window) = find_window(&conn, &atoms, root, pid)? else {
            return Ok(false);
        };
        let window_manager = window_manager_running(&conn, &atoms, root)?;
        // Not every window manager marks an iconified window hidden (Weston's,
        // under WSLg, doesn't), so tizen-sim marks it either way.
        if minimized {
            set_hidden_state(&conn, &atoms, window, true)?;
            if window_manager {
                // ICCCM 4.1.4: ask the window manager to iconify it.
                send_to_window_manager(&conn, root, window, atoms.wm_change_state, ICONIC_STATE)?;
            } else {
                conn.unmap_window(window)?;
            }
        } else {
            let focused = if window_manager {
                // A user time of 0 asks not to focus it (EWMH
                // _NET_WM_USER_TIME).
                conn.change_property32(
                    PropMode::REPLACE,
                    window,
                    atoms.net_wm_user_time,
                    AtomEnum::CARDINAL,
                    &[0],
                )?;
                active_window(&conn, &atoms, root)?
            } else {
                None
            };
            // Mapping an iconic window restores it (ICCCM 4.1.4).
            conn.map_window(window)?;
            set_hidden_state(&conn, &atoms, window, false)?;
            conn.sync()?;
            give_focus_back(&conn, &atoms, root, window, focused)?;
        }
        conn.sync()?;
        Ok(true)
    }

    /// Some window managers (Openbox) focus a window they restore; if
    /// this one does, focus goes back to the window that had it, as
    /// a pager would ask (EWMH _NET_ACTIVE_WINDOW).
    fn give_focus_back(
        conn: &RustConnection,
        atoms: &Atoms,
        root: Window,
        window: Window,
        focused: Option<Window>,
    ) -> Result<(), Error> {
        let Some(focused) = focused.filter(|&focused| focused != window) else {
            return Ok(());
        };
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            if active_window(conn, atoms, root)? == Some(window) {
                send_to_window_manager(conn, root, focused, atoms.net_active_window, FROM_PAGER)?;
                return Ok(());
            }
        }
        Ok(())
    }

    /// Whether an EWMH window manager runs: its check window names itself
    /// in `_NET_SUPPORTING_WM_CHECK`, so a dead one's leftover doesn't count.
    fn window_manager_running(
        conn: &RustConnection,
        atoms: &Atoms,
        root: Window,
    ) -> Result<bool, Error> {
        let check = |window| {
            property32(
                conn,
                window,
                atoms.net_supporting_wm_check,
                AtomEnum::WINDOW.into(),
            )
            .map(|value| value.first().copied())
        };
        Ok(match check(root)? {
            Some(window) => check(window)? == Some(window),
            None => false,
        })
    }

    /// The window the window manager has focused, if any.
    fn active_window(
        conn: &RustConnection,
        atoms: &Atoms,
        root: Window,
    ) -> Result<Option<Window>, Error> {
        let active = property32(conn, root, atoms.net_active_window, AtomEnum::WINDOW.into())?;
        Ok(active.first().copied().filter(|&window| window != 0))
    }

    /// Sends a client message about `window` to the window manager.
    fn send_to_window_manager(
        conn: &RustConnection,
        root: Window,
        window: Window,
        message: Atom,
        data: u32,
    ) -> Result<(), Error> {
        let event = ClientMessageEvent::new(32, window, message, [data, 0, 0, 0, 0]);
        conn.send_event(
            false,
            root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            event,
        )?;
        Ok(())
    }

    /// Adds `_NET_WM_STATE_HIDDEN` to the window's `_NET_WM_STATE`, or
    /// removes it, as a window manager would.
    fn set_hidden_state(
        conn: &RustConnection,
        atoms: &Atoms,
        window: Window,
        hidden: bool,
    ) -> Result<(), Error> {
        // The window manager writes it too; none of its writes may land
        // between this read and write.
        conn.grab_server()?;
        let result = (|| {
            let mut state = property32(conn, window, atoms.net_wm_state, AtomEnum::ATOM.into())?;
            state.retain(|&atom| atom != atoms.net_wm_state_hidden);
            if hidden {
                state.push(atoms.net_wm_state_hidden);
            }
            conn.change_property32(
                PropMode::REPLACE,
                window,
                atoms.net_wm_state,
                AtomEnum::ATOM,
                &state,
            )?;
            Ok(())
        })();
        conn.ungrab_server()?;
        result
    }

    /// The Engine's top-level window: the one it marks with its process id
    /// that is shown or minimized. A window manager may have reparented it
    /// into a frame, so the whole tree is searched.
    fn find_window(
        conn: &RustConnection,
        atoms: &Atoms,
        root: Window,
        pid: u32,
    ) -> Result<Option<Window>, Error> {
        let mut level = vec![root];
        while !level.is_empty() {
            let mut next = Vec::new();
            for window in level {
                let Ok(tree) = conn.query_tree(window)?.reply() else {
                    // Gone since its parent was listed.
                    continue;
                };
                next.extend(tree.children);
            }
            for &window in &next {
                if property32(conn, window, atoms.net_wm_pid, AtomEnum::CARDINAL.into())?.first()
                    != Some(&pid)
                {
                    continue;
                }
                let Ok(attributes) = conn.get_window_attributes(window)?.reply() else {
                    continue;
                };
                let hidden = property32(conn, window, atoms.net_wm_state, AtomEnum::ATOM.into())?
                    .contains(&atoms.net_wm_state_hidden);
                if !attributes.override_redirect
                    && (attributes.map_state == MapState::VIEWABLE || hidden)
                {
                    return Ok(Some(window));
                }
            }
            level = next;
        }
        Ok(None)
    }

    /// A window's 32-bit property, empty when it has none (or is gone).
    fn property32(
        conn: &RustConnection,
        window: Window,
        property: Atom,
        kind: Atom,
    ) -> Result<Vec<u32>, Error> {
        let Ok(reply) = conn
            .get_property(false, window, property, kind, 0, 1024)?
            .reply()
        else {
            return Ok(Vec::new());
        };
        Ok(reply.value32().map(Iterator::collect).unwrap_or_default())
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

/// A private X display for an off-screen Engine on Linux. Wayland
/// compositors (and XWayland under them) ignore the position a window asks
/// for, so an "off-screen" window would show on the developer's desktop;
/// on its own virtual display it can't. Without Xvfb installed, the window
/// is placed off the screen as elsewhere, which works on an X11 desktop.
mod virtual_display {
    #[cfg(target_os = "linux")]
    pub struct VirtualDisplay {
        number: u32,
        _server: tokio::process::Child,
        /// Kept open: Xvfb would die of SIGPIPE writing to a closed pipe.
        _stdout: tokio::io::BufReader<tokio::process::ChildStdout>,
    }

    #[cfg(target_os = "linux")]
    impl VirtualDisplay {
        pub fn use_for(&self, command: &mut tokio::process::Command) {
            command
                .env("DISPLAY", format!(":{}", self.number))
                .env_remove("WAYLAND_DISPLAY");
        }

        pub fn number(&self) -> u32 {
            self.number
        }
    }

    #[cfg(target_os = "linux")]
    pub async fn start() -> Option<VirtualDisplay> {
        use std::process::Stdio;
        use tokio::io::AsyncBufReadExt;

        // Xvfb picks a free display and writes its number to `-displayfd`.
        let mut command = tokio::process::Command::new("Xvfb");
        command
            .args([
                "-displayfd",
                "1",
                "-screen",
                "0",
                "1920x1080x24",
                "-nolisten",
                "tcp",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        super::containment::die_with_parent(&mut command);
        let mut server = match command.spawn() {
            Ok(server) => server,
            Err(error) => {
                tracing::warn!(
                    %error,
                    "Xvfb is not available; the off-screen Engine window may show on the desktop"
                );
                return None;
            }
        };
        let mut line = String::new();
        let mut stdout = tokio::io::BufReader::new(server.stdout.take()?);
        let read = stdout.read_line(&mut line);
        let number = match tokio::time::timeout(std::time::Duration::from_secs(10), read).await {
            Ok(Ok(_)) => line.trim().parse().ok(),
            _ => None,
        };
        let Some(number) = number else {
            tracing::warn!(
                "Xvfb did not start; the off-screen Engine window may show on the desktop"
            );
            return None;
        };
        Some(VirtualDisplay {
            number,
            _server: server,
            _stdout: stdout,
        })
    }

    #[cfg(not(target_os = "linux"))]
    pub enum VirtualDisplay {}

    #[cfg(not(target_os = "linux"))]
    impl VirtualDisplay {
        pub fn use_for(&self, _command: &mut tokio::process::Command) {
            match *self {}
        }

        pub fn number(&self) -> u32 {
            match *self {}
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub async fn start() -> Option<VirtualDisplay> {
        None
    }
}

fn free_port() -> std::io::Result<u16> {
    Ok(std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port())
}
