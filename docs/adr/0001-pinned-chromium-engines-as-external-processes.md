# Pinned Chromium Engines run as external processes

Apps run on the Chromium milestone their Tizen TV actually ships (Tizen 3.0 = M47 up to 10.0 = M130), not one modern engine, because the point of the tool is to surface the bugs a real TV would show. That rules out Tauri's own webview: WebView2 can't go below M86 and only the latest two majors are available as fixed versions, Linux Tauri uses WebKitGTK, and Tauri 2 has no Chromium (CEF) runtime. So every Engine is a separate Chromium process, downloaded on demand from Chrome for Testing (M113+) or the Chromium snapshot archive (older), and driven over the Chrome DevTools Protocol.

## Considered Options

- **One modern engine, with Version Profiles changing only APIs, keys, user agent and resolution.** Lighter and faster, but it misses engine-level breakage (e.g. ES2017 syntax on M47), which was the main reason for building the tool.
- **Embed the Engine in Tauri's webview.** Impossible for old milestones, as above.

## Consequences

- Old Engines get no security fixes, and the M47 Windows build is 32-bit only. Whether it runs on Windows 11 must be proven by the prototype before the MVP.
- CDP usage must stay within what M47 supports. See ADR-0002.
- The GUI never hosts the App itself. See ADR-0006 for how the screen gets into the GUI.
