# TV-like network: web security off, Access Policy enforced by our proxy

On the TV, packaged Apps aren't held to browser CORS the way a web page is. In WARP mode, network access is instead governed by the `<access>` allowlist in `config.xml`, and anything not listed is denied. To reproduce that, each Engine is launched with web security disabled, an isolated profile directory and all traffic routed through our local proxy. The proxy enforces the Access Policy per origin and logs every request for the GUI. HTTPS is checked by the CONNECT host only, and traffic is never decrypted.

## Status of the premise: reproduced on one TV

No document states the premise:

- **TD `guides/w3c/security/cors.md`** describes standard CORS for Tizen web apps: the `Origin` request header, `Access-Control-Allow-Origin` and preflight requests. It says nothing about packaged Apps being exempt.
- **TD `reference/security-privileges.md`** lists the `internet` privilege as what grants XHR, WebSocket and CORS. That is a privilege check, not a CORS exemption.
- **TD `guides/concepts/web-runtime.md`** describes WARP and CSP as the Access Policy, but doesn't say whether browser CORS applies on top of it.
- No Samsung TV guide fetched so far states the exemption.

For #32 it was reproduced on a 2019 TV (UN50RU7100, Tizen 5.0) in Developer Mode:

- **Setup.** A probe App was signed with a locally generated author certificate and the Tizen SDK's public distributor certificate; Developer Mode accepts that. An unsigned Package is refused (`Check certificate error`). It was installed and launched with `sdb`, and read through the Web Inspector.
- **The page** is `file:///index.html` with origin `file://`.
- **CORS:** XHR and `fetch` read responses from servers that send no CORS headers, over HTTP and HTTPS. A `POST` with a JSON body went through without a preflight (the server answers `OPTIONS` with 405). The TV sends no `Origin` header at all.
- **The premise holds on that TV,** so the Engine runs with web security off (`LaunchOptions::disable_web_security`, set in `session.rs`).

The same TV did **not** enforce the Access Policy either. With origins missing from `<access>`, with no `<access>` element at all, and even without the `internet` privilege, XHR, `fetch`, `<script>`, `<img>` and `wss://` WebSockets all succeeded. Whether that is down to Developer Mode, the Tizen version or the firmware is unknown. tizen-sim still enforces WARP as documented, by the project owner's decision: an App that relies on undeclared access is wrong by the platform docs, even if one TV lets it through.

CSP mode, measured on the same TV, does behave as documented, and as the dev server emulates it:

- **A `config.xml` policy** (`default-src 'self'; connect-src https://example.com`) is enforced in full. It blocks the inline script, `eval`, the external script, the image and XHR to other hosts, and lets XHR to `example.com` through.
- **With only `tizen:allow-navigation`,** the default policy applies. It blocks inline scripts, `eval` and external scripts, and leaves images and XHR open.
- **With only a report-only policy,** nothing is enforced, and the violations are reported.
- **A Package with Samsung's `tizen:allow-origin`** installs. Its effect couldn't be observed, since this TV enforces no `<access>`.

## Considered Options

- **Leave browser CORS on.** Blocks requests that work on a TV, if the premise holds.
- **Disable web security with no enforcement.** Matches the TV measured above, but requests missing from `<access>` would succeed here although the docs say a TV denies them.

## Consequences

- Fidelity relies on the `--disable-web-security` flag continuing to work across all pinned Engines. The prototype checks this.
- If other TVs or firmware do apply CORS, disabling web security lets cross-origin requests succeed here that fail there.
- Enforcing WARP is stricter than the TV measured above, so an App can fail here and work on that TV.
