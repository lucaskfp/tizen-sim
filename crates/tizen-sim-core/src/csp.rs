//! The content security policy a CSP-mode App's pages get (TD
//! guides/concepts/web-runtime.md, "Content security policy"). The dev
//! server sends it as response headers; a policy the bundler also sends is
//! enforced alongside it, so the stricter of the two applies, as on a TV.

use crate::app::AccessPolicy;

/// The policy the web runtime enforces when `config.xml` defines none.
pub const DEFAULT_POLICY: &str =
    "default-src *; script-src 'self'; style-src 'self'; object-src 'none';";

/// The headers for a CSP-mode App's documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageCsp {
    /// `Content-Security-Policy`.
    pub enforced: Option<String>,
    /// `Content-Security-Policy-Report-Only`.
    pub report_only: Option<String>,
}

/// What the Shims load from the dev server. On a TV they are part of the
/// platform, out of the App's policy's reach, so the policy is widened to
/// let exactly these through.
pub struct ShimSources<'a> {
    /// The bootstrap script's URL.
    pub script: &'a str,
    /// The Bridge's WebSocket URL, without its query.
    pub bridge: &'a str,
    /// The Bridge's URL for the calls the page waits on, without its query.
    pub bridge_http: &'a str,
}

/// The headers for `policy`'s pages; `None` in WARP mode.
///
/// A `config.xml` policy replaces the default one. The doc counts a
/// report-only policy as one, so an App with only a report-only policy has
/// nothing enforced. Both the default and that reading were measured on a
/// Tizen 5.0 TV (ADR-0003).
pub fn page_csp(policy: &AccessPolicy, shims: &ShimSources) -> Option<PageCsp> {
    let AccessPolicy::Csp(csp) = policy else {
        return None;
    };
    let enforced = match (&csp.policy, &csp.report_only) {
        (Some(policy), _) => Some(policy.as_str()),
        (None, Some(_)) => None,
        (None, None) => Some(DEFAULT_POLICY),
    };
    Some(PageCsp {
        enforced: enforced.map(|policy| allow_shims(policy, shims)),
        report_only: csp.report_only.clone(),
    })
}

/// Adds the Shims' sources to the directives that would block them.
fn allow_shims(policy: &str, shims: &ShimSources) -> String {
    let mut directives: Vec<Vec<String>> = policy
        .split(';')
        .map(|d| d.split_ascii_whitespace().map(String::from).collect())
        .filter(|d: &Vec<String>| !d.is_empty())
        .collect();
    let find = |directives: &[Vec<String>], name: &str| {
        directives
            .iter()
            .position(|d| d[0].eq_ignore_ascii_case(name))
    };
    for (name, source) in [
        ("script-src", shims.script),
        ("script-src-elem", shims.script),
        ("connect-src", shims.bridge),
        ("connect-src", shims.bridge_http),
    ] {
        let at = match find(&directives, name) {
            Some(at) => at,
            // script-src-elem falls back to script-src, already widened.
            None if name == "script-src-elem" => continue,
            None => match find(&directives, "default-src") {
                Some(default) => {
                    let mut inherited = directives[default].clone();
                    inherited[0] = name.to_string();
                    directives.push(inherited);
                    directives.len() - 1
                }
                None => continue,
            },
        };
        let directive = &mut directives[at];
        // 'none' must stand alone.
        directive.retain(|s| !s.eq_ignore_ascii_case("'none'"));
        directive.push(source.to_string());
    }
    directives
        .iter()
        .map(|d| d.join(" "))
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::CspPolicy;

    const SHIMS: ShimSources = ShimSources {
        script: "http://127.0.0.1:1/boot.js",
        bridge: "ws://127.0.0.1:1/bridge",
        bridge_http: "http://127.0.0.1:1/bridge",
    };

    fn csp(policy: Option<&str>, report_only: Option<&str>) -> AccessPolicy {
        AccessPolicy::Csp(CspPolicy {
            policy: policy.map(String::from),
            report_only: report_only.map(String::from),
            allow_navigation: None,
        })
    }

    #[test]
    fn warp_mode_has_no_policy() {
        assert_eq!(page_csp(&AccessPolicy::Warp(vec![]), &SHIMS), None);
    }

    #[test]
    fn config_policy_replaces_the_default_and_lets_the_shims_through() {
        let page = page_csp(&csp(Some("default-src 'none'; img-src *"), None), &SHIMS).unwrap();
        assert_eq!(
            page.enforced.as_deref(),
            Some(
                "default-src 'none'; img-src *; script-src http://127.0.0.1:1/boot.js; \
                 connect-src ws://127.0.0.1:1/bridge http://127.0.0.1:1/bridge"
            )
        );
        assert_eq!(page.report_only, None);

        let page = page_csp(
            &csp(
                Some("script-src 'self' 'unsafe-eval'; script-src-elem 'none'"),
                None,
            ),
            &SHIMS,
        )
        .unwrap();
        assert_eq!(
            page.enforced.as_deref(),
            Some(
                "script-src 'self' 'unsafe-eval' http://127.0.0.1:1/boot.js; \
                 script-src-elem http://127.0.0.1:1/boot.js"
            )
        );
    }

    #[test]
    fn without_a_config_policy_the_default_applies() {
        // Only <tizen:allow-navigation> put the App in CSP mode.
        let page = page_csp(&csp(None, None), &SHIMS).unwrap();
        assert_eq!(
            page.enforced.as_deref(),
            Some(
                "default-src *; script-src 'self' http://127.0.0.1:1/boot.js; style-src 'self'; \
                 object-src 'none'; connect-src * ws://127.0.0.1:1/bridge \
                 http://127.0.0.1:1/bridge"
            )
        );
    }

    #[test]
    fn a_report_only_policy_alone_enforces_nothing() {
        let page = page_csp(&csp(None, Some("default-src 'self'")), &SHIMS).unwrap();
        assert_eq!(page.enforced, None);
        assert_eq!(page.report_only.as_deref(), Some("default-src 'self'"));
    }
}
