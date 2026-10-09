use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use std::io::Write;
use std::path::PathBuf;
use tizen_sim_core::app::AppSource;
use tizen_sim_core::engine::DownloadProgress;
use tizen_sim_core::home::Home;
use tizen_sim_core::session::{ExitReason, Lifecycle, Session, SessionConfig, SessionEvent};

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
    Run {
        /// A Project Folder containing config.xml.
        app: PathBuf,
        /// Version Profile to use (defaults to the latest).
        #[arg(long)]
        profile: Option<String>,
        /// Run the Engine without a window.
        #[arg(long)]
        headless: bool,
        /// Lenient Mode: allow calls a TV would refuse, reporting each as a Diagnostic.
        #[arg(long)]
        lenient: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    match Cli::parse().command {
        Command::Run {
            app,
            profile,
            headless,
            lenient,
        } => run(app, profile, headless, lenient).await,
    }
}

async fn run(app: PathBuf, profile: Option<String>, headless: bool, lenient: bool) -> Result<()> {
    let session = Session::start(SessionConfig {
        source: AppSource::ProjectFolder(app),
        profile,
        headless,
        lenient,
        home: Home::from_env(),
    });
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
                    SessionEvent::Lifecycle(lifecycle) => match lifecycle {
                        Lifecycle::Starting { app_id, profile } => {
                            println!("Starting {app_id} on Tizen {profile}");
                        }
                        Lifecycle::EngineReady { engine_version, debugger_url } => {
                            println!("Engine ready: {engine_version}");
                            println!("  DevTools endpoint: {debugger_url}");
                        }
                        Lifecycle::AppLoaded { url } => println!("App loaded: {url} (Ctrl+C to stop)"),
                        Lifecycle::Failed { error } => bail!(error),
                        Lifecycle::Exited { reason } => {
                            match reason {
                                ExitReason::Stopped => println!("Stopped."),
                                ExitReason::EngineClosed => println!("The Engine window was closed."),
                                ExitReason::AppExited => println!("The App exited."),
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
