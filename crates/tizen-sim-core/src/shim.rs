//! The Shim bootstrap: the script every served HTML document loads before any
//! App script (ADR-0002), carrying the Session's data and the Shim bundle.

use crate::app::{App, ReadyWhen};
use crate::profile::VersionProfile;
use crate::remote_keys::{KeyMap, RemoteKey};
use crate::screen::AppResolution;
use serde_json::{Value, json};

/// The compiled Shims (`npm run build` in `shims/`), ES5 so they run on M47.
const SHIM_BUNDLE: &str = include_str!("../../../shims/dist/tizen-sim-shims.js");

/// Where the dev server serves the bootstrap script.
pub const BOOTSTRAP_PATH: &str = "/__tizen-sim/bootstrap.js";
/// Where the dev server accepts the Bridge: the WebSocket for the page's
/// notifications, and `POST`s for the calls the page waits on (ADR-0008).
pub const BRIDGE_PATH: &str = "/__tizen-sim/bridge";
/// Where the dev server serves the files of the App's Sandbox, by virtual
/// path, so the URIs `tizen.filesystem` hands out load in the page.
pub const SANDBOX_PATH: &str = "/__tizen-sim/sandbox";
/// The script that gives a page the Samsung Product API, as Apps reference it
/// (samsung-product-api-references.html). Relative references resolve below
/// any directory, so the dev server answers it at every path depth.
pub const WEBAPIS_SCRIPT: &str = "$WEBAPIS/webapis/webapis.js";

/// What the virtual `$WEBAPIS/webapis/webapis.js` serves: the bootstrap
/// already holds the Shims, so it only installs `webapis`.
pub const WEBAPIS_SCRIPT_BODY: &str = "window.__TIZEN_SIM__.installWebapis();\n";

/// Whether a request path names the virtual `$WEBAPIS` script, whatever
/// directory the page referring to it is in.
pub fn is_webapis_script(uri_path: &str) -> bool {
    let decoded = percent_encoding::percent_decode_str(uri_path).decode_utf8_lossy();
    decoded
        .strip_suffix(WEBAPIS_SCRIPT)
        .is_some_and(|dir| dir.ends_with('/'))
}

/// The Session data the Shims need, minus the Bridge address, which only the
/// dev server knows. `device` is the profile's device values with the
/// Session's overrides applied (`VersionProfile::device_values`).
/// `resolution` is the App's screen: the Shims report it as `screen.width`
/// and `height` on Engines whose metrics override can't (see `screen`).
pub fn session_data(
    app: &App,
    profile: &VersionProfile,
    device: &Value,
    lenient: bool,
    key_map: &KeyMap,
    resolution: AppResolution,
) -> Value {
    let (width, height) = resolution.size();
    let remote_keys: Vec<Value> = RemoteKey::all()
        .iter()
        .filter(|key| profile.supports_key(key))
        .map(|key| json!({ "name": key.name, "code": key.code, "mandatory": key.mandatory }))
        .collect();
    json!({
        "lenient": lenient,
        "sandboxPath": SANDBOX_PATH,
        "keyMap": key_map.to_json(),
        "screen": { "width": width, "height": height },
        "app": {
            "id": app.id(),
            "packageId": app.package(),
            "name": app.name(),
            "version": app.version(),
            "privileges": app.privileges(),
            "installedAt": installed_at_ms(),
        },
        "launchScreen": app.launch_screen().map(|screen| json!({
            "readyWhen": match screen.ready_when {
                ReadyWhen::FirstPaint => "first-paint",
                ReadyWhen::Complete => "complete",
                ReadyWhen::Custom => "custom",
            },
            "backgroundColor": screen.background_color,
            "backgroundImage": screen.background_image.as_deref().map(|file| app.served_path(file)),
            "image": screen.image.as_deref().map(|file| app.served_path(file)),
        })),
        "profile": {
            "version": profile.version,
            "milestone": profile.milestone,
            "capabilities": profile.capabilities,
            "device": device,
            "remoteKeys": remote_keys,
            "apis": crate::apis::ApiCatalog::bundled().for_version(&profile.version),
            "media": crate::media::MediaCatalog::bundled().for_version(&profile.version),
            "webFeatures": crate::web_features::WebFeatureCatalog::bundled().removed_on(&profile.version),
        },
    })
}

/// The Session's start time, reported as the App's install date.
fn installed_at_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// The bootstrap script: the Session data as `window.__TIZEN_SIM__`, then the
/// Shim bundle.
pub fn bootstrap_script(mut data: Value, bridge_url: &str) -> String {
    data["bridgeUrl"] = Value::String(bridge_url.to_string());
    // JSON allows U+2028/U+2029 in strings but ES5 doesn't, so an App name
    // containing one would be a syntax error on old Engines.
    let data = data
        .to_string()
        .replace('\u{2028}', r"\u2028")
        .replace('\u{2029}', r"\u2029");
    format!("window.__TIZEN_SIM__ = {data};\n{SHIM_BUNDLE}")
}

