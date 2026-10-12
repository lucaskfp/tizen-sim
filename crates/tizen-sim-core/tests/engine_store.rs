//! The Engine cache: download, checksum verification and reuse, served from a
//! local HTTP server so no real Engine is needed.

use sha2::{Digest, Sha256};
use std::io::Write;
use tizen_sim_core::engine::{EngineError, EngineSpec, EngineStore};

fn stand_in_engine_zip() -> Vec<u8> {
    zip_with_executable(b"not really chrome")
}

fn zip_with_executable(executable: &[u8]) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut buf);
    let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
    zip.start_file("stand-in-engine/chrome.exe", opts).unwrap();
    zip.write_all(executable).unwrap();
    zip.finish().unwrap();
    buf.into_inner()
}

/// A 64-bit ELF whose only content is a `.dynstr` holding `dynstr`.
fn stand_in_elf(dynstr: &[u8]) -> Vec<u8> {
    let shstrtab = b"\0.shstrtab\0.dynstr\0";
    let headers = 64 + shstrtab.len() + dynstr.len();
    let mut elf = b"\x7fELF\x02\x01".to_vec();
    elf.resize(64, 0);
    elf[0x28..0x30].copy_from_slice(&(headers as u64).to_le_bytes());
    elf[0x3a..0x40].copy_from_slice(&[64, 0, 3, 0, 1, 0]);
    elf.extend_from_slice(shstrtab);
    elf.extend_from_slice(dynstr);
    for (name, offset, size) in [
        (0u32, 0, 0),
        (1, 64, shstrtab.len()),
        (11, 64 + shstrtab.len(), dynstr.len()),
    ] {
        let mut header = [0u8; 64];
        header[..4].copy_from_slice(&name.to_le_bytes());
        header[0x18..0x20].copy_from_slice(&(offset as u64).to_le_bytes());
        header[0x20..0x28].copy_from_slice(&(size as u64).to_le_bytes());
        elf.extend_from_slice(&header);
    }
    elf
}

