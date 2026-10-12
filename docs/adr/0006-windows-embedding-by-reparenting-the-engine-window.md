# On Windows the Engine window is re-parented into the GUI; on Linux it stays separate

Because Engines are external processes (ADR-0001), the GUI can't render the App itself. On Windows we re-parent the Engine's native window into the GUI's screen area, so it looks and feels like one application. Remote-control and log panels sit beside the screen rather than over it, because a webview can't be layered above a foreign child window. On Linux the Engine keeps its own window, which is also what Sessions started from the CLI use everywhere.

## Considered Options

- **Separate windows everywhere.** Simplest and identical across platforms, but a worse experience on the primary platform.
- **Stream CDP screencast frames into the GUI.** Works everywhere, but it's laggy and makes video choppy.

## Consequences

- The GUI owns tricky window plumbing: focus, DPI changes, resizing, and recovering when an Engine crashes. The prototype must prove re-parenting works with old Engines.
- A Session the GUI adopts from the CLI (ADR-0005) is embedded too, while the GUI shows it. When the GUI lets go of it, or closes, its window is given back as its own top-level window, where it was, and the Session keeps running.
- Chromium draws its own title bar and info bars (the "Chrome for Testing" notice) inside its window, so removing the native frame isn't enough. The GUI makes the Engine window larger by those bars and offsets it so only the page shows; the page's position comes from Chromium's page window (`Chrome_RenderWidgetHostHWND`).
- The GUI places the Engine window as large as fits at 16:9, centered in its padded screen area, so the App's scaled screen fills it and the leftover space is a letterbox around it. The window is placed rather than the App offset inside it, because from M63 on `Emulation.setDeviceMetricsOverride` has no `offsetX`/`offsetY` (M63 and M130 protocols checked). Its `viewport` parameter does shift the drawing, but it seems to pin the view to document coordinates, which would hide scrolling. That is **unverified**: read from Blink's source, not measured. So a Session in its own window still draws the App at the top left (#57).
- An embedded window counts as occluded whenever the GUI is behind another window, and Chromium then stops drawing, which also stalls screenshots. Engines run with occluded-window backgrounding off; hiding an App (`Application.hide`) minimizes and freezes it explicitly, so it is unaffected.
