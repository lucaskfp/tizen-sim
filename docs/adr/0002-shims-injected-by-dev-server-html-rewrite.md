# Shims are injected by the dev server, not through CDP

Every App is served by our local dev server, which rewrites each HTML document so that the Shim bootstrap runs before any App script. It also answers the `$WEBAPIS/webapis/webapis.js` URL itself. We don't use CDP's `Page.addScriptToEvaluateOnNewDocument` or Tauri initialization scripts, because the former doesn't exist on M47 and the latter don't apply to external Engines (ADR-0001). HTML rewriting works the same on every Engine milestone and can be tested without a browser. CDP is reserved for the few things only it can do: trusted key events, viewport scaling, screenshots, reload, console capture, evaluating expressions in the page for the Session handle, and closing the Engine. It is never used to inject Shims.

## Consequences

- All injected JavaScript must run on M47, so the Shim bundle is compiled to ES5.
- For Dev Server URL Apps, our server sits in front of the developer's bundler as a proxy so it can rewrite HTML.
