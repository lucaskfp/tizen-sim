# Tizen TV behaviour the simulator must honour

What the **platform** does, distilled from the platform docs (see `tizen-docs.md` for where each source lives).

- **Scope.** Only platform behaviour belongs here: what the runtime, the remote and the APIs do. How an App *should* behave is the App developer's concern and out of scope: Samsung's UX guidelines and certification checklist (exit popups, focus rules, launch-time targets, "check the network on resume", and so on).
- **Sources.** Each rule cites its source. `TV:` means developer.samsung.com/smarttv/develop/; `TD:` means tizen-docs `docs/application/web/`; API pages are in the 9.0 TV reference.
- **What this file is not.** It records how the platform behaves, not what tizen-sim already implements. The issue tracker tracks gaps.
- **Unverified items.** Items marked **unverified** need a primary source or a real TV before code depends on them.

## Remote Keys

- **Registration.** Arrows, Enter and Back always reach the App. Every other key needs `tizen.tvinputdevice.registerKey`/`registerKeyBatch` first; an unregistered key produces neither keydown nor keyup. (TV: `guides/user-interaction/remote-control.html`; TD: `guides/text-input/input-device.md`)
- **Privilege.** Every `tvinputdevice` method requires `http://tizen.org/privilege/tv.inputdevice`. (`tvinputdevice.html`)
- **`getSupportedKeys`** omits Mandatory Keys.
- **`getKey(name)`** returns `null` only for unsupported keys, and throws `InvalidValuesError` for an invalid name such as `""`. Whether it returns Mandatory Keys is **unverified**; the doc implies it does. (`tvinputdevice.html#TVInputDeviceManager::getKey`)
- **`registerKey`/`unregisterKey`:** a Mandatory or unsupported key throws `InvalidValuesError`.
- **Batch methods:** a non-array or non-function argument throws `TypeMismatchError` synchronously. Invalid names go to the error callback. (`#TVInputDeviceManager::registerKeyBatch`)
- **Key codes.** The full table is in the remote-control guide, Table 3, and is mirrored in `crates/tizen-sim-core/data/remote_keys.toml`.
- **Long presses:**
  - Rewind gives MediaTrackPrevious and FastForward gives MediaTrackNext. (remote-control guide, Table 1)
  - Holding Return/Exit makes the **platform terminate the App**; the App gets no key. (TV: `guides/fundamentals/terminating-applications.html`)
- **Exit key:** the platform closes the App and shows the TV channel screen, even during playback. An open IME is closed first. (TV: `development-checklist/common.html` CO-US-04; CO-GE-07: "When the return or exit key is clicked, the application is terminated.")
- **`tizenhwkey`.** TV web apps don't get the generic Tizen `tizenhwkey` back event. (TV: `guides/user-interaction/user-interaction.html`)
- **`KeyboardEvent.key`** is not documented anywhere. Only `keyCode` is. One community report from Tizen 5.5 gives `key: "XF86Back"` with `code: "Escape"` for Back. **Unverified.**

## Lifecycle

- **Leaving the App** (Home/Smart Hub key, another App, source change, Instant-On power off) pauses JavaScript and rendering, including CSS animations.
  - `visibilitychange` fires with `document.hidden === true`.
  - On return it fires again and execution resumes.
  - `visibilitychange` also fires when the App exits.
  - (TV: `guides/fundamentals/multitasking.html`; TD: `guides/concepts/web-runtime.md`)
- **`getCurrentApplication().hide()`** sends the App to the background, with the same events. (`application.html#Application::hide`)
- **Background support.** `<tizen:setting background-support="enable"/>`, together with a background category, keeps it running in the background; the default is disabled. With `background-support` alone, "a web application process can be suspended in the background", so tizen-sim suspends it. (TD: `reference/config-xml.md`)
- **`getRequestedAppControl()`** never returns `null`. The docs don't say what a plain launch from Smart Hub reports. The `http://tizen.org/appcontrol/operation/default` operation comes from the native app-control docs (`APP_CONTROL_OPERATION_DEFAULT`), and the `callerAppId` value is **unverified**. (`application.html#Application::getRequestedAppControl`)
- **`getAppContext(contextId?)`** returns `{id, appId}`; without an id, the calling App's context; an unknown id throws `NotFoundError`. (`application.html#ApplicationManager::getAppContext`)
- **After exit,** no state is kept. JavaScript stops, and the next launch starts fresh. (TV: terminating-applications)

