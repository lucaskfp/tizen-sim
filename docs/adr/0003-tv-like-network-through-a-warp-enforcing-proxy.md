# TV-like network: web security off, Access Policy enforced by our proxy

On the TV, packaged Apps aren't held to browser CORS the way a web page is. In WARP mode, network access is instead governed by the `<access>` allowlist in `config.xml`, and anything not listed is denied. To reproduce that, each Engine is launched with web security disabled, an isolated profile directory and all traffic routed through our local proxy. The proxy enforces the Access Policy per origin and logs every request for the GUI. HTTPS is checked by the CONNECT host only, and traffic is never decrypted.

## Status of the premise: unverified

The first sentence above has **no primary source**. Re-checked for #44:

- **TD `guides/w3c/security/cors.md`** describes standard CORS for Tizen web apps: the `Origin` request header, `Access-Control-Allow-Origin` and preflight requests. It says nothing about packaged Apps being exempt.
- **TD `reference/security-privileges.md`** lists the `internet` privilege as what grants XHR, WebSocket and CORS. That is a privilege check, not a CORS exemption.
- **TD `guides/concepts/web-runtime.md`** describes WARP and CSP as the Access Policy, but doesn't say whether browser CORS applies on top of it.
- No Samsung TV guide fetched so far states the exemption.
- It hasn't been reproduced on the official emulator or a TV.

Until a primary source or a reproduction settles it, treat the premise as **unverified**. #32, which implements this ADR, must start by settling it. If CORS does apply on a TV, web security stays on and the proxy only adds the Access Policy on top.

Tried for #32 on a 2019 TV (UN50RU7100, Tizen 5.0) in Developer Mode, reached with `sdb`: the probe App (`<access>` to a server on the developer's machine that sends no CORS headers) couldn't be installed, because a retail TV only installs a Package signed with a Samsung distributor certificate for its DUID (`install failed[118, -12], reason: Check certificate error`). Creating that certificate needs the developer's Samsung account, so the reproduction is still pending.

#32 therefore ships with web security off, as this ADR decides, behind a single launch option (`LaunchOptions::disable_web_security`, set in `session.rs`). If a reproduction shows CORS applies, that line flips and the proxy keeps enforcing the Access Policy on top.

## Considered Options

- **Leave browser CORS on.** Blocks requests that work on a TV, if the premise holds.
- **Disable web security with no enforcement.** Simpler, but requests missing from `<access>` would succeed here and fail on the TV.

## Consequences

- Fidelity relies on the `--disable-web-security` flag continuing to work across all pinned Engines. The prototype checks this.
- If the premise is wrong, disabling web security lets cross-origin requests succeed here that fail on a TV.
