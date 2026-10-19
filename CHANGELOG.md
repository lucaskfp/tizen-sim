# Changelog

## [0.3.1](https://github.com/lucaskfp/tizen-sim/compare/v0.3.0...v0.3.1) (2026-10-19)


### Bug Fixes

* draw an App scaled past its resolution in the whole window ([#148](https://github.com/lucaskfp/tizen-sim/issues/148)) ([#149](https://github.com/lucaskfp/tizen-sim/issues/149)) ([6b0d84b](https://github.com/lucaskfp/tizen-sim/commit/6b0d84b275d42d9d614ea731544fa154b5e3c3f6))
* **gui:** clear the Version Profile when a Session ends before starting ([#150](https://github.com/lucaskfp/tizen-sim/issues/150)) ([#151](https://github.com/lucaskfp/tizen-sim/issues/151)) ([cbc7b5a](https://github.com/lucaskfp/tizen-sim/commit/cbc7b5aed14791c1d3aa2aff2183a98aa1ba6314))

## [0.3.0](https://github.com/lucaskfp/tizen-sim/compare/v0.2.0...v0.3.0) (2026-10-17)


### Features

* ask for a required gamepad at launch, per the `gamepad` metadata ([#109](https://github.com/lucaskfp/tizen-sim/issues/109)) ([#144](https://github.com/lucaskfp/tizen-sim/issues/144)) ([85f410d](https://github.com/lucaskfp/tizen-sim/commit/85f410d0c5f36dd66dcb4ad7b6b3431da35f8fd7))
* **gui:** show a grip on the panel splitter and highlight it after a hover delay ([#132](https://github.com/lucaskfp/tizen-sim/issues/132)) ([#134](https://github.com/lucaskfp/tizen-sim/issues/134)) ([d93b5ab](https://github.com/lucaskfp/tizen-sim/commit/d93b5ab576157df0aea44a1eeb5b18dae66ef14f))
* **gui:** show the installed tizen-sim version in the toolbar ([#122](https://github.com/lucaskfp/tizen-sim/issues/122)) ([#133](https://github.com/lucaskfp/tizen-sim/issues/133)) ([8c1fe49](https://github.com/lucaskfp/tizen-sim/commit/8c1fe491338defcbb341970070ff4e7f5749e53f))
* **gui:** show the session panel's tabs as a dropdown when they don't fit ([#125](https://github.com/lucaskfp/tizen-sim/issues/125)) ([61e467c](https://github.com/lucaskfp/tizen-sim/commit/61e467c06ce808860151151230d8afe7fe33bc4e))
* hide web features the TV's engine lacks ([#106](https://github.com/lucaskfp/tizen-sim/issues/106)) ([#135](https://github.com/lucaskfp/tizen-sim/issues/135)) ([b18f337](https://github.com/lucaskfp/tizen-sim/commit/b18f3379e6823fa6b60b6e78ecbec9b4a9eedbcd))
* offer newer releases in the GUI and install them ([#101](https://github.com/lucaskfp/tizen-sim/issues/101)) ([20c5380](https://github.com/lucaskfp/tizen-sim/commit/20c53807145f818c9e6d0c586f9742a78812e6ae))
* shim webapis.adinfo and webapis.widgetdata ([#120](https://github.com/lucaskfp/tizen-sim/issues/120)) ([#140](https://github.com/lucaskfp/tizen-sim/issues/140)) ([0f5f045](https://github.com/lucaskfp/tizen-sim/commit/0f5f045f91a96b0a8b80efd10e1279edbde35503))
* shim webapis.avinfo and tizen.tvdisplaycontrol ([#119](https://github.com/lucaskfp/tizen-sim/issues/119)) ([#137](https://github.com/lucaskfp/tizen-sim/issues/137)) ([36af6a8](https://github.com/lucaskfp/tizen-sim/commit/36af6a854124cdb73b0ccf436e2a1a063766c785))
* show the launch screen until its ready_when, as a TV does ([#113](https://github.com/lucaskfp/tizen-sim/issues/113)) ([#139](https://github.com/lucaskfp/tizen-sim/issues/139)) ([2dd08d3](https://github.com/lucaskfp/tizen-sim/commit/2dd08d37f45afa07521d27bab46b55f87ca8fcf9))
* tell the App when its Engine can't play a media format its TVs play ([#105](https://github.com/lucaskfp/tizen-sim/issues/105)) ([#127](https://github.com/lucaskfp/tizen-sim/issues/127)) ([1d5f9ae](https://github.com/lucaskfp/tizen-sim/commit/1d5f9ae919dfe34fb9a23094c903d20c48592392))


### Bug Fixes

* **deps:** update rustls and zip past RustSec advisories ([#124](https://github.com/lucaskfp/tizen-sim/issues/124)) ([a35ced6](https://github.com/lucaskfp/tizen-sim/commit/a35ced6341dd5339a6a24ca54efd73bcc9efca9f))
* **gui:** build the updater without zip, which has three new advisories ([#145](https://github.com/lucaskfp/tizen-sim/issues/145)) ([98d5751](https://github.com/lucaskfp/tizen-sim/commit/98d575142ecdde176af8c244ad6a6d7d7ee3eb90))

## [0.2.0](https://github.com/lucaskfp/tizen-sim/compare/v0.1.0...v0.2.0) (2026-10-14)


### Features

* resizable panel, dockable at the bottom or on the right ([#76](https://github.com/lucaskfp/tizen-sim/issues/76)) ([#78](https://github.com/lucaskfp/tizen-sim/issues/78)) ([8c6de2d](https://github.com/lucaskfp/tizen-sim/commit/8c6de2d2a841f21e1c00e9d718c2bbc7a376fa7d))


### Bug Fixes

* keep long presses and off-screen Engine windows reliable under load ([#75](https://github.com/lucaskfp/tizen-sim/issues/75)) ([#85](https://github.com/lucaskfp/tizen-sim/issues/85)) ([8b81e01](https://github.com/lucaskfp/tizen-sim/commit/8b81e016bc473524be291ec6623ff1829ba8f857))
* lay the App out again as soon as M63 or M69 resize their window ([#94](https://github.com/lucaskfp/tizen-sim/issues/94)) ([6327671](https://github.com/lucaskfp/tizen-sim/commit/63276715fcc4f6116411320c7a1e80cd71ca5f6f))
* make sure the debugging port an Engine is given is its own ([#83](https://github.com/lucaskfp/tizen-sim/issues/83)) ([#92](https://github.com/lucaskfp/tizen-sim/issues/92)) ([a0d3052](https://github.com/lucaskfp/tizen-sim/commit/a0d3052a9dcb5e1b6e8576a055b3ae4a0116b4ba))
