//! Spotting App documents that only start on Engines with ES modules, such as
//! a bundler's dev output, so old Version Profiles can raise a Diagnostic.

/// The first milestone that runs `<script type="module">` and skips
/// `<script nomodule>` (chromestatus.com/feature/5365692190687232, "ES6
/// Modules", shipped in Chrome 61). Older Engines ignore module scripts.
pub const ES_MODULES_MILESTONE: u32 = 61;

/// Whether the document has module scripts and no `nomodule` fallback, so
/// that none of the App's scripts run on an Engine without ES modules.
pub fn needs_es_modules(html: &[u8]) -> bool {
    let mut has_module = false;
    let mut has_fallback = false;
    for attributes in script_tags(html) {
        has_module |= attributes
            .iter()
            .any(|(name, value)| name == "type" && value.trim().eq_ignore_ascii_case("module"));
        has_fallback |= attributes.iter().any(|(name, _)| name == "nomodule");
    }
    has_module && !has_fallback
}

/// The attributes of each `<script>` tag outside comments and other scripts'
/// contents, names lowercased.
fn script_tags(html: &[u8]) -> Vec<Vec<(String, String)>> {
    let lower = html.to_ascii_lowercase();
    let mut tags = Vec::new();
    let mut i = 0;
    while i < lower.len() {
        if lower[i..].starts_with(b"<!--") {
            i = find(&lower, i + 4, b"-->").map_or(lower.len(), |end| end + 3);
        } else if lower[i..].starts_with(b"<script")
            && lower
                .get(i + 7)
                .is_some_and(|&b| b == b'>' || b == b'/' || b.is_ascii_whitespace())
        {
            let (attributes, length) = parse_attributes(&html[i + 7..]);
            tags.push(attributes);
            i = find(&lower, i + 7 + length, b"</script").unwrap_or(lower.len());
        } else {
            i += 1;
        }
    }
    tags
}

fn find(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|at| from + at)
}

/// Parses a tag's attributes up to its closing `>`, returning them and the
/// length of the tag's rest, `>` included.
fn parse_attributes(tag: &[u8]) -> (Vec<(String, String)>, usize) {
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    let skip = |i: &mut usize, keep: &dyn Fn(u8) -> bool| {
        while tag.get(*i).is_some_and(|&b| keep(b)) {
            *i += 1;
        }
    };
    let mut attributes = Vec::new();
    let mut i = 0;
    loop {
        skip(&mut i, &|b| b.is_ascii_whitespace() || b == b'/');
        match tag.get(i) {
            None => return (attributes, tag.len()),
            Some(b'>') => return (attributes, i + 1),
            _ => {}
        }
        let start = i;
        skip(&mut i, &|b| {
            !b.is_ascii_whitespace() && !matches!(b, b'=' | b'>' | b'/')
        });
        let name = text(&tag[start..i]).to_ascii_lowercase();
        skip(&mut i, &|b| b.is_ascii_whitespace());
        let mut value = String::new();
        if tag.get(i) == Some(&b'=') {
            i += 1;
            skip(&mut i, &|b| b.is_ascii_whitespace());
            if let Some(&quote @ (b'"' | b'\'')) = tag.get(i) {
                let end = find(tag, i + 1, &[quote]).unwrap_or(tag.len());
                value = text(&tag[i + 1..end]);
                i = end + 1;
            } else {
                let start = i;
                skip(&mut i, &|b| !b.is_ascii_whitespace() && b != b'>');
                value = text(&tag[start..i]);
            }
        }
        attributes.push((name, value));
    }
}

#[cfg(test)]
mod tests {
    use super::needs_es_modules;

    fn needs(html: &str) -> bool {
        needs_es_modules(html.as_bytes())
    }

    #[test]
    fn module_scripts_need_es_modules() {
        // Vite's dev output.
        assert!(needs(
            r#"<html><head><script type="module" src="/@vite/client"></script></head>
               <body><script type="module" src="/src/main.ts"></script></body></html>"#
        ));
        assert!(needs("<SCRIPT TYPE=Module>import './a.js'</SCRIPT>"));
        assert!(needs("<script defer type='module' src=a.js></script>"));
    }

    #[test]
    fn classic_scripts_do_not() {
        assert!(!needs(
            r#"<script src="app.js"></script><script>var a;</script>"#
        ));
        assert!(!needs(
            r#"<script type="text/javascript" src="app.js"></script>"#
        ));
        assert!(!needs("<html><body>No scripts</body></html>"));
    }

    /// A legacy build (e.g. @vitejs/plugin-legacy) adds `nomodule` scripts,
    /// which old Engines run instead.
    #[test]
    fn a_nomodule_fallback_runs_on_old_engines() {
        assert!(!needs(
            r#"<script type="module" src="/assets/index.js"></script>
               <script nomodule src="/assets/index-legacy.js"></script>"#
        ));
    }

    #[test]
    fn ignores_comments_script_contents_and_lookalike_tags() {
        assert!(!needs(
            r#"<!-- <script type="module"></script> --><script src="a.js"></script>"#
        ));
        assert!(!needs(
            r#"<script>document.write('<script type="module"><\/script>')</script>"#
        ));
        assert!(!needs(r#"<scripts type="module"></scripts>"#));
        assert!(!needs(r#"<script data-type="module"></script>"#));
    }
}
