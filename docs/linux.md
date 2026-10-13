# tizen-sim on Linux

Linux (x86-64) is a tier 2 platform: the CLI and the GUI do everything they do on Windows. CI runs the Session, CLI and GUI tests on Linux on Tizen 10.0, the Session suite on Tizen 3.0, and starts the real GUI; a nightly job runs the in-page Shim suite and the network tests on every Version Profile. This page lists where Linux differs and what it needs from the system.

## What it needs

- **The GUI** runs on Tauri's Linux webview, WebKitGTK 4.1 (`libwebkit2gtk-4.1-0` on Debian and Ubuntu). The CLI doesn't need it.
- **Engines** are downloaded on first use, as on Windows. The Engines from Tizen 5.5 (M69) to 10.0 (M130) need only what any Chrome needs.
- **The Tizen 3.0 to 5.0 Engines** (Chromium M47, M56 and M63 snapshots) link GConf, `libgconf-2-4` on Debian and Ubuntu. M47 and M56 also link GTK 2 (`libgtk2.0-0`), M47 dbus-glib (`libdbus-glib-1-2`), and M63 GTK 3. GConf is gone from Debian 13 and Ubuntu 24.04. On those, install Debian 12's `libgconf-2-4` and `gconf2-common` packages (from `deb.debian.org/debian/pool/main/g/gconf/`) by hand. When a library is missing, the Session fails with the loader's message naming it, for example `error while loading shared libraries: libgconf-2.so.4`.
- **Xvfb** (optional; the `xvfb` package) gives a headless Session on the Tizen 3.0 to 8.0 Engines a display of its own. See below.

## How it differs from Windows

- **The Engine keeps its own window.** The GUI doesn't embed the TV screen (ADR-0006): its screen area says the TV screen is in its own Engine window, and the remote, logs and the rest work the same. An adopted Session's window stays where it is, and letting go of the Session leaves it there.
- **No Chromium sandbox on Tizen 3.0 to 6.5.** Chromium M47's Linux sandbox needs a setuid-root helper (`chrome_sandbox`), which a per-user download can't install, so that Engine runs with `--no-sandbox` (`sandbox = false` in the Engine Manifest). So does your own build given with `--engine-path` for Tizen 3.0, since it has the same milestone's needs. It already gets no security fixes (ADR-0001): run only Apps and content you trust on it. Under the sandbox, M56 to M85 (Tizen 4.0 to 6.5) start but never open a page, as their renderers don't start (measured on Debian 13), so they run with `--no-sandbox` too. From Tizen 7.0 (M94) on, the Engines use Chromium's namespace sandbox, which needs no helper.
- **The Tizen 4.0 Engine is patched on install.** M56 exports the HarfBuzz it links statically, and the system's Pango binds to that copy instead of the system's HarfBuzz and crashes the Engine at startup (Debian 13, Ubuntu 22.04). tizen-sim renames those exports (`unexport` in the Engine Manifest) after unpacking the verified download. Your own M56 build given with `--engine-path` isn't patched.
- **Headless Sessions on Tizen 3.0 to 8.0.** M47 and M56 have no headless mode, and M63 to M108 only Chromium's old one, whose window never minimizes, so a hidden App would still see itself visible. A headless Session runs them in a window (ADR-0001). So does your own build given with `--engine-path` for those Version Profiles. With Xvfb installed, that window opens on a private virtual display that stops with the Engine. Without Xvfb, the window is placed off the screen. An X11 desktop honours that, but Wayland compositors, and X11 apps under XWayland, ignore it, so the window shows on the desktop.
- **Engine shutdown.** An Engine dies with the `tizen-sim` process that started it, even when that process is killed: `PR_SET_PDEATHSIG` on Linux, a Job Object on Windows.

## Known system issues

- **Ubuntu 24.04 and newer** block the unprivileged user namespaces Chromium's sandbox uses, so the Engines from Tizen 7.0 on exit at startup with a sandbox error. Allow them with `sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0`; CI does the same. Making it permanent, or adding an AppArmor profile for the Engine, is the machine owner's choice.
- **WSL (WSLg)** mounts `/tmp/.X11-unix` read-only, so Xvfb can't start there, and headless Sessions on Tizen 3.0 to 8.0 fall back to an off-screen window, which WSLg shows. WSLg's window manager also ignores requests to minimize a window, so hiding an App leaves a shown Engine window on the screen, though the App still sees itself hidden. Everything else works under WSLg.
