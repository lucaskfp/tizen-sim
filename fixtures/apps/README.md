# Fixture Apps

Tiny Tizen TV Apps used by the test suites. Each folder is a Project Folder App Source, named after the behaviour it exercises.

- `hello`: a valid App that logs to the console on load.
- `missing-config`: no `config.xml` at all.
- `malformed-config`: `config.xml` that isn't well-formed XML.
- `no-application`: valid XML without the `tizen:application` element.
- `missing-entry-point`: `<content src>` names a file that doesn't exist.
- `tizen-apis`: an App that declares no privileges, for the Tizen API Shims. Its first script records whether `tizen` already existed.
- `tizen-apis-privileged`: the same App, declaring `http://tizen.org/privilege/application.info`.
- `webapis`: loads `$WEBAPIS/webapis/webapis.js` and declares the Samsung `productinfo` and `network.public` privileges. Its first script records whether `webapis` already existed, in `index.html` and in `pages/nested/page.html`, which refers to the script relative to its own directory.
- `webapis-unprivileged`: loads `$WEBAPIS/webapis/webapis.js` without declaring any privilege.
- `remote-keys`: declares `tv.inputdevice` and records every keydown `keyCode` in `window.keyLog` and every keyup `keyCode` in `window.keyUpLog`.
- `background-support`: enables `background-support` with a `media` background category, so it keeps running while hidden.
- `screen`: a page with no background and an opaque red 100x100 box at its top-left corner, so everything else is transparent.
- `avplay`: loads `$WEBAPIS/webapis/webapis.js` without declaring any privilege, with an opaque red box at the bottom right of the screen (1700,900 to 1920,1080). `recordEvents()` sets an AVPlay listener that records every playback callback in `window.avEvents`, and `mediaUrl(name)` gives the absolute URL of a file in `media/`: `clip.webm`, a 3-second VP8 video of one colour (`#2080e0`, 320x180), and `not-a-video.webm`, which no Engine decodes.
- `required-version-too-high`: declares `required_version="99.0"`, newer than any Version Profile.
- `hot-reload`: loads `style.css` and `app.js`, which sets `window.scriptVersion`. Tests copy it to a temporary folder and edit the copy.
- `dev-server`: an App for a stand-in bundler: `index.html` loads `app.js` as an ES module, which opens an HMR WebSocket and applies updates without reloading. Tests serve it from a fake dev server and copy its `config.xml` to edit it.
- `filesystem`: declares `filesystem.read`, `filesystem.write` and `mediastorage`, for `tizen.filesystem` on the App's Sandbox.
- `filesystem-read-only`: declares only `filesystem.read`.
- `network`: declares `internet` and no `<access>` element. An inline script defines `window.request(method, url, done)` (XHR) and sets `window.inlineScriptRan`. Tests copy it and write the `<access>` elements or CSP they need into the copy's `config.xml`, since the servers it calls listen on ports chosen at test time.

The Package (`.wgt`) tests build their archives from these folders at test time, so no binary Packages are checked in.
