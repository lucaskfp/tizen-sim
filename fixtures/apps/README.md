# Fixture Apps

Tiny Tizen TV Apps used by the test suites. Each folder is a Project Folder App Source, named after the behaviour it exercises.

- `hello`: a valid App that logs to the console on load.
- `missing-config`: no `config.xml` at all.
- `malformed-config`: `config.xml` that isn't well-formed XML.
- `no-application`: valid XML without the `tizen:application` element.
- `missing-entry-point`: `<content src>` names a file that doesn't exist.
- `tizen-apis`: an App that declares no privileges, for the Tizen API Shims. Its first script records whether `tizen` already existed.
- `tizen-apis-privileged`: the same App, declaring `http://tizen.org/privilege/application.info`.
- `remote-keys`: declares `tv.inputdevice` and records every keydown `keyCode` in `window.keyLog` and every keyup `keyCode` in `window.keyUpLog`.
- `background-support`: enables `background-support` with a `media` background category, so it keeps running while hidden.
- `required-version-too-high`: declares `required_version="99.0"`, newer than any Version Profile.
