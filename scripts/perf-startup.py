#!/usr/bin/env python3
"""Cold-start timing for a built Folio binary.

    python3 -I scripts/perf-startup.py [--runs N] [--clean] [BINARY]

Runs the binary N times with FOLIO_PERF=1 FOLIO_PERF_QUIT=1 and prints the
median of every `folio_mark` / first-frame number, plus `exec_ms`: wall time
from spawning the process to the first-frame line, which also counts what
happens before `main` (dyld mapping the binary, static initialisers).

`--clean` points HOME at an empty directory, so there is no session to restore
and the window opens on the start page — the number the PRD's 300 ms is about.
`--home DIR` uses DIR as HOME instead. Without either, the user's own
session is restored (and written back on quit).
"""

import argparse
import os
import re
import statistics
import subprocess
import tempfile
import time

LINE = re.compile(r"(folio_mark (\S+) ms=|folio_first_frame_ms=)([\d.]+)(?: rss_mb=([\d.]+))?")


def run_once(binary, env):
    started = time.perf_counter()
    proc = subprocess.Popen([binary], env=env, stderr=subprocess.PIPE, text=True)
    values = {}
    for line in proc.stderr:
        match = LINE.search(line)
        if not match:
            continue
        if match.group(2):
            values[match.group(2)] = float(match.group(3))
        else:
            values["exec_ms"] = (time.perf_counter() - started) * 1000
            values["first_frame"] = float(match.group(3))
            if match.group(4):
                values["rss_mb"] = float(match.group(4))
    proc.wait(timeout=30)
    return values


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", nargs="?", default="target/release/folio")
    parser.add_argument("--runs", type=int, default=7)
    parser.add_argument("--clean", action="store_true")
    parser.add_argument(
        "--home",
        help="use this directory as HOME, e.g. one holding a copy of a real "
        "Library/Application Support/Folio, to time a session restore "
        "without touching the real session",
    )
    args = parser.parse_args()

    env = dict(os.environ, FOLIO_PERF="1", FOLIO_PERF_QUIT="1")
    home = None
    if args.clean:
        home = tempfile.TemporaryDirectory()
        env["HOME"] = home.name
    elif args.home:
        env["HOME"] = args.home

    runs = [run_once(args.binary, env) for _ in range(args.runs)]
    keys = []
    for run in runs:
        for key in run:
            if key not in keys:
                keys.append(key)
    size = os.path.getsize(args.binary) / (1024 * 1024)
    print(f"binary_mb={size:.1f} runs={args.runs} clean={args.clean}")
    for key in keys:
        samples = [run[key] for run in runs if key in run]
        print(f"{key:>16} median={statistics.median(samples):8.1f}  min={min(samples):8.1f}")


if __name__ == "__main__":
    main()
