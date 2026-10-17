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
- **Keys per release.** The guide's key table has no per-year columns, so every Version Profile gets the same Registrable Keys. That TVs before Tizen 10.0 have all of them is **unverified**.
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
- **Packages.** A `.wgt` is a zip archive packaged per the W3C Widget Packaging spec. It may carry W3C widget signatures (`author-signature.xml`, `signature1.xml`). A TV refuses a Package whose signature is invalid. tizen-sim doesn't verify signatures, because signing is out of scope. (TD `guides/concepts/web-runtime.md`, "Web application signature")
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
- **`tizen:launch_screen ready_when`:** `first-paint` (default) hides the splash screen at the "frame,rendered" callback, `complete` at "load,complete", and `custom` when the App calls `window.screen.show()`. The splash screen comes from `tizen:ls_default` (mandatory), `ls_landscape` or `ls_portrait`, each with `background_image`, `background_color`, `image` and `image_border`; the docs don't say how they're laid out. The TV guides don't mention the launch screen. Measured on a 2019 TV (Tizen 5.0, #113): `custom` without `show()` stays up for good; every App has `screen.show()`, but it hides the screen only with `custom` (under `complete` an earlier call is ignored until the load event); `first-paint` hides it before the load event; it stays up when the App goes to another page before hiding it, until that page calls `show()`, and once hidden it doesn't come back; the background image is stretched over the screen and the image is scaled to fit it, keeping its proportions, and centred; `image_border` changed nothing visible (`0px stretch`, `40px stretch`, `40px repeat` and `40px round` all looked the same).

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
- **Samsung privileges** (`http://developer.samsung.com/privilege/...`): `productinfo`, `tvinfo`, `network.public`, `drmplay`, `allshare`, `widgetdata`. The `adinfo` module page names `adinfo` too.
- **`application.info`** guards `ApplicationInformation.size`, which throws `SecurityError` when read without it. It does **not** guard `getAppsInfo` or `getAppInfo` on TV 9.0. (`application.html`)
- **`internet`** is required for XHR, WebSocket, SSE, AppCache and CORS. (TD security-privileges, W3C table)

## Network and Access Policy

- **Mode.** Any `tizen:content-security-policy`, `-report-only` or `tizen:allow-navigation` element puts the App in **CSP mode**. Otherwise it is in **WARP mode**: no external network access unless `<access origin subdomains>` allows it. Samsung's TV guide says the same: under WARP "you cannot access network resources by default" (TV: `guides/fundamentals/configuring-tv-applications.html`, "Network access policy").
- **CSP.** The default is `default-src *; script-src 'self'; style-src 'self'; object-src 'none';`. A CSP in `config.xml` replaces it. If the HTTP response also sends one, the stricter applies. Only the first occurrence of each element counts.
- **`allow-navigation`** controls link navigation and `window.open` to external URLs.
- (TD `guides/concepts/web-runtime.md`, `reference/config-xml.md`.) Measured on a Tizen 5.0 TV (ADR-0003):
  - A `config.xml` CSP is enforced in full, `connect-src` included.
  - With only `tizen:allow-navigation`, the default CSP applies: inline scripts, `eval` and external scripts are blocked; images and XHR are open.
