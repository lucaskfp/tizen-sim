"""Prints the nightly Shim suite's job matrix: one job per Engine Manifest
entry, i.e. per Version Profile with an Engine on a platform.

An Engine without a headless mode runs in a window off the screen. On a
platform where every Engine has a headless mode, the newest one also runs
that way, so the off-screen window (and Linux's virtual display) stays
tested even without an Engine that needs it.

Usage: python3 shim-suite-matrix.py [path/to/engines.toml]
"""

import json
import sys
import tomllib

RUNNERS = {"windows-x64": "windows-latest", "linux-x64": "ubuntu-latest"}

# Old Linux Engines link GConf, with GTK 2 or 3 and dbus-glib (measured on
# M47, M56 and M63; M69 no longer does). Ubuntu dropped GConf in 24.04, so
# they run on 22.04, the last release with it. Raise this when adding a newer
# Linux Engine that still needs it.
NEWEST_GCONF_MILESTONE = 63
GCONF_RUNNER = "ubuntu-22.04"
GCONF_PACKAGES = "libgtk2.0-0 libgtk-3-0 libgconf-2-4 libdbus-glib-1-2"


def has_headless(engine):
    # As the Engine Manifest's `headless`, which defaults to true.
    return engine.get("headless", True)


def version_key(engine):
    return tuple(int(part) for part in engine["profile"].split("."))


def matrix(manifest):
    jobs = []
    for platform, runner in RUNNERS.items():
        engines = sorted(
            (e for e in manifest["engine"] if e["platform"] == platform),
            key=version_key,
            reverse=True,
        )
        for engine in engines:
            jobs.append(job(engine, runner, off_screen=not has_headless(engine)))
        if engines and all(has_headless(e) for e in engines):
            jobs.append(job(engines[0], runner, off_screen=True, force_off_screen=True))
    return {"include": jobs}


def job(engine, runner, off_screen, force_off_screen=False):
    window = "off-screen window" if off_screen else "headless"
    if force_off_screen:
        window += ", forced"
    packages = ""
    if engine["platform"] == "linux-x64" and engine["milestone"] <= NEWEST_GCONF_MILESTONE:
        runner, packages = GCONF_RUNNER, GCONF_PACKAGES
    return {
        "profile": engine["profile"],
        "runner": runner,
        # System libraries the Engine needs beyond the runner's own.
        "packages": packages,
        "label": f"Tizen {engine['profile']}, {engine['platform']}, {window}",
        # Tells the test harness to treat the Engine as having no headless mode.
        "no_headless": "1" if force_off_screen else "",
    }


if __name__ == "__main__":
    path = sys.argv[1] if len(sys.argv) > 1 else "crates/tizen-sim-core/data/engines.toml"
    with open(path, "rb") as f:
        print(json.dumps(matrix(tomllib.load(f))))
