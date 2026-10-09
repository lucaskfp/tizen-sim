# Remote Key registration lives in the page; the Host asks before delivering

`tizen.tvinputdevice.registerKey` records the registration inside the page, in the Shim. Before the Host delivers a Registrable Key, it asks the page through CDP whether the key is registered. The alternative, where the Shim reports each registration to the Host over the Bridge, races with delivery. A test or a CLI `key` command sent right after `registerKey()` could reach the Host before the registration did, and the key would be wrongly dropped. Keeping the state in the page also gives the TV's lifetime for free: registrations disappear on reload, just as they do when an App relaunches.

## Considered Options

- **The Host owns registration state, fed by the Bridge.** This is what ticket #27 first described. It is simpler to read, but it is racy, unless every registration waits for an acknowledgement that the page can't wait for synchronously on M47.

## Consequences

- Each Registrable Key press costs one CDP evaluation. ADR-0002 lists reading Shim state among CDP's uses.
- An App could overwrite the Shim's hook (`window.__TIZEN_SIM__`). That's acceptable for a development tool.
