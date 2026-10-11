# Tizen TV platform documentation

Where to find the authoritative behaviour of the platform tizen-sim emulates, and how to read it. Verified rules distilled from these sources are in `tizen-behaviour.md`.

**Scope:** only platform behaviour is emulated. Guidance on how an App should behave (UX, certification) is the App developer's concern.

**Rule for agents:** before implementing or changing any emulated platform behaviour (a Shim, a Remote Key rule, `config.xml` handling, an Access Policy rule, a Diagnostic), find the documenting page and cite it in the code comment or ADR. If the docs are silent or conflict, say so ("unverified", "docs conflict"). Never fill the gap from memory.

## Sources

| Source | What it covers | How to read it |
|---|---|---|
| [Samsung/tizen-docs](https://github.com/Samsung/tizen-docs) | Generic Tizen web runtime, `config.xml`, privileges, error model, Tizen Web Device API reference per version | `gh api repos/Samsung/tizen-docs/contents/<path> --jq .content \| base64 -d`, or `https://raw.githubusercontent.com/Samsung/tizen-docs/master/<path>`. List paths with `gh api "repos/Samsung/tizen-docs/git/trees/master?recursive=1" --jq '.tree[].path'` |
| [samsungtizenos.com/docs](https://samsungtizenos.com/docs/application/web/overview) | The same content, published (`docs.tizen.org` redirects here). Hosts the **10.0 TV** API reference | `https://samsungtizenos.com/docs/application/web/api/10.0.0/tv/device_api/tv/tizen/<module>` |
| [developer.samsung.com/smarttv](https://developer.samsung.com/smarttv/develop/) | **TV-specific** behaviour that tizen-docs lacks: Remote Keys and key codes, termination, multitasking, resolution, TV `config.xml` metadata, IME, AVPlay, Samsung Product API (`webapis`), certification checklists, web engine per year | WebFetch the pages below |

When the generic Tizen docs and the Samsung TV guides disagree, the TV guide describes what a TV does. Example: `tizenhwkey` exists in generic Tizen, but TV web apps don't get it.

## Tizen Web Device API reference

- **Path pattern:** `docs/application/web/api/<version>/device_api/tv/tizen/<module>.html` in tizen-docs.
  - Versions in the repo: 4.0 to 10.0 (4.0, 5.0 and 5.5 also carry the `device_api/device` references). Guide links to `api/latest/...` are broken.
  - Pre-5.0 shapes of an API are in the 4.0 folder; "Deprecated. Since x.y" notes are in the version that deprecated a member.
- **Anchors** look like `#Interface::member`, e.g. `tvinputdevice.html#TVInputDeviceManager::getKey`.

| Module | Page | Notes |
|---|---|---|
| Errors, callbacks | `tizen.html` | `WebAPIException`, `WebAPIError`, code constants (`SECURITY_ERR`...) |
| Application | `application.html` | `ApplicationManager`, `Application` (`exit`, `hide`, `getRequestedAppControl`), `ApplicationInformation` |
| System info | `systeminfo.html`, `systeminfo_capability_keys.html` | Property and attribute names, capability key formats |
| Remote Keys | `tvinputdevice.html` | Registration semantics and errors per method |
| Other TV modules | `tvinfo.html`, `tvwindow.html`, `tvaudiocontrol.html`, `tvdisplaycontrol.html`, `websetting.html`, `filesystem.html`, `package.html` | |

## Guides and references (tizen-docs, under `docs/application/web/`)

| Topic | Path |
|---|---|
| Web runtime: lifecycle, install rules, privilege levels, CSP vs WARP | `guides/concepts/web-runtime.md` |
| Error model | `guides/concepts/error-handling.md` |
| `config.xml` element reference | `reference/config-xml.md` |
| Tizen Studio config tabs and defaults | `guides/development/setting-properties.md` |
| Privileges and levels | `reference/security-privileges.md` |
| Deprecation schedule | `reference/deprecation-policy.md` |
| Remote Key registration (samples say `tizen.inputdevice`; on TV it's `tizen.tvinputdevice`) | `guides/text-input/input-device.md` |
| System information properties per profile | `guides/device/system-information.md` |
| App controls, `getRequestedAppControl` | `guides/app-management/app-controls.md` |
| File system | `guides/data/file-system.md` |
| CORS, other W3C features | `guides/w3c/**` |

## Samsung TV guides (developer.samsung.com/smarttv/develop/...)

| Topic | Page |
|---|---|
| Remote Keys, key codes, long presses | `guides/user-interaction/remote-control.html` |
| Input overview (`tizenhwkey` unsupported on TV) | `guides/user-interaction/user-interaction.html` |
| Keyboard / IME | `guides/user-interaction/keyboardime.html` |
| Mouse, gamepad, touch | `guides/user-interaction/{mouse,gamepad,touch}.html` |
| Terminating applications | `guides/fundamentals/terminating-applications.html` |
| Multitasking (hide, `visibilitychange`, resume) | `guides/fundamentals/multitasking.html` |
| Screen resolution | `guides/fundamentals/managing-screen-resolution.html`, `guides/base_screen_resolution.html` |
| TV `config.xml`: metadata keys, TV privileges | `guides/fundamentals/configuring-tv-applications.html` |
| Network status | `guides/fundamentals/checking-network-status.html` |
| AVPlay | `guides/multimedia/media-playback/using-avplay.html` |
| Video/audio elements, adaptive streaming | `guides/multimedia/media-playback/using-video-elements.html`, `guides/multimedia/adaptive-streaming.html` |
| Certification test cases (App requirements are out of scope; read it only for platform facts it states, e.g. what the Exit key does) | `development-checklist/mandatory-features.html`, `development-checklist/common.html` |
| Web engine (Chromium milestone) per TV year | `specifications/web-engine-specifications.html` |
| General specs (app resolution) | `specifications/general-specifications.html` |
| Samsung Product API (`webapis.*`) | `api-references/samsung-product-api-references.html`, modules at `api-references/samsung-product-api-references/<module>-api.html` |
| Tizen Web Device API index (links to samsungtizenos.com 10.0) | `api-references/tizen-web-device-api-references.html` |