- **Report-only alone.** The doc counts `-report-only` as a `config.xml` CSP that makes the runtime ignore the default, so an App with only a report-only policy has nothing enforced. Measured on a Tizen 5.0 TV: nothing is enforced, and the violations are only reported.
- **`tizen:allow-origin`.** Samsung's Hosted Applications FAQ (Q4) says an App that downloads content "from multiple domains" should define them in `tizen:allow-origin`. It gives no syntax, and TD `reference/config-xml.md` has no such element. A Tizen 5.0 TV installs a Package with it, but since that TV enforces no `<access>` at all, its effect couldn't be observed. tizen-sim ignores it: **unverified**.
- **`<access>` in WARP mode** follows the W3C Widget Access Request Policy (https://www.w3.org/TR/widgets-access/, linked from web-runtime.md):
  - `origin` is `*` (all access) or an IRI with only scheme and authority. An element without `origin`, or whose origin has a path, query, fragment, user info or no host, is ignored.
  - `subdomains` is a boolean that case-sensitively matches `true` or `false` after trimming. It defaults to false, and any other value is false (W3C Widget Packaging, "Boolean attribute").
  - A URL is granted when scheme, host (case-insensitive) and port (the default for the scheme when absent) match. With `subdomains="true"`, any subdomain of a domain-name host matches too. IP addresses have no subdomains.
  - The grants are the union of all `<access>` elements.
  - Which schemes the Tizen runtime supports for `<access>` is **unverified**. tizen-sim accepts http, https, ws and wss.
- **HTTPS and WebSockets** reach tizen-sim's proxy as CONNECT tunnels, which show only host and port. So a tunnel opens when any of `https`, `wss` or `ws` is granted for them, which is looser than WARP's per-scheme match. That is a simulator approximation, not TV behaviour.
- **Denied requests.** WARP says only that the user agent "rejects" a request it doesn't grant, and that the default policy denies network access "whether this access is requested through APIs (e.g. XMLHttpRequest) or through markup (e.g. iframe, script, img)". tizen-sim fails a denied request as a network error; what a TV reports is **unverified**.
- **CORS:** TD `guides/w3c/security/cors.md` describes standard CORS. A packaged App on a TV is exempt: measured on a Tizen 5.0 TV in Developer Mode (see ADR-0003). There the page's origin is `file://`, no `Origin` header is sent, and neither CORS headers nor preflights are needed.
- **Observed, not documented:** the same TV enforced neither `<access>` nor the `internet` privilege for XHR, `fetch`, `<script>`, `<img>` or WebSockets. tizen-sim follows the docs here and keeps enforcing WARP (ADR-0003).

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

## Samsung Product API

Sources: TV `api-references/samsung-product-api-references.html` and its `webapi`, `productinfo`, `tvinfo`, `network`, `adinfo` and `widgetdata` module pages.

- **Loading.** An App gets `webapis` by loading `<script src="$WEBAPIS/webapis/webapis.js">`. The page names no other way, so a page without it has no `webapis`. That `$WEBAPIS` resolves at any path depth comes from the product spec (#23).
- **Errors.** `webapis`' `WebAPIException` has tizen's constants plus `INVALID_VALUES_ERR` 26, `IO_ERR` 27, `SERVICE_NOT_AVAILABLE_ERR` 28 and `UNKNOWN_ERR` 9999. That the matching error names carry those codes is **unverified**.
- **"Since" versions** are Tizen versions; members newer than the Version Profile are absent.
- **Privileges:**
  - `productinfo`: every method needs `productinfo`, except `isDisplayRotatorSupported` (since 5.5). `setSystemConfig` lists no `SecurityError` among its exceptions but describes one for `onerror`, so tizen-sim reports it there.
  - `tvinfo`: only `setMenuValue` (since 3.0, B2B products only) needs `tvinfo`. Which exception an unknown menu key raises is **unverified**.
  - `network`: every method needs `network.public` (TV `guides/fundamentals/checking-network-status.html` too), except `setIpMode` (`b2bcontrol`), the DHCP option 60 methods (partner `network.dhcpoption60`) and the SoftAP methods (`network.softAP`).
- **`productinfo`:**
  - `ProductInfoConfigKey`: `CONFIG_KEY_DATA_SERVICE` 0, `CONFIG_KEY_SERVICE_COUNTRY` 1 (read-only). The `NotSupportedError` `setSystemConfig` throws for it is marked deprecated since 3.0 with no replacement named; tizen-sim still throws it (**unverified**).
  - `getLicensedBrand` (since 7.0) throws `NotSupportedError` on Samsung products.
  - Example values: model code `15_HAWKP`, model `UJS9500`, real model `UN65JS9500`, server version `T-INFOLINK2014-1002`.
- **`tvinfo`:**
  - `removeCaptionChangeListener` throws `InvalidValuesError` for an unknown id.
  - `getTvInfoValue` keys `SMART_RC_BATTERY` (0–100) and `BOARD_TEMPERATURE` exist since 6.5.
  - `CAPTION_LANGUAGE_INDEX_KEY` and `CAPTION_LANGUAGE_KEY` exist since 10.0. The values of the menu keys listed without numbers (`UI_SIZE`, ...) are **unverified**.
- **`network`:**
  - `NetworkActiveConnectionType`: DISCONNECTED 0, WIFI 1, CELLULAR 2, ETHERNET 3.
  - Address getters throw `InvalidStateError` without a connection; Wi-Fi getters throw it without Wi-Fi.
  - `getTVName` exists since 4.0 and `getWiFiFrequency` since 6.0. The setters (`setIp`, `setSubnetMask`, `setGateway`, `setDns`, `setIpMode`) and the proxy methods are for B2B products only, so a TV has none of them.
  - `NetworkIpMode`'s descriptions conflict (DYNAMIC is described as IPv6, AUTO as IPv4 DHCP): **docs conflict**.
- **`adinfo`** (since 2.4): `getVersion`, `getTIFA` and `isLATEnabled`, each needing `adinfo`.
  - The TIFA is a randomized, resettable device identifier for advertising. Its format is **unverified**.
  - `getTIFA` throws `InvalidStateError` "if the PSID value is null". The page doesn't say what the PSID is or when it is null.
- **`widgetdata`** (since 2.3; `read`, `write` and `remove` since 4.0): "application secure storage" of encrypted data. `write` takes a string and `read` hands one back; neither takes a key or file name.
  - Every method needs `widgetdata`. `getVersion` throws `SecurityError`. `read`, `write` and `remove` throw only `TypeMismatchError` and pass `SecurityError` to `onerror`, as they do `NotFoundError` (nothing stored, for `read` and `remove`) and `DOMStringSizeError` (`write` data over 20000 characters).
  - `read`'s `onsuccess` is required; the other callbacks are optional and nullable.
  - Whether the data survives a relaunch, and whether clearing the App's data removes it, is **unverified**: the page doesn't say. tizen-sim keeps it in the App's Sandbox, like the App's files in `wgt-private`.

## File system

Sources: `filesystem.html` in the 4.0 TV reference (before the 5.0 rework), the 5.0 one (deprecations) and the 9.0 one (after it); TD `guides/data/file-system.md`. **Measured** below means checked with a probe App on a Tizen 5.0 TV (UN50RU7100, 2019), which has both shapes.

- **Two shapes.** Up to 4.0, `tizen.filesystem.resolve()` hands out `File` objects, and `File.openStream()` hands out `FileStream`s. 5.0 adds `openFile()`/`FileHandle` and path-based methods (`createDirectory`, `copyFile`, `listDirectory`, `pathExists`, ...), and deprecates `resolve`, `File` and `FileStream`. The 6.0 reference no longer has them. `getStorage`, `listStorages` and the storage listeners exist throughout.
  - Measured: calling `resolve` on 5.0 logs no deprecation warning.
- **Virtual roots.** Paths are `<root>/<path>`. The roots are `documents`, `downloads`, `images`, `music`, `videos`, `camera`, `wgt-package` (the App's files, read-only), `wgt-private` and `wgt-private-tmp`. `ringtones` isn't supported on TV (file-system guide). Measured:
  - `ringtones` gives `InvalidValuesError` (`NotFoundError` from `resolve`).
  - File URIs and absolute paths (`file:///tmp`, `/tmp`) work. tizen-sim keeps each App in its Sandbox, so these fail and raise the `filesystem-outside-sandbox` Diagnostic.
  - `wgt-private` also holds the Engine's own files (`.appdb.db`, `.cookie`, `chromium-efl`). tizen-sim doesn't put them there.
- **Persistence.** Measured: `wgt-private` survives the App closing and relaunching, and `wgt-private-tmp` comes back empty. The 4.0 `resolve` doc says it holds data "available during a widget execution cycle". tizen-sim empties it when a Session starts.
- **Privileges.** Each method names `filesystem.read` and/or `filesystem.write`. `openFile` needs write for modes `a`, `rw`, `rwo`, `w` and read for `r`, `rw`, `rwo`.
  - Measured: both are enforced. Without them, every method throws `SecurityError` except `toURI` and `getDirName`.
  - From 5.0, the reference adds that internal storage needs `mediastorage`. Measured: the TV **doesn't enforce it**; an App without it reads and writes `documents`. tizen-sim follows the reference and requires it for the media roots, as it does for the `internet` privilege the same TV also doesn't enforce (ADR-0003).
- **`resolve(location, ..., mode)`:** `mode` defaults to `rw`. A writable mode on a read-only root goes to the error callback as `InvalidValuesError`, and an unknown location as `NotFoundError`. Measured:
  - `.` or `..` in the location gives `InvalidValuesError`.
  - The mode is enforced on the `File`s it hands out, including children it resolves. With `r`, `createFile`, `createDirectory`, `deleteFile` and `openStream` in a writable mode fail with `InvalidValuesError`, but `copyTo` works. That `moveTo` and `deleteDirectory` are refused too is **unverified**.
- **`File`** (measured):
  - A root resolved by name has `path` and `fullPath` equal to its name and `parent` null. A root reached as a child's `parent` has both ending in `/` (`documents/`).
  - `createDirectory` on an existing directory gives `InvalidValuesError`, though the reference says `IOError`.
  - `createFile` gives `IOError` for an existing file and `UnknownError` for a missing parent directory.
- **`FileStream`** (measured):
  - `openStream('w')` empties the file.
  - Reading a `w` or `a` stream, or writing an `r` one, throws `IOError`, and so does any use after `close()`.
  - `eof` turns true only when a read starts at the end. Reading exactly up to the end leaves it false, with `bytesAvailable` 0; after that, `bytesAvailable` is -1.
  - An unsupported encoding called neither callback within 4s; tizen-sim reports `InvalidValuesError` to the error callback (**unverified**).
- **`openFile` modes.** `r` and `rw` throw `NotFoundError` for a missing file. `w` and `rwo` create or empty it, and `a` creates it. `makeParents` defaults to true. Measured:
  - An invalid path gives `NotFoundError`.
  - A directory opens in `r`; reading it then fails with `IOError`.
  - In `a`, `seek` moves the position, but writes still go to the end.
  - Reading a `w` handle or writing an `r` one throws `IOError`, as do seeking before the start and closing twice.
- **Path methods** (measured):
  - `pathExists` with `.` or `..` gives `false`.
  - `listDirectory` on a file gives `IOError`.
  - `copyFile` into a missing directory gives `IOError`.
  - `copyDirectory` without `overwrite` stops at the first conflict, keeping what it already copied.
  - Deleting a virtual root gives `IOError`.
  - `getDirName` returns a path without `/` unchanged, keeps `/` as `/`, and drops trailing and doubled slashes (`/a/b/` gives `/a`).
- **Storages.** Measured: `listStorages` reports `internal0` and then the virtual roots, all `INTERNAL` and `MOUNTED`. `maxNameLength` is 255 and `maxPathLength` 4096.
- **`toURI`** returns a `file://` URI on a TV: `file:///opt/usr/home/owner/content/Documents/...` for `documents`, and `.../apps_rw/<package>/data/...` for `wgt-private` (measured). A page served over HTTP can't load one, so tizen-sim returns its dev server's URL for the file.

## Screen and media

- **Resolution.** The App is 1920×1080 CSS px on all models, or 1280×720 on FHD TVs per the resolution guide. Larger content is scaled down. A custom width comes from the viewport meta. The guide names `screen.width`/`height` and the system info `DISPLAY` property as how an App reads the resolution. (TV: `guides/fundamentals/managing-screen-resolution.html`, `specifications/general-specifications.html`)
  - What `DISPLAY.resolutionWidth`/`Height` and `devicePixelRatio` report on an FHD TV is **unverified**.
- **Transparent page areas** show black on a TV. That comes from the product spec (#23); no guide fetched so far states it. **Unverified.**
- **AVPlay** (TV: `guides/multimedia/media-playback/using-avplay.html`; `api-references/samsung-product-api-references/avplay-api.html`):
  - States go NONE → IDLE (`open`) → READY (`prepare`) → PLAYING/PAUSED. `stop()` returns to IDLE and keeps the settings, so `prepare()` and `play()` replay; `close()` returns to NONE.
  - Each method lists the states it "can be called in"; outside them it throws `InvalidStateError`. `seekTo`, `jumpForward`/`jumpBackward` and `prepareAsync` report errors to their error callback, and the App must wait for a seek's callback before calling anything else. `pause()` during a seek throws `InvalidAccessError`.
  - `prepare()`/`prepareAsync()` invoke `onbufferingstart` and the player enters READY; `play()` invokes `oncurrentplaytime` (milliseconds). The order of `onbufferingprogress`/`onbufferingcomplete`, how often `oncurrentplaytime` fires, and the state after `onstreamcompleted` are **unverified**.
  - `setDisplayRect` always uses 1920×1080 coordinates, whatever the App's resolution. Negative values throw `InvalidValuesError`. That no video shows before the first `setDisplayRect` is **unverified**.
  - By default video fills the display area; `PLAYER_DISPLAY_MODE_LETTER_BOX` fits it.
  - `setSpeed` takes -16, -8, -4, -2, 1, 2, 4, 8 or 16. A time buffer in `setBufferingParam` is at least 4 seconds.
  - `setDrm` and `getUID` need the `drmplay` privilege; `getUID` is deprecated since 7.0. No other method names a privilege, and the `avplay` privilege is "not used in TV Web applications".
  - Whether the Access Policy applies to AVPlay's streams is **unverified**. tizen-sim plays them in a `<video>`, so it does.
  - `getDuration` and `getCurrentTime` are in milliseconds.
- **Media formats.** Each TV year's video specifications have one table per model group: containers, video codecs, audio codecs (TV `specifications/media-specifications/<year>-tv-video-specifications.html`, 2017 = Tizen 3.0 to 2026 = Tizen 10.0). The media specifications index adds a Music table without years (MP3, AAC in `.m4a`/`.aac`, FLAC, Ogg Vorbis, WAV, WMA, ...; WMA is gone from 24TV_BASIC4 and from 2025 on) and the MSE section, which names only WebM VP9 and "H.264 codec supports up to FHD". The AVPlay and `<video>` guides name no formats and point to these pages. Read off all ten pages (#105), for the formats a web App can name in a MIME type; `crates/tizen-sim-core/data/media.toml` has them per year, generated by `data/media-specs.py`:
  - Every year, every model group: MP4, MOV, 3GP, MKV, MPEG-TS, MPEG-PS, AVI, ASF and FLV with H.264, HEVC, MPEG-4 Part 2, AAC, HE-AAC, AC-3, E-AC-3 (DD+) and MP3; WebM with VP8 and Vorbis. From 2018: AC-4, MPEG-H and Opus (in MP4 and the like; WebM lists only Vorbis). 2017 only: DTS.
  - Varies by model group: VP9 is missing on the FHD groups of 2017 to 2020 (on every group from 2021); AV1 first appears in 2020, missing on 20TV_BASIC2 (FHD), then on every group from 2021; in 2024 WMA, VC-1 and WMV are missing on 24TV_BASIC4 (FHD). Within a group, HEVC profiles (Main, Main10) and resolution, frame rate and bitrate limits vary.
  - AVPlay also plays MPEG-DASH (`.mpd`), Smooth Streaming (`.ism/Manifest`) and HLS (`.m3u8`) (TV `guides/multimedia/adaptive-streaming.html`). Whether `<video>` plays them natively isn't stated: **unverified**.
  - Which of these formats a TV's MSE takes, beyond H.264 and WebM VP9, is **unverified**.
- **What the Engines play.** Measured with `canPlayType` and `MediaSource.isTypeSupported` on every Windows Engine (#105):
  - None plays MOV, MKV, MPEG-TS, MPEG-PS, AVI, ASF, FLV, WMA, AC-3, E-AC-3, AC-4, MPEG-H, DTS or MPEG-4 Part 2, nor HLS or DASH natively. All play WebM with VP8, VP9, Vorbis and Opus, Ogg and WAV.
  - The Chromium snapshots (Tizen 3.0 to 8.0) have no H.264, AAC, HE-AAC or HEVC. M47 to M63 (3.0 to 5.0) play no MP4 at all, nor MP3 (FLAC from M63); from M69 (5.5) MP4 with VP9, then AV1 and Opus from M76 (6.0), and MP3.
  - Chrome for Testing (9.0, 10.0) plays H.264, AAC, HE-AAC and HEVC in MP4 and 3GP; HEVC likely through the computer's own decoder (**unverified** on other hardware). Played for real with a 3-second H.264 Constrained Baseline/AAC-LC MP4 in `<video>`, MSE and AVPlay.
  - The Linux Engines are **unverified** locally: they come from the same two sources, and Chromium's `proprietary_codecs` build flag ("Enables proprietary codecs and demuxers; e.g. H264, AAC, MP3, and MP4", `build/config/features.gni`) is on only for Chrome-branded and Chrome for Testing builds. The nightly per-Engine suite checks them.
  - No Chromium build with these codecs exists for the snapshot milestones (ADR-0001). So tizen-sim raises the `media-format-unsupported` Diagnostic when the App asks about (`canPlayType`, `isTypeSupported`), adds (`addSourceBuffer`) or plays (`<video>`, `<audio>`, `<source>`, AVPlay) a format its Version Profile's TVs list and its Engine can't play, naming the format, what the Engine lacks and whether only some model groups play it. A failed stream's format comes from its `<source type>` or its URL's extension; others go unreported. The Engine's own answers are left as they are, and AVPlay shows its test pattern.
- **Video plane.** The rule that the AVPlay video sits behind the page comes from the user's specification (ADR-0004). The guides fetched so far don't state it explicitly. The guide's only related rule: the `<object type="application/avplayer">` element's `style` must follow `setDisplayRect` changes.

## Engine features

- **Milestones.** Each Tizen TV release ships a fixed Chromium milestone (TV: `specifications/web-engine-specifications.html`), mirrored in `crates/tizen-sim-core/data/profiles.toml`. Which web features an App can use follows from that milestone.
- **User agent.** Each TV release sends its own user agent string, listed per year in TV `guides/fundamentals/retrieving-platform-information.html` (Table 2), e.g. Tizen 3.0: `Mozilla/5.0 (SMART-TV; LINUX; Tizen 3.0) AppleWebKit/538.1 (KHTML, like Gecko) Version/3.0 TV Safari/538.1`. Mirrored per Version Profile in `profiles.toml`.
- **API availability.** Each member of the API references has a "Since" version; members newer than the Version Profile are absent. Those of Shimmed APIs that some profile lacks are listed in `crates/tizen-sim-core/data/apis.toml`. Deprecated APIs keep working for two releases, the notice included, then are removed (TD `reference/deprecation-policy.md`), logging "DEPRECATION WARNING" messages in the meantime; the exact console wording for web APIs is **unverified**.
- **Web features per year.** The same page (TV `specifications/web-engine-specifications.html`, "Web Standard Feature Support") lists which web features each TV year has, one table per area (JavaScript (ES6), Device, ... Miscellaneous Features). Samsung's engine lacks some features that upstream Chromium has at the milestone it ships. Mirrored per Version Profile in `crates/tizen-sim-core/data/web_features.toml`, generated by `web-features.py`; the CSS tables that follow aren't mirrored.
  - Measured on every Windows Engine (#106): the Engines have several features the page says their TVs lack. On 3.0 these are `Promise`, `Map`, `Symbol`, `WeakMap`, `WeakSet`, `Object.assign`, ES2015's `Number` methods, `requestIdleCallback`, the Push API, WebRTC, the Geolocation API and User Timing. On 4.0 they are Proxy, Reflect, `copyWithin`, Geolocation, `setSinkId`, IntersectionObserver and WebRTC. `MouseEvent.buttons` is on every Engine, and the `autocapitalize` property from 5.5; the page says both are missing up to 9.0.
  - tizen-sim follows the page. Before any App script, the Shims delete what an App reaches of each feature its Version Profile's TVs lack: globals, and members of prototypes. Workers and `about:blank` frames keep them. Syntax (classes, `let`, destructuring...), events, HTTP headers and TLS can't be removed, nor can ES2015's `Number` constants (not configurable). These go unenforced, with no Diagnostic either, except for ES modules (below). Preload, Shadow DOM v1 on 3.0 and PNaCl on 10.0 need nothing: no Engine has them where the page says their TVs lack them.
  - On the M47 Engine, `class`, `let` and destructuring are syntax errors outside strict mode, which matches the page's "No" for 3.0. The page's other 3.0 syntax rows (default and rest parameters, `for-of`, generators, `new.target`, octal and binary literals, template literals) weren't measured, and what 3.0 TVs do with them is **unverified**. Whether 3.0 TVs really lack the builtins the M47 Engine has (`Promise`, `Map`...) is **unverified**: the page says they do, though it also names M47 as their engine.
  - Rows the table leaves ambiguous are **unverified** and not applied. **Custom Elements** doesn't say which version: it is No on 6.5 to 9.0 and Yes on the other years, while the Engines have v1 (`customElements`) from 4.0 on and v0 (`document.registerElement`) only up to 6.0. **HTML Imports** (No on 6.5 to 9.0) and **Shadow DOM v0** (No on 10.0) match the Engines, which have both only up to 6.0, except that the page also says Yes on 10.0 for HTML Imports. **Template literals** is partial on 5.0 and 10.0 but full in between. **input event** is No on 3.0, between Yes on 2.4 and partial from 4.0. **Content Security Policy Level 1** is No on 4.0 while Level 2 is Yes. The page doesn't say what "partially" or "basic support" leaves out; such rows keep everything.
  - The Performance table has a stray cell after Resource Timing. Its spans still give 3.0 a No for Resource Timing and User Timing, and tizen-sim applies that. Resource entries still come from `performance.getEntries()`; whether a 3.0 TV returns them is **unverified**.
  - The page says **Module objects** is No on 5.0 (M63), though Chromium has had ES modules since 61 (see below). Whether 5.0 TVs run module scripts is **unverified**. tizen-sim raises the `es-modules-unsupported` Diagnostic there too, saying so.
- **No favicon.** Measured on a Tizen 5.0 TV (UN50RU7100) with a probe App that ships no favicon: the App loads from `file://` (`file:///index.html`, the Package at `/opt/usr/apps/<package>/res/wgt/`), and no request for `/favicon.ico` is made, while the App's own request for a missing file is logged as `Failed to load resource: net::ERR_FILE_NOT_FOUND`. The M63 Engine on the desktop also fetches no favicon for a `file://` page, but does for the same page over HTTP. tizen-sim serves Apps over HTTP, so it answers the Engine's favicon request with 204 when the App has none (#53).
- **ES modules** arrived in Chromium 61 (chromestatus.com/feature/5365692190687232). Older Engines ignore `<script type="module">` and run `<script nomodule>`. A document with module scripts and no `nomodule` fallback, such as a bundler's dev output, runs none of its App code on those TVs. tizen-sim raises the `es-modules-unsupported` Diagnostic for it. That Tizen TVs before M61 behave exactly like upstream Chromium here is **unverified**.

## Text input (IME)

Source: TV `guides/user-interaction/keyboardime.html`.

- **Showing the virtual keyboard.** Focusing an `<input>`/`<textarea>` shows it. A focus set by script doesn't show it when `use.keypad.without.useraction` is false, and `use.ime=false` disables it.
- **IME key codes:** Done 65376, Cancel 65385, other IME keys 229. With a physical keyboard, Enter sends Done and Esc sends Cancel.
