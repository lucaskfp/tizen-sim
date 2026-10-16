"""Generates web_features.toml: the web features each Tizen TV release has.

Reads Samsung's TV web engine specifications page, saved as HTML, from
https://developer.samsung.com/smarttv/develop/specifications/web-engine-specifications.html.
Its "Web Standard Feature Support" section has one table per area (JavaScript
(ES6), Device, DOM, ... Miscellaneous Features), one row per feature and one
column per TV year; cells span rows and columns where values repeat. Only the
Tizen 3.0 to 10.0 columns, the Version Profiles, are kept. The CSS tables after
that section are not mirrored.

REMOVE says what the Shims delete on a profile whose TVs lack a feature;
UNVERIFIED marks rows the table leaves ambiguous, which are not applied.

Usage: python3 web-features.py <web-engine-specifications.html> > web_features.toml
"""

import html
import re
import sys
from html.parser import HTMLParser

PAGE = "specifications/web-engine-specifications.html"

# The section's tables, in page order.
TABLES = ["JavaScript (ES6)", "Device", "DOM", "File System", "Graphics", "Multimedia",
          "Network and Connectivity", "Offline Storage", "Performance",
          "Real-time Communication", "Security", "User Input", "Web Components",
          "Miscellaneous Features"]

PROFILES = ["3.0", "4.0", "5.0", "5.5", "6.0", "6.5", "7.0", "8.0", "9.0", "10.0"]

SUPPORT = {"Yes": "yes", "No": "no", "Yes (partially)": "partial",
           "Yes (basic support)": "basic"}

# What an App reaches of each feature: paths from `window`, the last segment
# a member deleted from its owner and the owner's prototypes; a trailing `*`
# deletes every global with that prefix. Missing paths are skipped, so a
# list can name what any Engine has. Features without an entry either can't
# be removed (syntax, events, HTTP headers, TLS, file formats) or have nothing
# to remove, as no Engine has them where their TVs lack them (Preload and
# Shadow DOM v1 on 3.0, PNaCl on 10.0; measured, #106).
REMOVE = {
    "Array.prototype.copyWithin() method": ["Array.prototype.copyWithin"],
    "Map objects": ["Map"],
    # The methods ES2015 adds (ECMA-262 6th edition 20.1.2, against 5.1's
    # 15.7.3). Its constants, EPSILON and MAX_/MIN_SAFE_INTEGER, can't be
    # deleted on the 3.0 Engine (measured, #106), so they stay.
    "Number object extensions": [
        "Number.isFinite", "Number.isInteger", "Number.isNaN", "Number.isSafeInteger",
        "Number.parseFloat", "Number.parseInt"],
    "Object.assign() method": ["Object.assign"],
    "Promise objects": ["Promise"],
    "Proxy objects": ["Proxy"],
    "Reflect object": ["Reflect"],
    "Symbol objects": ["Symbol"],
    "WeakMap objects": ["WeakMap"],
    "WeakSet objects": ["WeakSet"],
    "Geolocation API": [
        "Navigator.prototype.geolocation", "Geolocation", "Coordinates", "Position",
        "PositionError", "GeolocationCoordinates", "GeolocationPosition",
        "GeolocationPositionError"],
    "ImageBitmapRenderingContext interface": ["ImageBitmapRenderingContext"],
    "imageSmoothingQuality property": ["CanvasRenderingContext2D.prototype.imageSmoothingQuality"],
    "Audio Output Devices API": ["HTMLMediaElement.prototype.setSinkId",
                                 "HTMLMediaElement.prototype.sinkId"],
    "Intersection Observer API": ["IntersectionObserver", "IntersectionObserverEntry"],
    "requestIdleCallback() method": ["requestIdleCallback", "cancelIdleCallback", "IdleDeadline"],
    # Resource entries still come from performance.getEntries().
    "Resource Timing": [
        "PerformanceResourceTiming", "Performance.prototype.clearResourceTimings",
        "Performance.prototype.setResourceTimingBufferSize",
        "Performance.prototype.onresourcetimingbufferfull",
        "Performance.prototype.webkitClearResourceTimings",
        "Performance.prototype.webkitSetResourceTimingBufferSize",
        "Performance.prototype.onwebkitresourcetimingbufferfull"],
    "User Timing": ["Performance.prototype.mark", "Performance.prototype.measure",
                    "Performance.prototype.clearMarks", "Performance.prototype.clearMeasures",
                    "PerformanceMark", "PerformanceMeasure"],
    "Push API": ["PushManager", "PushSubscription", "PushSubscriptionOptions", "PushEvent",
                 "PushMessageData", "ServiceWorkerRegistration.prototype.pushManager"],
    "WebRTC API": ["RTC*", "webkitRTC*"],
    "autocapitalize attribute": ["HTMLElement.prototype.autocapitalize"],
    "MouseEvent.buttons property": ["MouseEvent.prototype.buttons"],
    "WebVR API": ["Navigator.prototype.getVRDisplays", "VR*"],
}

