# tizen-sim

An open-source desktop simulator for **Samsung Tizen TV web apps**, replacing Samsung's TV Web Simulator. It runs an App on the same Chromium milestone the target TV generation ships, and reproduces what that TV's platform does: the `tizen.*` and `webapis.*` APIs, the remote control, the network rules, video layering and app termination. That way a developer sees on the desktop what would happen on the TV.

- **Product spec:** [#23](https://github.com/lucaskfp/tizen-sim/issues/23); tickets are its sub-issues.
- **Vocabulary:** `CONTEXT.md` (App, Version Profile, Engine, Session, Shim, Remote Key, Diagnostic, ...).
- **Architectural decisions:** `docs/adr/`.

## Facts, not guesses

Every factual claim, in an answer, a comment, a doc, an issue or a PR, comes from a source you checked in this task: the docs (`docs/agents/tizen-docs.md` says where), the code, a measurement on an Engine or a TV, or the tool's own output. This covers how the TV, Chromium, a library or the tooling behaves, and what this repo does.

- Look it up before stating it. If a question can be answered by reading a page, running a command or measuring, do that first, then answer.
- Cite what you checked: the page, file or measurement.
- If you couldn't check something, say so: mark it **unverified**, or say you don't know. Never fill the gap with something that sounds plausible.
- Don't generalise past what the source says. A rule read off one page, one year or one Engine holds for that page, year or Engine until the others are checked.

## Scope

**In scope: the platform.** What a Tizen TV does for and to an App:

- Run the App on the Engine of the chosen Version Profile, Tizen 3.0 to 10.0.
- Shim the platform APIs as documented, including errors and privilege enforcement.
- Deliver Remote Keys the way the TV does, including registration and what the platform does itself (holding Back terminates the App).
- Apply the App's `config.xml` and Access Policy the way the TV would.
- Raise **Diagnostics** when an App relies on something that behaves differently or fails on a real TV.
- Give the developer a fast loop: hot reload, console, DevTools, a CLI and a GUI.

**Out of scope:**

- **How an App should behave.** Samsung's UX guidelines and certification checklist (exit confirmation popups, focus rules, launch-time targets, "check the network on resume") are the App developer's concern. The simulator neither enforces nor checks them.
- **Product features outside the simulator:** DRM playback, Tizen 2.x/WebKit TVs, packaging and signing `.wgt` files, Tizen Studio integration, decrypting HTTPS, non-TV Tizen profiles, macOS.

When unsure whether something is platform behaviour or App guidance, ask: "does the TV do this, or does Samsung tell the App to do it?" Only the first belongs here.

## Agent skills

### Issue tracker

Issues live in GitHub Issues on `lucaskfp/tizen-sim`, managed with the `gh` CLI. See `docs/agents/issue-tracker.md`. Issues labelled `priority: high` come before other ready tickets.

### Triage labels

Uses the five default triage labels: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.

### Tizen platform docs

Before implementing or changing any emulated platform behaviour, read the documented rules in `docs/agents/tizen-behaviour.md`, then find and cite the source page via `docs/agents/tizen-docs.md`. Don't fill gaps from memory: mark them unverified.
