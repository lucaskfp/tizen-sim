# Sessions are owned by whoever starts them; no background service

A Session (Engine process, dev server, proxy, watcher, CDP connection) is owned by the process that started it: a foreground `tizen-sim run` or the GUI. Ctrl+C or closing the GUI cleans it up. Each Session announces itself in a per-user Session Registry with its control endpoint and an access token. CLI commands such as `key`, `reload` and `screenshot` reach a running Session through the Registry, and the GUI can adopt Sessions the CLI started.

## Considered Options

- **A per-user background service owning all Sessions.** Uniform, but it brings auto-start, upgrade and orphan-process problems that a developer tool doesn't need.

## Consequences

- Registry entries can go stale when an owner crashes, so clients must check that the owning process is alive before using an entry.