UNVERIFIED = {
    "Custom Elements": (
        "The row doesn't say which version. It is No on 6.5 to 9.0 and Yes before and on "
        "10.0; the Engines have Custom Elements v1 (customElements) from 4.0 on, and v0 "
        "(document.registerElement) up to 6.0 only (measured, #106)."),
    "HTML Imports": "No on 6.5 to 9.0 and Yes before and on 10.0; the Engines have it up to 6.0 only (measured, #106).",
    "Shadow DOM v0": "No on 10.0 only; the Engines have it up to 6.0 only (measured, #106).",
    "Template literals": "Yes (partially) on 5.0 and 10.0 but Yes in between; the page doesn't say what is missing.",
    "input event": "No on 3.0 between Yes on 2.4 and Yes (partially) on 4.0 to 9.0; the page doesn't say what is missing.",
    "Content Security Policy Level 1": "No on 4.0 only, while Level 2 is Yes there.",
}


class Tables(HTMLParser):
    """Each table on the page, after the heading before it, as rows of (text, rowspan, colspan)."""

    def __init__(self):
        super().__init__()
        self.tables, self.open, self.cell = [], [], None
        self.heading, self.in_heading = "", False

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == "table":
            self.open.append((self.heading, []))
        elif tag == "tr" and self.open:
            self.open[-1][1].append([])
        elif tag in ("td", "th") and self.open:
            self.cell = ["", int(attrs.get("rowspan") or 1), int(attrs.get("colspan") or 1)]
        elif re.fullmatch(r"h[1-6]", tag):
            self.in_heading, self.heading = True, ""

    def handle_endtag(self, tag):
        if tag in ("td", "th") and self.cell is not None and self.open:
            self.open[-1][1][-1].append(self.cell)
            self.cell = None
        elif tag == "table" and self.open:
            self.tables.append(self.open.pop())
        elif re.fullmatch(r"h[1-6]", tag):
            self.in_heading = False
            self.heading = " ".join(self.heading.split())

    def handle_data(self, data):
        if self.cell is not None:
            self.cell[0] += data
        if self.in_heading:
            self.heading += data


def grid(rows):
    """The table's cells by (row, column), with spanned cells repeated."""
    cells = {}
    for r, row in enumerate(rows):
        c = 0
        for text, rowspan, colspan in row:
            while (r, c) in cells:
                c += 1
            text = " ".join(html.unescape(text).split())
            for dr in range(rowspan):
                for dc in range(colspan):
                    cells[(r + dr, c + dc)] = text
            c += colspan
    return cells


def main(path):
    parser = Tables()
    parser.feed(open(path, encoding="utf-8").read())
    tables = {heading: rows for heading, rows in parser.tables}
    seen = set()
    print("# Web features per Version Profile, from Samsung's TV web engine")
    print(f"# specifications (TV {PAGE}), \"Web Standard Feature Support\":")
    print("# one entry per row, `source` citing the page and the table it is in.")
    print("# Generated by web-features.py; edit that, not this file.")
    print("#")
    print("# - `support`: the row's cell for each Version Profile: yes, partial")
    print("#   (\"Yes (partially)\"), basic (\"Yes (basic support)\") or no.")
    print("# - `remove`: what the Shims delete where it is `no` (see the script).")
    print("# - `unverified`: why the row is ambiguous; it is not applied.")
    for table in TABLES:
        cells = grid(tables[table])
        header = [cells.get((0, c), "") for c in range(20)]
        columns = {}
        for profile in PROFILES:
            matches = [c for c, h in enumerate(header) if h.startswith(f"Tizen {profile} ")
                       or h.startswith(f"Tizen {profile}(")]
            if len(matches) != 1:
                sys.exit(f"{table}: no single column for Tizen {profile}: {header}")
            columns[profile] = matches[0]
        rows = max(r for r, _ in cells) + 1
        for r in range(1, rows):
            name = cells.get((r, 0), "")
            support = {}
            for profile in PROFILES:
                # Some cells read "Yes(partially)".
                value = re.sub(r"\s*\(", " (", cells.get((r, columns[profile]), ""))
                if value not in SUPPORT:
                    sys.exit(f"{table}: {name}: Tizen {profile} is {value!r}")
                support[profile] = SUPPORT[value]
            seen.add(name)
            print(f"\n[[feature]]")
            print(f"name = {toml_string(name)}")
            print(f"source = {toml_string(f'TV {PAGE}, {table} table')}")
            print("support = { " + ", ".join(f'"{p}" = "{support[p]}"' for p in PROFILES) + " }")
            if name in REMOVE:
                print("remove = [" + ", ".join(toml_string(p) for p in REMOVE[name]) + "]")
            if name in UNVERIFIED:
                print(f"unverified = {toml_string(UNVERIFIED[name])}")
    unknown = (set(REMOVE) | set(UNVERIFIED)) - seen
    if unknown:
        sys.exit(f"not on the page: {sorted(unknown)}")


def toml_string(value):
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


if __name__ == "__main__":
    main(sys.argv[1])
