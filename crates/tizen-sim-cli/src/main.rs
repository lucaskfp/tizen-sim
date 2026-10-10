use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::io::Write;
use std::path::PathBuf;
use tizen_sim_core::app::AppSource;
use tizen_sim_core::engine::{self, DownloadProgress, EngineSpec};
use tizen_sim_core::home::Home;
use tizen_sim_core::network::{NetworkRequest, Verdict};
use tizen_sim_core::profile::VersionProfile;
use tizen_sim_core::remote_keys::{KeyMap, PhysicalKey};
use tizen_sim_core::screen::AppResolution;
use tizen_sim_core::session::{
    ExitReason, HotReload, KeyOutcome, Lifecycle, Session, SessionConfig, SessionEvent,
};

/// A fast simulator for Tizen TV web apps.
#[derive(Parser)]
#[command(name = "tizen-sim", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run an App and stream its console until Ctrl+C.
    Run(RunArgs),
    /// List, fetch and remove cached Engines.
    #[command(subcommand)]
    Engines(EnginesCommand),
    /// List the Version Profiles.
    Profiles,
}

#[derive(Subcommand)]
enum EnginesCommand {
    /// List the cached Engines and the disk space they use.
    List,
    /// Download the Engine for a Version Profile ahead of time.
    Fetch {
        /// The Version Profile (defaults to the latest).
        profile: Option<String>,
    },
    /// Remove a Version Profile's cached Engine to free its disk space.
    Remove {
        /// The Version Profile whose Engine to remove.
        #[arg(required_unless_present = "all")]
        profile: Option<String>,
        /// Remove every cached Engine.
        #[arg(long, conflicts_with = "profile")]
        all: bool,
    },
}

