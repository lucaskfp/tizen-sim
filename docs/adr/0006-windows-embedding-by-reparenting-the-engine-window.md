# On Windows the Engine window is re-parented into the GUI; on Linux it stays separate

Because Engines are external processes (ADR-0001), the GUI can't render the App itself. On Windows we re-parent the Engine's native window into the GUI's screen area, so it looks and feels like one application. Remote-control and log panels sit beside the screen rather than over it, because a webview can't be layered above a foreign child window. On Linux the Engine keeps its own window, which is also what Sessions started from the CLI use everywhere.

## Considered Options

- **Separate windows everywhere.** Simplest and identical across platforms, but a worse experience on the primary platform.
- **Stream CDP screencast frames into the GUI.** Works everywhere, but it's laggy and makes video choppy.

## Consequences

- The GUI owns tricky window plumbing: focus, DPI changes, resizing, and recovering when an Engine crashes. The prototype must prove re-parenting works with old Engines.
