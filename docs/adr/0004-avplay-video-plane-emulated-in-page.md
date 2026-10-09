# AVPlay's Video Plane is emulated inside the page

On a real TV, AVPlay draws video on a hardware plane behind the web page. Any opaque page content, including an `html` or `body` background, hides it, which shows up as the classic black-screen bug. We emulate the plane as the back-most element of the page's root stacking context, positioned by `setDisplayRect`. The browser's own paint order then makes App content cover it exactly as on the TV. The one thing DOM order can't reproduce is the root (canvas) background, which always paints beneath everything. So the Shim checks it: if it is opaque over the display area, the Video Plane is hidden and a Diagnostic explains why.

## Considered Options

- **A real separate video layer behind a transparent Engine window.** Closest to the hardware model, but it needs window transparency and frame sync that old Chromium on Windows doesn't offer. Revisit in Phase 2.
- **Ordinary `<video>` in the page.** Would show video that the TV would hide, so it would mask the black-screen bug.

## Consequences

- Old Engines likely lack H.264/AAC, so the Video Plane holds a simulated player (state machine, events, test pattern) and uses real playback only when the Engine can decode the stream.