## `config.xml`

Sources: TD `reference/config-xml.md`, TD `guides/development/setting-properties.md`, TV `guides/fundamentals/configuring-tv-applications.html`.

- **Root and file.** `config.xml` is required at the package root, with a W3C `<widget>` root.
- **`<widget version>`** has the form `[0-255].[0-255].[0-65535]`.
- **Exactly one `<tizen:application>`,** and all of its attributes are mandatory:
  - `package`: 10 alphanumeric characters.
  - `id`: `<package>.<name>`, where the name is 1–52 alphanumeric bytes.
  - `required_version`: a float such as `2.4`. Installation is refused when the platform's version is lower.
  - `launch_mode` (optional): `single` (default), `group` or `caller`, from version 2.4.
- **Start page.** `<content src>` defaults to `index.html`. `<tizen:content src>` points at a hosted start page instead. Only the first occurrence counts.
- **`<tizen:setting>` defaults:**
  - `context-menu`: enable
  - `encryption`: disable
  - `screen-orientation`: portrait
  - `background-support`: disable
  - `install-location`: auto
  - TV adds `pointing-device-support`; `disable` turns the mouse off.
- **TV metadata keys** (`http://samsung.com/tv/metadata/...`) that change runtime behaviour:
  - `prelaunch.support`
  - `use.network`
  - `use.preview`
  - `gamepad`
  - `use.voiceguide`
  - `use.ime`, `use.keypad.without.useraction`
- **Size limits.** `<preference>` (name ≤ 80 bytes, value ≤ 8192 bytes, `readonly` honoured) is exposed to the App through the widget interface. `tizen:metadata` has the same limits and is read-only.
- **`tizen:launch_screen ready_when`:** `first-paint` (default), `complete`, or `custom`. With `custom`, the screen waits for `window.screen.show()`.

## Privileges

Sources: TD `reference/security-privileges.md`, TD `guides/concepts/web-runtime.md`, TV configuring-tv-applications.

- **Levels:** public, partner, platform.
  - A public App that declares a partner- or platform-level privilege fails to install. The Web API table lists nine of them: `appmanager.certificate`, `appmanager.kill`, `networkbearerselection`, `systemmanager`, `tee.client` (partner); `bluetoothmanager`, `bookmark.read`, `bookmark.write`, `packagemanager.install` (platform).
  - Calling an API whose privilege isn't declared throws `SecurityError` at runtime.
- **TV-relevant public privileges:**
  - `tv.inputdevice`, `tv.window`, `tv.display`
  - `application.info`, `application.launch`, `package.info`
  - `internet`, `filesystem.read`, `filesystem.write`, `content.read`, `content.write`, `download`, `alarm`, `power`, `system`, `fullscreen`, `mediastorage`, `externalstorage`
  - `tv.audio` (deprecated since 5.0)
- **Samsung privileges** (`http://developer.samsung.com/privilege/...`): `productinfo`, `tvinfo`, `network.public`, `drmplay`, `allshare`, `widgetdata`.
- **`application.info`** guards `ApplicationInformation.size`, which throws `SecurityError` when read without it. It does **not** guard `getAppsInfo` or `getAppInfo` on TV 9.0. (`application.html`)
- **`internet`** is required for XHR, WebSocket, SSE, AppCache and CORS. (TD security-privileges, W3C table)

## Network and Access Policy

- **Mode.** Any `tizen:content-security-policy`, `-report-only` or `tizen:allow-navigation` element puts the App in **CSP mode**. Otherwise it is in **WARP mode**: no external network access unless `<access origin subdomains>` allows it.
- **CSP.** The default is `default-src *; script-src 'self'; style-src 'self'; object-src 'none';`. A CSP in `config.xml` replaces it. If the HTTP response also sends one, the stricter applies. Only the first occurrence of each element counts.
- **`allow-navigation`** controls link navigation and `window.open` to external URLs.
- (TD `guides/concepts/web-runtime.md`, `reference/config-xml.md`)
- **CORS:** TD `guides/w3c/security/cors.md` describes standard CORS. ADR-0003's premise that packaged Apps skip browser CORS has **no primary source yet**; resolve it before #32.

