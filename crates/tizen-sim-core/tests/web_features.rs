//! Web features the Version Profile's TVs lack are absent from the page, and
//! those they have are left alone (TV specifications/web-engine-specifications.html,
//! mirrored in data/web_features.toml).
//!
//! Run once per Engine with `TIZEN_SIM_TEST_PROFILE`, as the nightly job does:
//! what each Engine has differs, e.g. the 4.0 Engine has the Geolocation API
//! its TVs lack (measured, #106).

mod support;

use serde_json::{Value, json};
use support::{STARTUP_SECS, profile, start};
use tizen_sim_core::session::Session;
use tizen_sim_core::web_features::WebFeatureCatalog;

/// Whether the page reaches `path`, as the catalog writes it: a path from
/// `window`, or a global name prefix ending in `*`.
async fn reaches(session: &Session, path: &str) -> bool {
    let expression = format!(
        "(function (path) {{
            var parts = path.split('.');
            var owner = window;
            for (var i = 0; i < parts.length - 1; i++) {{
                if (owner == null) return false;
                owner = owner[parts[i]];
            }}
            if (owner == null) return false;
            var last = parts[parts.length - 1];
            if (last.charAt(last.length - 1) !== '*') return last in owner;
            var names = Object.getOwnPropertyNames(owner);
            for (var j = 0; j < names.length; j++) {{
                if (names[j].indexOf(last.slice(0, -1)) === 0) return true;
            }}
            return false;
        }})({})",
        json!(path)
    );
    session.evaluate(&expression).await.unwrap() == Value::Bool(true)
}

fn removed(feature: &str) -> bool {
    WebFeatureCatalog::bundled()
        .removed_on(&profile().version)
        .iter()
        .any(|removed| removed.name == feature)
}

async fn loaded() -> support::TestSession {
    let test = start("web-features");
    tokio::time::timeout(
        std::time::Duration::from_secs(STARTUP_SECS),
        test.session.wait_until_loaded(),
    )
    .await
    .unwrap()
    .unwrap();
    test
}

#[tokio::test]
async fn features_the_tvs_lack_are_absent() {
    let test = loaded().await;
    for feature in WebFeatureCatalog::bundled().removed_on(&profile().version) {
        for path in &feature.paths {
            assert!(
                !reaches(&test.session, path).await,
                "Tizen {}: `{path}` of {} is still there",
                profile().version,
                feature.name
            );
        }
    }
}

#[tokio::test]
async fn the_app_never_sees_them() {
    let test = loaded().await;
    let seen = test.session.evaluate("window.seenAtLoad").await.unwrap();
    // Every Engine has both (measured, #106).
    assert_eq!(seen["geolocation"], !removed("Geolocation API"));
    let promise = if removed("Promise objects") {
        "undefined"
    } else {
        "function"
    };
    assert_eq!(seen["promise"], promise);
}

#[tokio::test]
async fn features_the_tvs_have_keep_working() {
    let test = loaded().await;
    if !removed("Geolocation API") {
        assert!(reaches(&test.session, "navigator.geolocation").await);
    }
    if !removed("Promise objects") {
        let resolved = test
            .session
            .evaluate("Promise.resolve(1) instanceof Promise")
            .await
            .unwrap();
        assert_eq!(resolved, Value::Bool(true));
    }
    if !removed("WebRTC API") {
        assert!(reaches(&test.session, "webkitRTCPeerConnection").await);
    }
}
