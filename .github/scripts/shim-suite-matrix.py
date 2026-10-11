"""Prints the nightly Shim suite's job matrix: one job per Engine Manifest
entry, i.e. per Version Profile with an Engine on a platform.

An Engine without a headless mode runs in a window off the screen. On a
platform where every Engine has a headless mode, the newest one also runs
that way, so the off-screen window (and Linux's virtual display) stays
tested before an Engine that needs it arrives.

Usage: python3 shim-suite-matrix.py [path/to/engines.toml]
"""

import json
import sys
import tomllib

RUNNERS = {"windows-x64": "windows-latest", "linux-x64": "ubuntu-latest"}


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
    return {
        "profile": engine["profile"],
        "runner": runner,
        "label": f"Tizen {engine['profile']}, {engine['platform']}, {window}",
        # Tells the test harness to treat the Engine as having no headless mode.
        "no_headless": "1" if force_off_screen else "",
    }


if __name__ == "__main__":
    path = sys.argv[1] if len(sys.argv) > 1 else "crates/tizen-sim-core/data/engines.toml"
    with open(path, "rb") as f:
        print(json.dumps(matrix(tomllib.load(f))))