## Errors

- **Two error types.** Synchronous errors are `WebAPIException`; asynchronous errors are `WebAPIError` passed to an error callback. (TD `guides/concepts/error-handling.md`, `tizen.html`)
- **Codes.** DOM-named errors use the legacy DOMException codes:
  - NotFoundError 8, NotSupportedError 9, InvalidStateError 11, TypeMismatchError 17, SecurityError 18, NetworkError 19, AbortError 20, QuotaExceededError 22, TimeoutError 23.
  - Tizen-only names (InvalidValuesError, IOError, ServiceNotAvailableError, UnknownError, VerificationError) use 0.
- **Constants.** `WebAPIException` exposes the constants `INDEX_SIZE_ERR` (1) … `DATA_CLONE_ERR` (25).
- **Deprecations.** Deprecated APIs keep working for two releases and are removed in the third, logging a "DEPRECATION WARNING". (TD `reference/deprecation-policy.md`)

## System info

Source: `systeminfo.html`, `systeminfo_capability_keys.html`, TD `guides/device/system-information.md`.

- **`getPropertyValue` errors.** A string that isn't a `SystemInfoPropertyId` throws `TypeMismatchError` synchronously. A valid but unsupported property goes to the error callback as `NotSupportedError`. `getPropertyValueArray` and `getCount` validate the same way; `getCount` returns 0 for an unsupported property.
- **Change listeners.** `addPropertyValueChangeListener` returns an id. A listener on a property that never changes (e.g. BUILD) is accepted but never invoked. PANEL can't be watched: the error callback gets `NotSupportedError`. `removePropertyValueChangeListener` throws `InvalidValuesError` for invalid input; whether an unknown id counts is **unverified**, so tizen-sim ignores it.
- **`getDeviceUptime()`** (since 8.0) returns seconds since boot.
- **Properties on TV:** BUILD, CPU, DISPLAY, LOCALE, MEMORY, NETWORK, WIFI_NETWORK, ETHERNET_NETWORK, NET_PROXY_NETWORK, PERIPHERAL, STORAGE, VIDEOSOURCE, ADS, SERVICE_COUNTRY, SOURCE_INFO, PANEL.
- **`LOCALE`:** both `language` and `country` are `LANGUAGE_REGION`, e.g. `en_US`.
- **`http://tizen.org/feature/platform.version`** has the form `Major.Minor.Patch`, e.g. `9.0.0`.
- **`getCapability` with an unknown key:** the doc lists only `UnknownError`. **Unverified** on hardware.

## Screen and media

- **Resolution.** The App is 1920×1080 CSS px on all models, or 1280×720 on FHD TVs per the resolution guide. Larger content is scaled down. A custom width comes from the viewport meta. (TV: `guides/fundamentals/managing-screen-resolution.html`, `specifications/general-specifications.html`)
- **AVPlay:**
  - States go NONE → IDLE (`open`) → READY (`prepare`) → PLAYING/PAUSED. `stop()` returns to IDLE; `close()` returns to NONE.
  - `setDisplayRect` always uses 1920×1080 coordinates.
  - `setDrm` and `getUID` need the `drmplay` privilege.
  - (TV: `guides/multimedia/media-playback/using-avplay.html`)
- **Video plane.** The rule that the AVPlay video sits behind the page comes from the user's specification (ADR-0004). The guides fetched so far don't state it explicitly.

## Text input (IME)

Source: TV `guides/user-interaction/keyboardime.html`.

- **Showing the virtual keyboard.** Focusing an `<input>`/`<textarea>` shows it. A focus set by script doesn't show it when `use.keypad.without.useraction` is false, and `use.ime=false` disables it.
- **IME key codes:** Done 65376, Cancel 65385, other IME keys 229. With a physical keyboard, Enter sends Done and Esc sends Cancel.
