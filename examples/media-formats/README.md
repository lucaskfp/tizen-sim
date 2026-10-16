# Media Formats

A test App for tizen-sim's media checks (#105): what the Engine of each Version Profile plays of the formats Tizen TVs list, and the `media-format-unsupported` Diagnostics it raises when a TV would play something the Engine can't.

```bash
tizen-sim run examples/media-formats --profile 8.0
```

- The table shows the Engine's own `canPlayType` and `MediaSource.isTypeSupported` answers. Asking is enough for a Diagnostic.
- Arrows move and Enter runs an action: H.264/AAC MP4 and VP8 WebM in `<video>` and AVPlay, an MPEG-TS file name, HLS and MPEG-DASH through AVPlay, and MSE `addSourceBuffer`. The `.ts`, `.m3u8` and `.mpd` files don't exist: the file name is enough for the Diagnostic.
- AVPlay draws behind the page, in the empty area at the bottom right. It shows the clip, or the test pattern when the Engine can't decode the stream.
- Compare `--profile 8.0` (Chromium snapshot: no H.264 or AAC) with `--profile 10.0` (Chrome for Testing: plays them). MPEG-TS, AC-3 and DASH raise a Diagnostic on both.
