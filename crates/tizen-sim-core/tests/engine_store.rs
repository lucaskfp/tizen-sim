//! The Engine cache: download, checksum verification and reuse, served from a
//! local HTTP server so no real Engine is needed.

use sha2::{Digest, Sha256};
use std::io::Write;
use tizen_sim_core::engine::{EngineError, EngineSpec, EngineStore};

fn stand_in_engine_zip() -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut buf);
    let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
    zip.start_file("stand-in-engine/chrome.exe", opts).unwrap();
    zip.write_all(b"not really chrome").unwrap();
    zip.finish().unwrap();
    buf.into_inner()
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
    }
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