/// Inserts the bootstrap `<script>` as the first child of `<head>`, so it runs
/// before any App script. Documents without `<head>` get it right after
/// `<html>`, else after the doctype and any leading comments.
///
/// Works on bytes so documents in any ASCII-compatible encoding keep their
/// content untouched.
pub fn inject_bootstrap(html: &[u8]) -> Vec<u8> {
    let tag = format!(r#"<script src="{BOOTSTRAP_PATH}"></script>"#);
    let at = open_tag_end(html, b"head")
        .or_else(|| open_tag_end(html, b"html"))
        .unwrap_or_else(|| prologue_end(html));
    let mut out = Vec::with_capacity(html.len() + tag.len());
    out.extend_from_slice(&html[..at]);
    out.extend_from_slice(tag.as_bytes());
    out.extend_from_slice(&html[at..]);
    out
}

/// Byte offset just past the first `<name ...>` opening tag outside comments,
/// matching case-insensitively and not as a prefix (`<head>`, not `<header>`).
fn open_tag_end(html: &[u8], name: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < html.len() {
        if html[i..].starts_with(b"<!--") {
            i = comment_end(html, i);
            continue;
        }
        if html[i] == b'<' && starts_with_ignore_case(&html[i + 1..], name) {
            let after = i + 1 + name.len();
            if matches!(
                html.get(after),
                Some(b'>' | b' ' | b'\t' | b'\n' | b'\r' | b'/')
            ) {
                return html[after..]
                    .iter()
                    .position(|&b| b == b'>')
                    .map(|end| after + end + 1);
            }
        }
        i += 1;
    }
    None
}

/// Byte offset past leading whitespace, comments and `<!DOCTYPE ...>`.
fn prologue_end(html: &[u8]) -> usize {
    let mut i = 0;
    loop {
        while html.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        if html[i..].starts_with(b"<!--") {
            i = comment_end(html, i);
        } else if starts_with_ignore_case(&html[i..], b"<!doctype") {
            i = html[i..]
                .iter()
                .position(|&b| b == b'>')
                .map_or(html.len(), |end| i + end + 1);
        } else {
            return i;
        }
    }
}

fn comment_end(html: &[u8], start: usize) -> usize {
    html[start + 4..]
        .windows(3)
        .position(|w| w == b"-->")
        .map_or(html.len(), |end| start + 4 + end + 3)
}

fn starts_with_ignore_case(haystack: &[u8], prefix: &[u8]) -> bool {
    haystack.len() >= prefix.len() && haystack[..prefix.len()].eq_ignore_ascii_case(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAG: &str = r#"<script src="/__tizen-sim/bootstrap.js"></script>"#;

    fn inject(html: &str) -> String {
        String::from_utf8(inject_bootstrap(html.as_bytes())).unwrap()
    }

    #[test]
    fn goes_first_in_head() {
        assert_eq!(
            inject("<!DOCTYPE html><html><head><script>app()</script></head></html>"),
            format!("<!DOCTYPE html><html><head>{TAG}<script>app()</script></head></html>")
        );
    }

    #[test]
    fn handles_attributes_case_and_lookalike_tags() {
        assert_eq!(
            inject("<HTML lang=en><header></header><HEAD data-x=\"1\">"),
            format!("<HTML lang=en><header></header><HEAD data-x=\"1\">{TAG}")
        );
    }

    #[test]
    fn ignores_tags_inside_comments() {
        assert_eq!(
            inject("<!-- <head> --><html><head><title>t</title>"),
            format!("<!-- <head> --><html><head>{TAG}<title>t</title>")
        );
    }

    #[test]
    fn falls_back_to_html_then_after_the_doctype() {
        assert_eq!(
            inject("<html><body><script>app()</script>"),
            format!("<html>{TAG}<body><script>app()</script>")
        );
        assert_eq!(
            inject("<!DOCTYPE html>\n<!-- c --><script>app()</script>"),
            format!("<!DOCTYPE html>\n<!-- c -->{TAG}<script>app()</script>")
        );
        assert_eq!(
            inject("<script>app()</script>"),
            format!("{TAG}<script>app()</script>")
        );
    }

    #[test]
    fn leaves_non_utf8_content_untouched() {
        let latin1 = b"<html><head><title>Caf\xe9</title>";
        let out = inject_bootstrap(latin1);
        assert!(out.ends_with(b"<title>Caf\xe9</title>"));
        assert!(out.windows(TAG.len()).any(|w| w == TAG.as_bytes()));
    }

    #[test]
    fn recognises_the_webapis_script_at_any_depth() {
        assert!(is_webapis_script("/$WEBAPIS/webapis/webapis.js"));
        assert!(is_webapis_script(
            "/pages/nested/$WEBAPIS/webapis/webapis.js"
        ));
        assert!(is_webapis_script("/%24WEBAPIS/webapis/webapis.js"));
        assert!(!is_webapis_script("/webapis/webapis.js"));
        assert!(!is_webapis_script("/x$WEBAPIS/webapis/webapis.js"));
    }

    #[test]
    fn bootstrap_data_is_valid_es5() {
        let script = bootstrap_script(
            json!({ "app": { "name": "a\u{2028}b\u{2029}c" } }),
            "ws://x",
        );
        let first_line = script.lines().next().unwrap();
        assert!(!first_line.contains('\u{2028}') && !first_line.contains('\u{2029}'));
        assert!(first_line.contains(r"a\u2028b\u2029c"), "{first_line}");
    }
}