async fn serve(bytes: Vec<u8>) -> String {
    let app = axum::Router::new().route(
        "/engine.zip",
        axum::routing::get(move || async move { bytes.clone() }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}/engine.zip")
}

fn spec(url: String, sha256: String) -> EngineSpec {
    EngineSpec {
        profile: "99.0".into(),
        milestone: 999,
        platform: "test".into(),
        url,
        sha256,
        executable: "stand-in-engine/chrome.exe".into(),
        headless: true,
        sandbox: true,
        unexport: Vec::new(),
    }
}

#[tokio::test]
async fn an_engines_listed_exports_are_renamed_on_install() {
    let zip = zip_with_executable(&stand_in_elf(b"\0hb_shape\0ChromeMain\0"));
    let sha = hex::encode(Sha256::digest(&zip));
    let url = serve(zip).await;
    let dir = tempfile::tempdir().unwrap();
    let mut spec = spec(url, sha);
    spec.unexport = vec!["hb_".into()];

    let installed = EngineStore::new(dir.path().to_path_buf())
        .ensure(&spec, |_| {})
        .await
        .unwrap();

    assert_eq!(
        std::fs::read(&installed.executable).unwrap(),
        stand_in_elf(b"\0_b_shape\0ChromeMain\0")
    );
}

#[tokio::test]
async fn exports_listed_for_an_executable_that_is_not_elf_fail_the_install() {
    let zip = stand_in_engine_zip();
    let sha = hex::encode(Sha256::digest(&zip));
    let url = serve(zip).await;
    let dir = tempfile::tempdir().unwrap();
    let mut spec = spec(url, sha);
    spec.unexport = vec!["hb_".into()];

    let error = EngineStore::new(dir.path().to_path_buf())
        .ensure(&spec, |_| {})
        .await
        .unwrap_err();

    assert!(matches!(error, EngineError::Unpack(_)), "{error}");
    assert!(
        EngineStore::new(dir.path().to_path_buf())
            .list()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn downloads_once_then_reuses_the_cache() {
    let zip = stand_in_engine_zip();
    let sha = hex::encode(Sha256::digest(&zip));
    let url = serve(zip).await;
    let dir = tempfile::tempdir().unwrap();
    let store = EngineStore::new(dir.path().to_path_buf());

    let mut progress = 0;
    let first = store
        .ensure(&spec(url.clone(), sha.clone()), |_| progress += 1)
        .await
        .unwrap();
    assert!(first.freshly_downloaded);
    assert!(progress > 0, "no download progress was reported");
    assert_eq!(
        std::fs::read(&first.executable).unwrap(),
        b"not really chrome"
    );

    let second = store
        .ensure(&spec(url, sha), |_| panic!("downloaded again"))
        .await
        .unwrap();
    assert!(!second.freshly_downloaded);
    assert_eq!(second.executable, first.executable);
}

#[tokio::test]
async fn another_process_installing_the_same_engine_first_is_not_an_error() {
    // An Engine installed elsewhere, to copy in as the other process's work.
    let zip = stand_in_engine_zip();
    let sha = hex::encode(Sha256::digest(&zip));
    let elsewhere = tempfile::tempdir().unwrap();
    let url = serve(zip.clone()).await;
    let theirs = EngineStore::new(elsewhere.path().to_path_buf())
        .ensure(&spec(url, sha.clone()), |_| {})
        .await
        .unwrap();
    let install_dir = theirs
        .executable
        .strip_prefix(elsewhere.path())
        .unwrap()
        .components()
        .next()
        .unwrap()
        .as_os_str()
        .to_owned();

    // While this process downloads, another one (a parallel test, or the CLI
    // and the GUI starting at once) finishes installing the same Engine.
    let dir = tempfile::tempdir().unwrap();
    let (from, to) = (
        elsewhere.path().join(&install_dir),
        dir.path().join(&install_dir),
    );
    let app = axum::Router::new().route(
        "/engine.zip",
        axum::routing::get(move || {
            let (zip, from, to) = (zip.clone(), from.clone(), to.clone());
            async move {
                copy_dir(&from, &to);
                zip
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let ours = EngineStore::new(dir.path().to_path_buf())
        .ensure(&spec(format!("http://{addr}/engine.zip"), sha), |_| {})
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(&ours.executable).unwrap(),
        b"not really chrome"
    );
    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(entries, vec![install_dir], "left behind: {entries:?}");
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[tokio::test]
async fn rejects_a_download_with_the_wrong_checksum() {
    let url = serve(stand_in_engine_zip()).await;
    let dir = tempfile::tempdir().unwrap();
    let store = EngineStore::new(dir.path().to_path_buf());

    let err = store
        .ensure(&spec(url, "00".repeat(32)), |_| {})
        .await
        .unwrap_err();
    assert!(
        matches!(err, EngineError::ChecksumMismatch { .. }),
        "{err:?}"
    );
    // Nothing usable is left behind.
    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert!(leftovers.is_empty(), "left behind: {leftovers:?}");
}

#[tokio::test]
async fn an_interrupted_download_leaves_nothing_behind() {
    // A server that sends the start of the archive, then stalls forever.
    let app = axum::Router::new().route(
        "/engine.zip",
        axum::routing::get(|| async {
            let first = futures_util::stream::once(async {
                Ok::<_, std::io::Error>(axum::body::Bytes::from(vec![0u8; 64 * 1024]))
            });
            let stall = futures_util::stream::pending();
            axum::body::Body::from_stream(futures_util::StreamExt::chain(first, stall))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let dir = tempfile::tempdir().unwrap();
    let store = EngineStore::new(dir.path().to_path_buf());
    let spec = spec(format!("http://{addr}/engine.zip"), "00".repeat(32));
    // The caller gives up mid-download, e.g. on Ctrl+C.
    let interrupted = tokio::time::timeout(
        std::time::Duration::from_millis(500),
        store.ensure(&spec, |_| {}),
    )
    .await;
    assert!(interrupted.is_err(), "the download should still be running");

    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert!(leftovers.is_empty(), "left behind: {leftovers:?}");
}

#[tokio::test]
async fn lists_cached_engines_and_removing_one_frees_its_cache() {
    let zip = stand_in_engine_zip();
    let sha = hex::encode(Sha256::digest(&zip));
    let url = serve(zip).await;
    let dir = tempfile::tempdir().unwrap();
    let store = EngineStore::new(dir.path().to_path_buf());
    assert!(store.list().unwrap().is_empty());

    let installed = store.ensure(&spec(url, sha), |_| {}).await.unwrap();
    let cached = store.list().unwrap();
    assert_eq!(cached.len(), 1, "{cached:?}");
    assert_eq!(cached[0].profile, "99.0");
    assert_eq!(cached[0].platform, "test");
    assert!(cached[0].size >= b"not really chrome".len() as u64);
    assert!(installed.executable.starts_with(&cached[0].dir));

    // Another profile's Engine is left alone.
    assert!(store.remove("98.0").unwrap().is_empty());
    assert!(installed.executable.is_file());

    let removed = store.remove("99.0").unwrap();
    assert_eq!(removed.len(), 1);
    assert!(!removed[0].dir.exists());
    assert!(store.list().unwrap().is_empty());
    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert!(leftovers.is_empty(), "left behind: {leftovers:?}");
}

#[test]
fn listing_an_engine_cache_that_does_not_exist_yet_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    let store = EngineStore::new(dir.path().join("engines"));
    assert!(store.list().unwrap().is_empty());
    assert!(store.remove("10.0").unwrap().is_empty());
}
