use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use std::io::Write;
use std::path::PathBuf;
use tizen_sim_core::app::AppSource;
use tizen_sim_core::engine::DownloadProgress;
use tizen_sim_core::home::Home;
use tizen_sim_core::remote_keys::{KeyMap, PhysicalKey};
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
}

#[derive(Args)]
struct RunArgs {
    /// A Project Folder containing config.xml, a Package (.wgt file), or the
    /// http:// URL of your bundler's dev server (with --config).
    app: String,
    /// The App's config.xml, for a dev server URL. Changing it restarts the App.
    #[arg(long, value_name = "CONFIG_XML")]
    config: Option<PathBuf>,
    /// Version Profile to use (defaults to the latest).
    #[arg(long)]
    profile: Option<String>,
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
            headless: self.headless,
            lenient: self.lenient,
            key_map,
            device,
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
        Command::Run(args) => run(args.into_config()?).await,
    }
}

async fn run(config: SessionConfig) -> Result<()> {
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
                        Lifecycle::EngineReady { engine_version, debugger_url } => {
                            println!("Engine ready: {engine_version}");
                            println!("  DevTools endpoint: {debugger_url}");
                        }
                        Lifecycle::AppLoaded { url } => println!("App loaded: {url} (Ctrl+C to stop)"),
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
