//! The Shim bootstrap: the script every served HTML document loads before any
//! App script (ADR-0002), carrying the Session's data and the Shim bundle.

use crate::app::App;
use crate::profile::VersionProfile;
use serde_json::{Value, json};

/// The compiled Shims (`npm run build` in `shims/`), ES5 so they run on M47.
const SHIM_BUNDLE: &str = include_str!("../../../shims/dist/tizen-sim-shims.js");

/// Where the dev server serves the bootstrap script.
pub const BOOTSTRAP_PATH: &str = "/__tizen-sim/bootstrap.js";
/// Where the dev server accepts the Bridge WebSocket.
pub const BRIDGE_PATH: &str = "/__tizen-sim/bridge";

/// The Session data the Shims need, minus the Bridge address, which only the
/// dev server knows.
pub fn session_data(app: &App, profile: &VersionProfile, lenient: bool) -> Value {
    json!({
        "lenient": lenient,
        "app": {
            "id": app.id(),
            "packageId": app.package(),
            "name": app.name(),
            "version": app.version(),
            "privileges": app.privileges(),
            "installedAt": installed_at_ms(),
        },
        "profile": {
            "version": profile.version,
            "milestone": profile.milestone,
            "capabilities": profile.capabilities,
            "device": profile.device,
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
