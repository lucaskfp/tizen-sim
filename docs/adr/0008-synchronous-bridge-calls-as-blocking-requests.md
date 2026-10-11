# Calls the page waits on go over the Bridge as blocking HTTP requests

Some platform APIs return the Host's answer synchronously: `tizen.filesystem`'s `openFile`, `FileHandle.readString` and `pathExists`, the pre-5.0 `File.resolve` and `FileStream.read`, and so on. The Bridge's WebSocket can't block, so for these calls a Shim sends a synchronous `XMLHttpRequest`: a JSON-RPC request `POST`ed to the Bridge's path on the dev server, with the Session's token. The reply carries the result, or the name of the WebAPIException to throw. Notifications still go over the WebSocket.

Synchronous XHR works on every Engine down to M47. M47 logs a deprecation warning for it, once per document. The Host drops Engine deprecation notices whose source is the Shim bootstrap, because the App didn't cause them.

## Considered Options

- **Preload the state into the page.** The bootstrap could carry the App's files, and writes could go back as notifications. That fails for large files, and it races, like the Host-owned key registrations ADR-0007 rejected: a write could still be in flight when the page reloads or another Session starts.
- **CDP.** The Host can't answer a call made in the page through CDP without pausing the page in the debugger, and ADR-0002 keeps CDP away from the Shims' own work.

## Consequences

- The request runs on the page's thread and blocks it, as the real synchronous APIs do on a TV.
- In CSP mode, `connect-src` is widened to the Bridge's HTTP URL as well as its WebSocket URL.
- The requests go to the dev server, so the Network Log never shows them.
- One of ADR-0007's reasons no longer holds: a Shim can now wait for the Host on M47. ADR-0007 still stands for its other reasons. Asking the page avoids an extra request on every `registerKey()`, and registrations are dropped on reload for free.
