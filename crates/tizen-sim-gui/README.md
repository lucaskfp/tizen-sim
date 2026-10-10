# tizen-sim GUI

The desktop GUI: a Tauri v2 app with a React, TypeScript, Tailwind and shadcn/ui frontend. It starts a Session for a Project Folder, a Package or a Dev Server URL, and shows its log, Network Log and Diagnostics, with an on-screen remote, a Version Profile switcher, reload and screenshot. The Engine opens in its own window.

The Rust side (`src/`) drives Sessions through `tizen-sim-core`; the frontend (`ui/`) only shows the Session's event stream and sends commands. Only the GUI's own window has IPC (`capabilities/main.json`).

## Running it

```bash
npm ci
npm run tauri dev
```

`npm run tauri build` builds a release binary that serves the built frontend. A plain `cargo build` builds a binary that loads the frontend from the Vite dev server (`npm run dev`).

## Tests

- `cargo test -p tizen-sim-gui`: the Session control against real headless Sessions, and the IPC permissions on Tauri's mock runtime.
- `npm test`: smoke tests of the frontend against a stand-in for the Rust side.
