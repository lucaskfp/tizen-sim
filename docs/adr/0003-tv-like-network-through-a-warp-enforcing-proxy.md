# TV-like network: web security off, Access Policy enforced by our proxy

On the TV, packaged Apps aren't held to browser CORS the way a web page is. In WARP mode, network access is instead governed by the `<access>` allowlist in `config.xml`, and anything not listed is denied. To reproduce that, each Engine is launched with web security disabled, an isolated profile directory and all traffic routed through our local proxy. The proxy enforces the Access Policy per origin and logs every request for the GUI. HTTPS is checked by the CONNECT host only, and traffic is never decrypted.

## Considered Options

- **Leave browser CORS on.** Blocks requests that work on a TV.
- **Disable web security with no enforcement.** Simpler, but requests missing from `<access>` would succeed here and fail on the TV.

## Consequences

- Fidelity relies on the `--disable-web-security` flag continuing to work across all pinned Engines. The prototype checks this.
