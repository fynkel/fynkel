#!/usr/bin/env python3
"""Writes test/collect_all.input.json — the recorded input stream that
walks the Playground's route and collects every coin (docs/input.md has
the recording format). Replayed by test/game_test.dart on the Dart side
and by `fynkel test --headless --input test/collect_all.input.json` on the
Rust host (the runtime walks the controller from its named actions).

The camera rig starts at yaw 0, so W is −Z, D is +X and A is −X; the
route never turns the camera. Frames run at 60 Hz, one tick each.

    python3 tools/route.py
"""

import json
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "test" / "collect_all.input.json"
DT = 1 / 60

# (seconds, keys held) segments, in order; a key named in `taps` is pressed
# for one frame at that time within the segment.
ROUTE = [
    # Coins 1 and 2 straight ahead, to z −12.
    (3.0, ["W"], {}),
    # Coin 3, up the ramp, the platform and coin 4 on it, to x 17.5.
    (4.4, ["D"], {}),
    # Off the platform to coin 5, through the sweeper's corridor to coin 6.
    (3.45, ["W"], {}),
    # Wait for the launcher's ball to roll past the lane's mouth.
    (1.9, [], {}),
    # Across the ball lane (coins 7, 8), a jump onto the block for coin 9,
    # then on to the switch pad.
    (6.45, ["A"], {"Space": 4.5}),
    # Stand on the pad, use it, wait for the gate.
    (1.6, [], {"E": 0.2}),
    # Through the gate to coin 10.
    (2.0, ["A"], {}),
    # Stop; let the last frames settle.
    (0.5, [], {}),
]


def main():
    frames = []
    held = set()
    for seconds, keys, taps in ROUTE:
        n = round(seconds / DT)
        tap_at = {round(t / DT): k for k, t in taps.items()}
        for i in range(n):
            events = []
            want = set(keys)
            for k in sorted(held - want):
                events.append({"device": "keyboard", "control": k, "phase": "up"})
            for k in sorted(want - held):
                events.append({"device": "keyboard", "control": k, "phase": "down", "x": 1})
            held = want
            if i in tap_at:
                events.append({"device": "keyboard", "control": tap_at[i], "phase": "down", "x": 1})
            if i - 1 in tap_at:
                events.append({"device": "keyboard", "control": tap_at[i - 1], "phase": "up"})
            frames.append({"dt": DT, "events": events} if events else {"dt": DT})
    for k in sorted(held):
        frames.append({"dt": DT, "events": [{"device": "keyboard", "control": k, "phase": "up"}]})
    doc = {"fynkel": "input-recording", "formatVersion": 1, "frames": frames}
    OUT.write_text(json.dumps(doc, separators=(",", ":")) + "\n")
    print(f"wrote {OUT}: {len(frames)} frames, {sum(len(f.get('events', [])) for f in frames)} events, {len(frames) * DT:.2f} s")


if __name__ == "__main__":
    main()
