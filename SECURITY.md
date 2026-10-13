# Security policy

## Reporting a vulnerability

Report vulnerabilities privately through GitHub: on the repository's [Security tab](https://github.com/lucaskfp/tizen-sim/security), choose **Report a vulnerability**. Don't open a public issue.

Include what's affected (the CLI, the GUI or a Session's Engine), the version or commit, your OS, and the steps or proof of concept. You'll get a reply within a week. A fix is released before the advisory is published, and you're credited unless you'd rather not be.

## What's in scope

tizen-sim runs untrusted web content, an App, in a pinned Chromium Engine, and puts local servers in front of it. Issues in what tizen-sim adds are in scope, for example:

- the network proxy that enforces an App's Access Policy;
- the dev server, the DevTools relay and other listeners, and who can reach them;
- how Packages (`.wgt` files) are unpacked, and the App's sandboxed file system;
- how Engines are downloaded and verified.

## What isn't

The Engines are old Chromium builds on purpose: they match the Chromium a Tizen TV ships (ADR-0001), so they carry that version's known vulnerabilities. Don't open a tizen-sim App you don't trust. Known Chromium vulnerabilities in a pinned Engine aren't tizen-sim vulnerabilities, unless tizen-sim makes them reachable from outside the App.

## Supported versions

Fixes go into the latest release only.
