//! CLI smoke tests: drive the real `tizen-sim` binary.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn fixture(name: &str) -> String {
    format!("{}/../../fixtures/apps/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Runs of the same App share an Engine profile, so these tests take turns.
fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn spawn_run(app: &str) -> Child {
    spawn_run_path(&fixture(app))
}

fn spawn_run_path(app: &str) -> Child {
    Command::new(env!("CARGO_BIN_EXE_tizen-sim"))
        .args(["run", app, "--headless"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap()
}

/// Reads stdout until the App has loaded, returning everything printed and
/// the Engine's debugging address.
fn read_until_loaded(child: &mut Child) -> (Vec<String>, String) {
    let stdout = BufReader::new(child.stdout.take().unwrap());
    let mut lines = vec![];
    let mut debug_addr = None;
    for line in stdout.lines() {
        let line = line.unwrap();
        if let Some(url) = line.trim().strip_prefix("DevTools endpoint: ws://") {
            debug_addr = url.split('/').next().map(String::from);
        }
        let loaded = line.starts_with("App loaded:");
        lines.push(line);
        if loaded {
            break;
        }
    }
    let addr = debug_addr.unwrap_or_else(|| panic!("no DevTools endpoint printed: {lines:#?}"));
    (lines, addr)
}

#[test]
fn run_prints_lifecycle_and_console() {
    let _turn = one_at_a_time();
    let mut child = spawn_run("hello");
    let (lines, _) = read_until_loaded(&mut child);
    child.kill().unwrap();
    let _ = child.wait();

    let all = lines.join("\n");
    assert!(
        all.contains("Starting Fixture001.Hello on Tizen 10.0"),
        "{all}"
    );
    assert!(all.contains("Engine ready: Chrome/130."), "{all}");
    assert!(
        all.contains("DevTools: http://127.0.0.1:") && all.contains("/devtools/inspector.html?ws="),
        "{all}"
    );
    assert!(all.contains("[console.log] hello from fixture"), "{all}");
}

#[test]
fn killing_the_cli_takes_the_engine_down_with_it() {
    let _turn = one_at_a_time();
    let mut child = spawn_run("hello");
    let (_, debug_addr) = read_until_loaded(&mut child);

    // Not a graceful Ctrl+C: the CLI gets no chance to clean up.
    child.kill().unwrap();
    let _ = child.wait();

    let deadline = Instant::now() + Duration::from_secs(10);
    while std::net::TcpStream::connect(&debug_addr).is_ok() {
        assert!(
            Instant::now() < deadline,
            "the Engine outlived the CLI ({debug_addr} still open)"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn invalid_app_fails_with_the_problem_named() {
    let output = Command::new(env!("CARGO_BIN_EXE_tizen-sim"))
        .args(["run", &fixture("malformed-config"), "--headless"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("config.xml") && stderr.contains("line"),
        "{stderr}"
    );
}

/// Packs a fixture App as `app.wgt` in a throwaway directory.
fn package_fixture(name: &str) -> (tempfile::TempDir, String) {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let wgt = dir.path().join("app.wgt");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&wgt).unwrap());
    for entry in std::fs::read_dir(fixture(name)).unwrap() {
        let entry = entry.unwrap();
        zip.start_file(
            entry.file_name().to_str().unwrap(),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(&std::fs::read(entry.path()).unwrap())
            .unwrap();
    }
    zip.finish().unwrap();
    let path = wgt.to_str().unwrap().to_string();
    (dir, path)
}

#[test]
fn run_opens_a_package() {
    let _turn = one_at_a_time();
    let (_dir, wgt) = package_fixture("hello");
    let mut child = spawn_run_path(&wgt);
    let (lines, _) = read_until_loaded(&mut child);
    child.kill().unwrap();
    let _ = child.wait();

    let all = lines.join("\n");
    assert!(
        all.contains("Starting Fixture001.Hello on Tizen 10.0"),
        "{all}"
    );
    assert!(all.contains("[console.log] hello from fixture"), "{all}");
}

#[test]
fn a_corrupt_package_fails_with_the_problem_named() {
    let dir = tempfile::tempdir().unwrap();
    let wgt = dir.path().join("broken.wgt");
    std::fs::write(&wgt, b"not a zip archive").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_tizen-sim"))
        .args(["run", wgt.to_str().unwrap(), "--headless"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("broken.wgt") && stderr.contains("not a valid Package"),
        "{stderr}"
    );
}

#[test]
fn map_key_rejects_unknown_keys() {
    for (mapping, expected) in [
        ("F5=NotAKey", "unknown Remote Key `NotAKey`"),
        ("Hyper=Back", "unknown keyboard key `Hyper`"),
        ("F5", "expected KEY=REMOTE_KEY"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_tizen-sim"))
            .args(["run", &fixture("hello"), "--headless", "--map-key", mapping])
            .output()
            .unwrap();
        assert!(!output.status.success(), "{mapping} was accepted");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(expected), "{mapping}: {stderr}");
    }
}

#[test]
fn config_goes_with_a_dev_server_url_and_only_with_one() {
    let config = format!("{}/config.xml", fixture("dev-server"));
    for (args, expected) in [
        (vec!["http://localhost:5173/"], "--config <CONFIG_XML>"),
        (
            vec![&fixture("hello") as &str, "--config", &config],
            "--config only applies to a dev server URL",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_tizen-sim"))
            .arg("run")
            .args(&args)
            .arg("--headless")
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?} was accepted");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(expected), "{args:?}: {stderr}");
    }
}

/// Runs `tizen-sim` with its per-user files in `home`.
fn tizen_sim_in(home: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_tizen-sim"))
        .env("TIZEN_SIM_HOME", home)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn profiles_lists_the_version_profiles() {
    let home = tempfile::tempdir().unwrap();
    let output = tizen_sim_in(home.path(), &["profiles"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .find(|l| l.starts_with("10.0"))
        .unwrap_or_else(|| panic!("10.0 not listed: {stdout}"));
    assert!(line.contains("M130") && line.contains("latest"), "{line}");
}

#[test]
fn engines_list_and_remove_on_an_empty_cache() {
    let home = tempfile::tempdir().unwrap();
    let list = tizen_sim_in(home.path(), &["engines", "list"]);
    assert!(list.status.success());
    assert!(
        String::from_utf8_lossy(&list.stdout).contains("No Engines cached"),
        "{list:?}"
    );

    let remove = tizen_sim_in(home.path(), &["engines", "remove", "10.0"]);
    assert!(remove.status.success());
    assert!(
        String::from_utf8_lossy(&remove.stdout).contains("No cached Engine for Tizen 10.0"),
        "{remove:?}"
    );
}

#[test]
fn engines_fetch_rejects_an_unknown_profile() {
    let home = tempfile::tempdir().unwrap();
    let output = tizen_sim_in(home.path(), &["engines", "fetch", "1.0"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown Version Profile `1.0`"), "{stderr}");
}

#[test]
fn engine_path_launches_the_given_binary_without_downloading() {
    let home = tempfile::tempdir().unwrap();
    let missing = home.path().join("my-chromium").join("chrome.exe");
    let output = tizen_sim_in(
        home.path(),
        &[
            "run",
            &fixture("hello"),
            "--headless",
            "--engine-path",
            missing.to_str().unwrap(),
        ],
    );
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("could not start the Engine") && stderr.contains("my-chromium"),
        "{stderr}"
    );
    assert!(!stdout.contains("Downloading Engine"), "{stdout}");
    assert!(!home.path().join("engines").exists());
}

/// The cached Engine for the latest Version Profile, downloaded if need be,
/// so runs with a throwaway home don't download it again.
fn cached_engine() -> String {
    use tizen_sim_core::engine::{EngineSpec, current_platform};
    let spec = EngineSpec::for_profile("10.0", current_platform().unwrap()).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let engine = runtime
        .block_on(
            tizen_sim_core::home::Home::from_env()
                .engine_store()
                .ensure(spec, |_| {}),
        )
        .unwrap();
    engine.executable.to_str().unwrap().to_string()
}

/// `tizen-sim run` with its per-user files in `home`, its output drained in
/// the background so it can keep printing. Returns once the App has loaded.
/// A throwaway home has its own Engine profiles, so tests using one needn't
/// take turns.
fn run_in(home: &std::path::Path, app: &str, engine: &str) -> Child {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tizen-sim"))
        .env("TIZEN_SIM_HOME", home)
        .args(["run", &fixture(app), "--headless", "--engine-path", engine])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let (loaded_tx, loaded) = std::sync::mpsc::channel();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    std::thread::spawn(move || {
        for line in stdout.lines().map_while(Result::ok) {
            if line.starts_with("App loaded:") {
                let _ = loaded_tx.send(());
            }
        }
    });
    loaded
        .recv_timeout(Duration::from_secs(120))
        .expect("the App never loaded");
    child
}

fn stdout_of(output: &std::process::Output) -> String {
    assert!(output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn another_terminal_lists_and_controls_a_running_session() {
    let engine = cached_engine();
    let home = tempfile::tempdir().unwrap();
    let mut owner = run_in(home.path(), "remote-keys", &engine);

    let sessions = stdout_of(&tizen_sim_in(home.path(), &["sessions"]));
    let line = sessions
        .lines()
        .find(|l| l.contains("Fixture006.RemoteKeys"))
        .unwrap_or_else(|| panic!("the Session is not listed: {sessions}"));
    assert!(
        line.contains("Tizen 10.0") && line.contains("running"),
        "{line}"
    );
    assert!(
        line.contains(&owner.id().to_string()),
        "no owner pid: {line}"
    );

    // With one Session running, commands act on it without being told which.
    let key = stdout_of(&tizen_sim_in(home.path(), &["key", "Back"]));
    assert!(key.contains("Back delivered"), "{key}");
    let key = stdout_of(&tizen_sim_in(home.path(), &["key", "ColorF0Red"]));
    assert!(
        key.contains("ColorF0Red dropped") && key.contains("not registered"),
        "{key}"
    );
    let unknown = tizen_sim_in(home.path(), &["key", "NotAKey"]);
    assert!(!unknown.status.success());
    assert!(
        String::from_utf8_lossy(&unknown.stderr).contains("unknown Remote Key `NotAKey`"),
        "{unknown:?}"
    );
    let reload = stdout_of(&tizen_sim_in(home.path(), &["reload"]));
    assert!(reload.contains("Reloaded"), "{reload}");
    let png = home.path().join("shot.png");
    let shot = stdout_of(&tizen_sim_in(
        home.path(),
        &["screenshot", "--output", png.to_str().unwrap()],
    ));
    assert!(shot.contains("shot.png"), "{shot}");
    assert!(
        std::fs::read(&png)
            .unwrap()
            .starts_with(b"\x89PNG\r\n\x1a\n")
    );

    // Killed without a chance to clean up, it leaves no live-looking entry.
    owner.kill().unwrap();
    let _ = owner.wait();
    let sessions = stdout_of(&tizen_sim_in(home.path(), &["sessions"]));
    assert!(sessions.contains("No Sessions running"), "{sessions}");
    let key = tizen_sim_in(home.path(), &["key", "Back"]);
    assert!(!key.status.success());
    assert!(
        String::from_utf8_lossy(&key.stderr).contains("no Session is running"),
        "{key:?}"
    );
}

#[test]
fn with_several_sessions_commands_need_to_be_told_which() {
    let engine = cached_engine();
    let home = tempfile::tempdir().unwrap();
    let mut first = run_in(home.path(), "hello", &engine);
    let mut second = run_in(home.path(), "remote-keys", &engine);

    let key = tizen_sim_in(home.path(), &["key", "Back"]);
    assert!(!key.status.success());
    let stderr = String::from_utf8_lossy(&key.stderr);
    assert!(
        stderr.contains("several Sessions are running")
            && stderr.contains("Fixture001.Hello")
            && stderr.contains("Fixture006.RemoteKeys"),
        "{stderr}"
    );

    let sessions = stdout_of(&tizen_sim_in(home.path(), &["sessions"]));
    let id = sessions
        .lines()
        .find(|l| l.contains("Fixture006.RemoteKeys"))
        .and_then(|l| l.split_whitespace().next())
        .unwrap_or_else(|| panic!("no id listed: {sessions}"))
        .to_string();
    let key = stdout_of(&tizen_sim_in(
        home.path(),
        &["key", "Back", "--session", &id],
    ));
    assert!(key.contains("Back delivered"), "{key}");

    for owner in [&mut first, &mut second] {
        owner.kill().unwrap();
        let _ = owner.wait();
    }
}

#[test]
fn reset_clears_what_the_app_stored() {
    let home = tempfile::tempdir().unwrap();
    let state = home.path().join("state");
    let sandbox = state.join("sandboxes/Fixture013.Filesystem/wgt-private");
    let engine_data = state.join("engine-data/Fixture013.Filesystem/10.0");
    let other = state.join("sandboxes/Fixture001.Hello");
    for dir in [&sandbox, &engine_data, &other] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(sandbox.join("state.json"), "{}").unwrap();

    let reset = tizen_sim_in(home.path(), &["reset", &fixture("filesystem")]);
    assert!(reset.status.success(), "{reset:?}");
    assert!(
        stdout_of(&reset).contains("Fixture013.Filesystem"),
        "{reset:?}"
    );
    assert!(!state.join("sandboxes/Fixture013.Filesystem").exists());
    assert!(!state.join("engine-data/Fixture013.Filesystem").exists());
    assert!(other.exists(), "another App's data was cleared");

    let again = tizen_sim_in(home.path(), &["reset", &fixture("filesystem")]);
    assert!(again.status.success(), "{again:?}");
    assert!(stdout_of(&again).contains("Nothing to reset"), "{again:?}");
}
