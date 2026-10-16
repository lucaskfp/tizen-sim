# Contributing to tizen-sim

By taking part you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md). Report security vulnerabilities privately, as [SECURITY.md](SECURITY.md) describes, not in public issues.

Contributions are welcome as issues: bug reports, behaviour you've measured on a real TV, proposals and fixes. Only collaborators open pull requests.

## Reporting issues and proposing work

- **Issues** live in [GitHub Issues](https://github.com/lucaskfp/tizen-sim/issues). They are the way to contribute: pull requests from outside contributors are turned off.
- **Bug reports** should give:
  - the Version Profile;
  - your OS;
  - the command or GUI steps;
  - what you expected, ideally what the real TV does.
- **Triage labels:**
  - `needs-triage`: not evaluated yet.
  - `needs-info`: waiting on the reporter.
  - `ready-for-agent`: fully specified, ready for an AI agent.
  - `ready-for-human`: needs human implementation.
  - `wontfix`: won't be actioned.
  
  Issues labelled `priority: high` come first.
- **Measurements from a real TV** are especially valuable: what an API returns, which keys arrive, what the network allows. Many documented rules are marked *unverified* until someone checks them on hardware.

## Pull requests

Only the repo's collaborators can open pull requests. If you have a fix, open an issue describing it; you can paste the diff there, and a maintainer will turn it into a pull request. If you'd like to contribute regularly, ask in an issue to be added as a collaborator.

For collaborators:

1. Work from an issue, so the approach is agreed before you write code.
2. Branch from `main`.
3. Keep the change to one issue, with tests: unit tests next to the code, and Session tests under `crates/tizen-sim-core/tests` for behaviour an App can see.
4. Run the checks under [Development](#development) before pushing.
5. Open the pull request against `main` and reference the issue (`Closes #123`).

`main` only takes pull requests whose CI passes: lint, tests on Windows and Linux, the GUI and Shim builds, and CodeQL. Pull requests are squash-merged, so the PR title becomes the commit message: write it as a [Conventional Commit](https://www.conventionalcommits.org). The `PR title` check enforces it, with the types `feat`, `fix`, `docs`, `ci`, `build`, `test`, `refactor`, `perf`, `chore` and `revert`. `feat` and `fix` decide the next version.

## Platform behaviour must be sourced

tizen-sim is only useful if it matches real TVs, so emulated behaviour is never filled in from memory.

1. Read the rules already recorded in [docs/agents/tizen-behaviour.md](docs/agents/tizen-behaviour.md).
2. Find the source page for the behaviour you're changing, using [docs/agents/tizen-docs.md](docs/agents/tizen-docs.md), and cite it in the code or tests.
3. If no source or measurement exists, mark the behaviour **unverified** rather than guessing.

## Vocabulary and decisions

- [CONTEXT.md](CONTEXT.md) defines the project's terms (App, Version Profile, Engine, Session, Shim, Remote Key, Diagnostic, ...). Use them in code, docs and issues.
- A significant design decision gets an ADR in [docs/adr/](docs/adr/).

## Development

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The tests start real Engines, so the first run downloads them.

**Test environment variables:**

| Variable                     | Effect                                                                                    |
| ---------------------------- | ----------------------------------------------------------------------------------------- |
| `TIZEN_SIM_TEST_PROFILE`     | The Version Profile to test on, e.g. `TIZEN_SIM_TEST_PROFILE=3.0 cargo test -p tizen-sim-core` |
| `TIZEN_SIM_TEST_HEADED`      | Run the Engines in visible windows                                                        |
| `TIZEN_SIM_TEST_NO_HEADLESS` | Run as an Engine without a headless mode does, in an off-screen window                    |

On Linux, run the Tizen 3.0 tests under `xvfb-run`, as CI does.

**Shims.** The core embeds the committed bundle, `shims/dist/tizen-sim-shims.js`. After changing `shims/src`, rebuild it and commit the result; CI checks that it matches the sources and is ES5:

```bash
cd shims
npm ci
npm run build
npm run check-es5
```

**GUI frontend.** In `crates/tizen-sim-gui`, `npm run build` and `npm test` typecheck the frontend and smoke-test it.

**Other conventions:**

- **Commits** follow [Conventional Commits](https://www.conventionalcommits.org) (`feat:`, `fix:`, `docs:`, `ci:`, ...) and reference their issue, e.g. `(#50)`.
- **Code** should read like the code around it: same naming, idiom and comment density.
- **CI** runs lint and tests on Windows and Linux on every push. A nightly job runs the in-page Shim suite and the network tests on every Version Profile's Engine.

## AI agents

The repo is set up for coding agents. [AGENTS.md](AGENTS.md) is their entry point, and [docs/agents/](docs/agents/) holds the issue-tracker, triage and platform-documentation conventions.
