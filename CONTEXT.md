# tizen-sim

A desktop simulator for Samsung Tizen TV web applications. It runs each App on the same browser engine version the real TV ships and reproduces the TV's platform APIs, remote control, network rules and video layering.

## Language

### Apps

**App**:
A Tizen TV web application, identified by the application id declared in its `config.xml`.
_Avoid_: widget, wgt, project

**App Source**:
Where an App's files come from: a Package, a Project Folder, or a Dev Server URL.
_Avoid_: input, target

**Package**:
A packaged App in a `.wgt` archive.
_Avoid_: bundle, widget file

**Project Folder**:
A directory containing an App's unpacked files, including its `config.xml`.
_Avoid_: workspace, source dir

**Dev Server URL**:
The address of a developer's own bundler dev server, paired with a `config.xml`, used as an App Source.
_Avoid_: remote URL, hosted app

**Access Policy**:
The network and navigation rules an App declares in its `config.xml`. It is in either **WARP mode** (an allowlist of origins, everything else denied) or **CSP mode** (Content Security Policy rules).
_Avoid_: CORS policy, whitelist

### Running

**Version Profile**:
A description of one Tizen TV release: its Tizen version, its Engine, which platform APIs and Remote Keys exist, its screen resolution and its user agent.
_Avoid_: target, OS version, platform version

**Engine**:
The pinned Chromium build matching the browser engine a Version Profile's TVs ship.
_Avoid_: webview, browser, runtime, web engine

**Engine Manifest**:
The list that says where to obtain the Engine for each Version Profile on each desktop platform.
_Avoid_: engine registry, download list

**Session**:
One App running in one Engine under one Version Profile, controllable from both the GUI and the CLI.
_Avoid_: instance, run, simulation

**Session Registry**:
The per-user list of running Sessions, used by clients to find and control them.
_Avoid_: session store, daemon

**Lenient Mode**:
A Session setting that turns off strict TV enforcement of privileges and key registration, logging the violations instead.
_Avoid_: debug mode, permissive mode

### Platform emulation

**Shim**:
The in-page imitation of a platform API (`tizen.*`, `webapis.*`) as the App sees it.
_Avoid_: polyfill, stub, mock, fake

**Host**:
The simulator side that backs Shims needing state outside the page, such as files, registered keys or app exit.
_Avoid_: backend, native side

**Sandbox**:
An App's directory on the developer's disk that stands in for the TV's storage of the App's files, under Tizen's virtual roots (`wgt-private`, `documents`, ...). It persists across Sessions until the App is reset.
_Avoid_: app storage, data folder

**Bridge**:
The channel over which Shims and the Host talk.
_Avoid_: IPC, RPC layer

**Remote Key**:
A button on the TV remote, identified by its Tizen key name and key code. It is either a **Mandatory Key** (arrows, Enter, Back), which is always delivered, or a **Registrable Key**, which is delivered only after the App registers it.
_Avoid_: VK_ key, key code, TV key

**Video Plane**:
The layer behind the web page where AVPlay video appears. Any opaque page content above it hides the video.
_Avoid_: video layer, video overlay, player

**Network Log**:
The Session's record of every request the App makes over the network: method, URL, status, timing and the Access Policy's verdict (allowed or blocked). The App's own files aren't in it, since on a TV they aren't fetched over the network.
_Avoid_: request log, HAR, traffic

**Diagnostic**:
A warning raised when an App does something that would behave differently or fail on a real TV.
_Avoid_: lint, error, hint
