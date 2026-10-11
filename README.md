# tizen-sim

An open-source desktop simulator for **Samsung Tizen TV web apps**, built as a replacement for Samsung's TV Web Simulator.

A Tizen TV runs your App on an old, fixed version of Chromium, with its own platform APIs, remote control, network rules and video layering. tizen-sim runs your App on the **same Chromium milestone the target TV generation ships** and reproduces what that TV's platform does to it. What you see on the desktop is what you'd see on the TV, without deploying to a device for every change.

- **The TV's browser engine.** Each Version Profile pins the Chromium build its TVs ship, downloaded on first use: Chromium M47 for Tizen 3.0 (2017 TVs), Chromium M130 for Tizen 10.0.
- **Platform APIs.** `tizen.*` and `webapis.*` Shims behave as documented, including their errors and privilege checks: calling an API without the privilege your `config.xml` declares throws `SecurityError`, as on the TV.
- **The remote control.** Remote Keys arrive the way the TV delivers them, with `tizen.tvinputdevice` registration. The platform's own key handling is included, for example holding Back terminates the App.
- **Network rules.** Your `config.xml` Access Policy (WARP `<access>` or CSP) is enforced through a proxy, and every request is listed in a Network Log.
- **Lifecycle.** Hiding and resuming fire `visibilitychange` and pause JavaScript, and background support is honoured.
- **Video.** AVPlay plays on a Video Plane behind the page, so transparent regions show the video as they do on a TV.
- **Diagnostics.** tizen-sim flags code that behaves differently, or fails, on a real TV.
- **A fast loop.** Hot reload (including your own bundler's dev server), console streaming, DevTools, screenshots, a CLI and a desktop GUI.

> **Status:** early development. The product spec is [#23](https://github.com/lucaskfp/tizen-sim/issues/23); its sub-issues are the roadmap.

## Supported platforms and TVs

| Version Profile | Engine         | Windows x64 | Linux x64 |
| --------------- | -------------- | ----------- | --------- |
| Tizen 10.0      | Chromium M130  | yes         | yes       |
| Tizen 3.0       | Chromium M47   | yes         | yes       |

- **Version Profiles.** The goal is every release from Tizen 3.0 to 10.0. `tizen-sim profiles` lists the ones available in your build.
- **Linux** is a tier 2 platform. [docs/linux.md](docs/linux.md) lists where it differs from Windows and what it needs from the system, such as GTK 2 and GConf for the Tizen 3.0 Engine.
- **macOS** is not supported.

## Getting started

There are no prebuilt releases yet, so build from source. You need:

- [Rust](https://rustup.rs) (stable). The CLI needs nothing else.
- [Node.js](https://nodejs.org) 24, for the GUI or for changing the Shims.
- On Linux, the system packages listed in [docs/linux.md](docs/linux.md).

```bash
git clone https://github.com/lucaskfp/tizen-sim.git
cd tizen-sim
cargo build --release -p tizen-sim
```

The CLI binary is `target/release/tizen-sim`. Engines are downloaded to `~/.tizen-sim/engines` the first time a Version Profile is used; set `TIZEN_SIM_HOME` to keep tizen-sim's data elsewhere.

### The CLI

```bash
# Run a Project Folder (a directory with a config.xml) and stream its console
tizen-sim run path/to/my-app

# Run a packaged .wgt on the Tizen 3.0 Engine
tizen-sim run my-app.wgt --profile 3.0

# Run your bundler's dev server, with the App's config.xml
tizen-sim run http://localhost:5173 --config path/to/config.xml

# Open DevTools once the App has loaded
tizen-sim run path/to/my-app --devtools
```

`run` keeps going until Ctrl+C and reloads the App when its files change. These commands act on a running Session from another terminal:

```bash
tizen-sim sessions                 # list running Sessions
tizen-sim key Back                 # press a Remote Key
tizen-sim reload                   # reload the App
tizen-sim screenshot               # save a PNG at the TV's resolution
tizen-sim reset                    # forget what the App stored, for a first launch
tizen-sim engines                  # list, fetch and remove cached Engines
```

Run `tizen-sim help <command>` for every option. `run` also takes these:

- `--headless`: no window.
- `--lenient`: allow calls a TV would refuse, reporting each as a Diagnostic.
- `--map-key`: map a keyboard key to a Remote Key.
- `--device`: override the device values the APIs report.
- `--resolution`: 1920x1080 or 1280x720.
- `--engine-path`: use your own Chromium build.

### The GUI

The desktop GUI does everything the CLI does. It adds an on-screen remote, a Version Profile switcher, and the log, Network Log and Diagnostics in panels. It can also adopt a Session started from the CLI.

```bash
cd crates/tizen-sim-gui
npm ci
npm run tauri dev
```

See [crates/tizen-sim-gui/README.md](crates/tizen-sim-gui/README.md) for details.

## How it works

- **Engines.** Each Engine is a pinned Chromium build run as an external process and driven over the DevTools protocol (ADR-0001).
- **Shims.** A local dev server serves your App and injects the Shims into its HTML (ADR-0002). The Shims are TypeScript compiled to ES5, so they run on every Engine down to M47.
- **Network.** Traffic goes through a proxy that enforces the App's Access Policy (ADR-0003).
- **Video.** The AVPlay Video Plane is emulated in the page (ADR-0004).

Every architectural decision is recorded in [docs/adr/](docs/adr/).

| Path                      | What it is                                                                          |
| ------------------------- | ----------------------------------------------------------------------------------- |
| `crates/tizen-sim-core`   | The simulator: Sessions, Engines, Version Profiles, the dev server, network proxy and Shim host |
| `crates/tizen-sim-cli`    | The `tizen-sim` command-line tool                                                   |
| `crates/tizen-sim-gui`    | The desktop GUI (Tauri v2, React, TypeScript)                                       |
| `shims/`                  | The in-page `tizen.*` and `webapis.*` Shims                                         |
| `fixtures/apps/`          | Tiny Apps used by the test suites                                                   |
| `docs/`                   | ADRs, Linux notes, and the documented TV behaviour the simulator follows            |

## Scope

tizen-sim emulates **the platform**: what a Tizen TV does for and to an App. It does not judge **how an App should behave**. Samsung's UX guidelines and certification checklist (exit popups, focus rules, launch-time targets) are the App developer's concern, and the simulator neither enforces nor checks them. When in doubt, ask: *does the TV do this, or does Samsung tell the App to do it?* Only the first belongs here.

Also out of scope:

- DRM playback
- Tizen 2.x/WebKit TVs
- packaging and signing `.wgt` files
- Tizen Studio integration
- decrypting HTTPS
- non-TV Tizen profiles
- macOS

## Contributing

Contributions are welcome: bug reports, behaviour you've measured on a real TV, docs and code.

### Reporting issues and proposing work

- **Issues** live in [GitHub Issues](https://github.com/lucaskfp/tizen-sim/issues). Pull requests aren't used to request features, so open an issue first for anything beyond a small fix.
- **Bug reports** should give:
  - the Version Profile;
  - your OS;
  - the command or GUI steps;
  - what you expected, ideally what the real TV does.
- **Triage labels:**
  - `needs-triage`: not evaluated yet.
  - `needs-info`: waiting on the reporter.
  - `ready-for-agent`: fully specified, ready for an AI agent.
  - `ready-for-human`: needs human implementation.
  - `wontfix`: won't be actioned.
  
  Issues labelled `priority: high` come first.
- **Measurements from a real TV** are especially valuable: what an API returns, which keys arrive, what the network allows. Many documented rules are marked *unverified* until someone checks them on hardware.

### Platform behaviour must be sourced

tizen-sim is only useful if it matches real TVs, so emulated behaviour is never filled in from memory.

1. Read the rules already recorded in [docs/agents/tizen-behaviour.md](docs/agents/tizen-behaviour.md).
2. Find the source page for the behaviour you're changing, using [docs/agents/tizen-docs.md](docs/agents/tizen-docs.md), and cite it in the code or tests.
3. If no source or measurement exists, mark the behaviour **unverified** rather than guessing.

### Vocabulary and decisions

- [CONTEXT.md](CONTEXT.md) defines the project's terms (App, Version Profile, Engine, Session, Shim, Remote Key, Diagnostic, ...). Use them in code, docs and issues.
- A significant design decision gets an ADR in [docs/adr/](docs/adr/).

### Development

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The tests start real Engines, so the first run downloads them.

**Test environment variables:**

| Variable                     | Effect                                                                                    |
| ---------------------------- | ----------------------------------------------------------------------------------------- |
| `TIZEN_SIM_TEST_PROFILE`     | The Version Profile to test on, e.g. `TIZEN_SIM_TEST_PROFILE=3.0 cargo test -p tizen-sim-core` |
| `TIZEN_SIM_TEST_HEADED`      | Run the Engines in visible windows                                                        |
| `TIZEN_SIM_TEST_NO_HEADLESS` | Run as an Engine without a headless mode does, in an off-screen window                    |

On Linux, run the Tizen 3.0 tests under `xvfb-run`, as CI does.

**Shims.** The core embeds the committed bundle, `shims/dist/tizen-sim-shims.js`. After changing `shims/src`, rebuild it and commit the result; CI checks that it matches the sources and is ES5:

```bash
cd shims
npm ci
npm run build
npm run check-es5
```

**GUI frontend.** In `crates/tizen-sim-gui`, `npm run build` and `npm test` typecheck the frontend and smoke-test it.

**Other conventions:**

- **Commits** follow [Conventional Commits](https://www.conventionalcommits.org) (`feat:`, `fix:`, `docs:`, `ci:`, ...) and reference their issue, e.g. `(#50)`.
- **Code** should read like the code around it: same naming, idiom and comment density.
- **CI** runs lint and tests on Windows and Linux on every push. A nightly job runs the in-page Shim suite on every Version Profile's Engine.

### AI agents

The repo is set up for coding agents. [AGENTS.md](AGENTS.md) is their entry point, and [docs/agents/](docs/agents/) holds the issue-tracker, triage and platform-documentation conventions.

## License

Licensed under either of MIT or Apache-2.0, at your option, as declared in `Cargo.toml`.