#[derive(Args)]
struct RunArgs {
    /// A Project Folder containing config.xml, a Package (.wgt file), or the
    /// http:// URL of your bundler's dev server (with --config).
    app: String,
    /// The App's config.xml, for a dev server URL. Changing it restarts the App.
    #[arg(long, value_name = "CONFIG_XML")]
    config: Option<PathBuf>,
    /// Version Profile to use (defaults to the last one used for this App,
    /// or the latest for an App never run before).
    #[arg(long)]
    profile: Option<String>,
    /// Run this Chromium executable instead of downloading the Version
    /// Profile's Engine, e.g. to work offline or try a custom build.
    #[arg(long, value_name = "CHROMIUM")]
    engine_path: Option<PathBuf>,
    /// Run the Engine without a window.
    #[arg(long)]
    headless: bool,
    /// Lenient Mode: allow calls a TV would refuse, reporting each as a Diagnostic.
    #[arg(long)]
    lenient: bool,
    /// Map a keyboard key to a Remote Key, e.g. `--map-key F5=MediaPlayPause`.
    /// Adds to (or overrides) the defaults: Backspace/Escape=Back,
    /// F1-F4=colour keys, PageUp/PageDown=ChannelUp/ChannelDown.
    #[arg(long = "map-key", value_name = "KEY=REMOTE_KEY")]
    map_keys: Vec<String>,
    /// Override a device value the platform APIs report, e.g.
    /// `--device duid=ABC123` or `--device networkType=WIFI`. JSON values
    /// (numbers, booleans) are taken as such; anything else is a string.
    #[arg(long = "device", value_name = "NAME=VALUE")]
    device: Vec<String>,
    /// The TV screen size the App is laid out at: 1920x1080 as on UHD TVs,
    /// or 1280x720 as on FHD TVs.
    #[arg(long, value_enum, default_value = "1920x1080")]
    resolution: Resolution,
    /// Show a checkerboard where the page is transparent, instead of the
    /// black a TV shows there.
    #[arg(long)]
    checkerboard: bool,
    /// Open DevTools for the App once it has loaded.
    #[arg(long)]
    devtools: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum Resolution {
    #[value(name = "1920x1080")]
    Uhd,
    #[value(name = "1280x720")]
    Fhd,
}

impl RunArgs {
    fn into_config(self) -> Result<SessionConfig> {
        let mut key_map = KeyMap::default();
        for mapping in &self.map_keys {
            let (physical, remote) = mapping
                .split_once('=')
                .with_context(|| format!("--map-key {mapping}: expected KEY=REMOTE_KEY"))?;
            let physical = PhysicalKey::parse(physical.trim())
                .with_context(|| format!("--map-key {mapping}"))?;
            key_map
                .insert(physical, remote.trim())
                .with_context(|| format!("--map-key {mapping}"))?;
        }
        let mut device = serde_json::Map::new();
        for setting in &self.device {
            let (name, value) = setting
                .split_once('=')
                .with_context(|| format!("--device {setting}: expected NAME=VALUE"))?;
            let value = serde_json::from_str(value.trim())
                .unwrap_or_else(|_| serde_json::Value::String(value.trim().to_string()));
            device.insert(name.trim().to_string(), value);
        }
        let is_url = self.app.starts_with("http://") || self.app.starts_with("https://");
        let source = match (is_url, self.config) {
            (true, Some(config)) => AppSource::DevServerUrl {
                url: self
                    .app
                    .parse()
                    .with_context(|| format!("invalid dev server URL {}", self.app))?,
                config,
            },
            (true, None) => {
                bail!("a dev server URL needs the App's config.xml: --config <CONFIG_XML>")
            }
            (false, Some(_)) => {
                bail!(
                    "--config only applies to a dev server URL; a Project Folder or Package has its own"
                )
            }
            (false, None) => AppSource::from_path(PathBuf::from(self.app)),
        };
        Ok(SessionConfig {
            source,
            profile: self.profile,
            engine_path: self.engine_path,
            headless: self.headless,
            lenient: self.lenient,
            key_map,
            device,
            resolution: match self.resolution {
                Resolution::Uhd => AppResolution::Uhd,
                Resolution::Fhd => AppResolution::Fhd,
            },
            checkerboard: self.checkerboard,
            home: Home::from_env(),
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    match Cli::parse().command {
        Command::Run(args) => {
            let devtools = args.devtools;
            run(args.into_config()?, devtools).await
        }
        Command::Engines(command) => engines(command, &Home::from_env()).await,
        Command::Profiles => {
            profiles(&Home::from_env());
            Ok(())
        }
    }
}

fn profiles(home: &Home) {
    let cached = home.engine_store().list().unwrap_or_default();
    let latest = &VersionProfile::latest().version;
    for profile in VersionProfile::all() {
        let mut notes = vec![];
        if &profile.version == latest {
            notes.push("latest");
        }
        if cached
            .iter()
            .any(|engine| engine.profile == profile.version)
        {
            notes.push("Engine cached");
        }
        println!(
            "{:<6} Chromium M{:<4} {}",
            profile.version,
            profile.milestone,
            notes.join(", ")
        );
    }
}

async fn engines(command: EnginesCommand, home: &Home) -> Result<()> {
    let store = home.engine_store();
    match command {
        EnginesCommand::List => {
            let cached = store.list()?;
            if cached.is_empty() {
                println!("No Engines cached in {}", home.engines_dir().display());
            }
            for engine in cached {
                println!(
                    "{:<6} {:<12} {:>5} MB  {}",
                    engine.profile,
                    engine.platform,
                    megabytes(engine.size),
                    engine.dir.display()
                );
            }
        }
        EnginesCommand::Fetch { profile } => {
            let profile = match profile {
                Some(version) => VersionProfile::find(&version)?,
                None => VersionProfile::latest(),
            };
            let spec = EngineSpec::for_profile(&profile.version, engine::current_platform()?)?;
            let installed = store.ensure(spec, |p| print_progress(&p)).await?;
            let state = if installed.freshly_downloaded {
                "downloaded"
            } else {
                "already cached"
            };
            println!(
                "Engine for Tizen {} {state}: {}",
                profile.version,
                installed.executable.display()
            );
        }
        EnginesCommand::Remove { profile, all } => {
            let profiles: Vec<String> = match profile {
                Some(profile) => vec![profile],
                None => {
                    debug_assert!(all);
                    let mut cached: Vec<String> =
                        store.list()?.into_iter().map(|e| e.profile).collect();
                    cached.dedup();
                    cached
                }
            };
            if profiles.is_empty() {
                println!("No Engines cached.");
            }
            for profile in profiles {
                let removed = store.remove(&profile).with_context(|| {
                    format!("removing the Engine for Tizen {profile} (is a Session running on it?)")
                })?;
                if removed.is_empty() {
                    println!("No cached Engine for Tizen {profile}.");
                }
                for engine in removed {
                    println!(
                        "Removed the Engine for Tizen {} ({}), freeing {} MB.",
                        engine.profile,
                        engine.platform,
                        megabytes(engine.size)
                    );
                }
            }
        }
    }
    Ok(())
}

fn megabytes(bytes: u64) -> u64 {
    bytes.div_ceil(1024 * 1024)
}

async fn run(config: SessionConfig, open_devtools: bool) -> Result<()> {
    let changed = match config.source {
        AppSource::ProjectFolder(_) | AppSource::DevServerUrl { .. } => "config.xml changed",
        AppSource::Package(_) => "The Package changed and was unpacked again",
    };
    let session = Session::start(config);
    let mut events = session.events();
    let mut stopping = false;

    loop {
        tokio::select! {
            event = events.next() => {
                let Some(event) = event else { return Ok(()) };
                match event {
                    SessionEvent::EngineDownload(progress) => print_progress(&progress),
                    SessionEvent::Console(message) => println!("[console.{}] {}", message.level, message.text),
                    SessionEvent::Diagnostic(diagnostic) => println!("[diagnostic:{}] {}", diagnostic.code, diagnostic.message),
                    SessionEvent::RemoteKey { key, outcome: KeyOutcome::Dropped { reason } } => {
                        println!("[key dropped] {key}: {reason}");
                    }
                    SessionEvent::RemoteKey { .. } => {}
                    SessionEvent::Network(request) => print_network(&request),
                    SessionEvent::HotReload(hot_reload) => match hot_reload {
                        HotReload::FilesChanged { paths } => println!("[changed] {}", paths.join(", ")),
                        HotReload::StylesSwapped => println!("Stylesheets swapped in place."),
                        HotReload::Reloaded => println!("App reloaded."),
                        HotReload::Restarted { app_id } => println!("{changed}: restarted {app_id}."),
                        HotReload::Failed { error } => println!("[hot reload failed] {error}"),
                    },
                    SessionEvent::Lifecycle(lifecycle) => match lifecycle {
                        Lifecycle::Starting { app_id, profile } => {
                            println!("Starting {app_id} on Tizen {profile}");
                        }
                        Lifecycle::EngineReady { engine_version, debugger_url, devtools_url } => {
                            println!("Engine ready: {engine_version}");
                            println!("  DevTools endpoint: {debugger_url}");
                            println!("  DevTools: {devtools_url}");
                        }
                        Lifecycle::AppLoaded { url } => {
                            println!("App loaded: {url} (Ctrl+C to stop)");
                            if open_devtools
                                && let Err(error) = session.open_devtools().await
                            {
                                println!("[devtools] {error}");
                            }
                        }
                        Lifecycle::Hidden => println!("The App is hidden (in the background)."),
                        Lifecycle::Shown => println!("The App is back in the foreground."),
                        Lifecycle::Failed { error } => bail!(error),
                        Lifecycle::Exited { reason } => {
                            match reason {
                                ExitReason::Stopped => println!("Stopped."),
                                ExitReason::EngineClosed => println!("The Engine window was closed."),
                                ExitReason::AppExited => println!("The App exited."),
                                ExitReason::TerminatedByRemote => println!("Exit was pressed or Back held: the App was terminated, as on a TV."),
                                ExitReason::FailedToStart => {}
                            }
                            return Ok(());
                        }
                    },
                }
            }
            _ = tokio::signal::ctrl_c(), if !stopping => {
                stopping = true;
                println!("Stopping...");
                session.stop().await;
            }
        }
    }
}

fn print_network(request: &NetworkRequest) {
    let NetworkRequest {
        method,
        url,
        status,
        duration,
        verdict,
    } = request;
    let ms = duration.as_millis();
    match (verdict, status) {
        (Verdict::Blocked { .. }, _) => println!("[network blocked] {method} {url}"),
        (Verdict::Allowed, Some(status)) => println!("[network] {method} {url} {status} {ms}ms"),
        (Verdict::Allowed, None) => println!("[network] {method} {url} {ms}ms"),
    }
}

fn print_progress(progress: &DownloadProgress) {
    const MB: f64 = 1024.0 * 1024.0;
    let received = progress.received as f64 / MB;
    match progress.total {
        Some(total) if total > 0 => {
            let pct = progress.received * 100 / total;
            print!(
                "\rDownloading Engine: {pct:>3}% ({received:.0}/{:.0} MB)",
                total as f64 / MB
            );
            if progress.received >= total {
                println!();
            }
        }
        _ => print!("\rDownloading Engine: {received:.0} MB"),
    }
    let _ = std::io::stdout().flush();
}
