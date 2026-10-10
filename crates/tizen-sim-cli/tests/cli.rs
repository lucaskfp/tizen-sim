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
