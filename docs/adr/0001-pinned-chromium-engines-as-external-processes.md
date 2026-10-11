# Pinned Chromium Engines run as external processes

Apps run on the Chromium milestone their Tizen TV actually ships (Tizen 3.0 = M47 up to 10.0 = M130), not one modern engine, because the point of the tool is to surface the bugs a real TV would show. That rules out Tauri's own webview: WebView2 can't go below M86 and only the latest two majors are available as fixed versions, Linux Tauri uses WebKitGTK, and Tauri 2 has no Chromium (CEF) runtime. So every Engine is a separate Chromium process, downloaded on demand from Chrome for Testing (M113+) or the Chromium snapshot archive (older), and driven over the Chrome DevTools Protocol.

## Considered Options

- **One modern engine, with Version Profiles changing only APIs, keys, user agent and resolution.** Lighter and faster, but it misses engine-level breakage (e.g. ES2017 syntax on M47), which was the main reason for building the tool.
- **Embed the Engine in Tauri's webview.** Impossible for old milestones, as above.

## Consequences

- Old Engines get no security fixes, and the M47 Windows build is 32-bit only. It runs on Windows 11 (#24 prototype, then #38).
- A portable Engine needs read access granted to the AppContainer SIDs after unpacking, or its sandboxed processes can't start (#24).
- Snapshot Engines are older than Chromium's headless mode on Windows: a headless Session runs M47 in a window placed off the screen (`headless = false` in the Engine Manifest).
- Each Engine needs its own testing: a nightly job (#39) runs the in-page Shim suite on every Engine Manifest entry. On Linux it runs under a virtual display (Xvfb), which an off-screen window needs; while every Linux Engine has a headless mode, the newest one also runs off-screen there so that path stays tested.
- CDP usage must stay within what M47 supports, or degrade where it can't. See ADR-0002. Measured on M47 (#38): no `Browser` domain (the window is measured from the page and minimized through the OS), no `Log` domain or `Runtime.consoleAPICalled` (console comes from `Console.messageAdded`), no page lifecycle states (a hidden App is paused in the debugger), screenshots of the window rather than the emulated screen (cut out and scaled), and only one debugging client per page (DevTools goes through a relay on the Session's connection).
- The GUI never hosts the App itself. See ADR-0006 for how the screen gets into the GUI.
