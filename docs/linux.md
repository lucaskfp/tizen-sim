# tizen-sim on Linux

Linux (x86-64) is a tier 2 platform: the CLI and the GUI do everything they do on Windows. CI runs the Session, CLI and GUI tests on Linux on Tizen 10.0, the Session suite on Tizen 3.0, and starts the real GUI. This page lists where Linux differs and what it needs from the system.

## What it needs

- **The GUI** runs on Tauri's Linux webview, WebKitGTK 4.1 (`libwebkit2gtk-4.1-0` on Debian and Ubuntu). The CLI doesn't need it.
- **Engines** are downloaded on first use, as on Windows. The Tizen 10.0 Engine (Chrome for Testing M130) needs only what any Chrome needs.
- **The Tizen 3.0 Engine** (a 2015 Chromium M47 snapshot) links GTK 2, GConf and dbus-glib: `libgtk2.0-0`, `libgconf-2-4` and `libdbus-glib-1-2` on Debian and Ubuntu. GConf is gone from Debian 13 and Ubuntu 24.04. On those, install Debian 12's `libgconf-2-4` and `gconf2-common` packages (from `deb.debian.org/debian/pool/main/g/gconf/`) by hand. When a library is missing, the Session fails with the loader's message naming it, for example `error while loading shared libraries: libgconf-2.so.4`.
- **Xvfb** (optional; the `xvfb` package) gives a headless Session on the Tizen 3.0 Engine a display of its own. See below.

## How it differs from Windows

- **The Engine keeps its own window.** The GUI doesn't embed the TV screen (ADR-0006): its screen area says the TV screen is in its own Engine window, and the remote, logs and the rest work the same. An adopted Session's window stays where it is, and letting go of the Session leaves it there.
- **No Chromium sandbox on Tizen 3.0.** Chromium M47's Linux sandbox needs a setuid-root helper (`chrome_sandbox`), which a per-user download can't install, so that Engine runs with `--no-sandbox` (`sandbox = false` in the Engine Manifest). So does your own build given with `--engine-path` for Tizen 3.0, since it has the same milestone's needs. It already gets no security fixes (ADR-0001): run only Apps and content you trust on it.
- **Headless Sessions on Tizen 3.0.** M47 has no headless mode, so a headless Session runs it in a window (ADR-0001). With Xvfb installed, that window opens on a private virtual display that stops with the Engine. Without Xvfb, the window is placed off the screen. An X11 desktop honours that, but Wayland compositors, and X11 apps under XWayland, ignore it, so the window shows on the desktop.
- **Hiding an App on Tizen 3.0** (`Application.hide`, or Home) pauses its JavaScript as on Windows, but the Engine window isn't minimized: M47 has no protocol command for it, and tizen-sim only minimizes through the OS on Windows (#50). So the App never sees itself hidden: `document.hidden` stays false and `visibilitychange` doesn't fire, neither on hiding nor on exit. The nightly Shim suite fails on Tizen 3.0 on Linux until this is fixed.
- **Engine shutdown.** An Engine dies with the `tizen-sim` process that started it, even when that process is killed: `PR_SET_PDEATHSIG` on Linux, a Job Object on Windows.

## Known system issues

- **Ubuntu 24.04 and newer** block the unprivileged user namespaces Chromium's sandbox uses, so the Tizen 10.0 Engine exits at startup with a sandbox error. Allow them with `sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0`; CI does the same. Making it permanent, or adding an AppArmor profile for the Engine, is the machine owner's choice.
- **WSL (WSLg)** mounts `/tmp/.X11-unix` read-only, so Xvfb can't start there, and headless Tizen 3.0 Sessions fall back to an off-screen window, which WSLg shows. Everything else works under WSLg.
