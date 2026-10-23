"""Makes the lab's two music tracks, assets/music/calm.ogg and chase.ogg.

Both are made here from sines and noise, so they're ours (CC0), and both
loop without a seam: every note is written round the end of the track.
Each is over 10 seconds, so the engine streams it. Needs numpy and ffmpeg
(with libvorbis):

    python3 tool/music.py
"""

import pathlib
import subprocess

import numpy as np

RATE = 48000
OUT = pathlib.Path(__file__).resolve().parent.parent / "assets" / "music"


def hz(note):
    """The frequency of a MIDI note."""
    return 440.0 * 2 ** ((note - 69) / 12)


def add(track, start, sound):
    """Adds `sound` (frames x 2) to `track` from frame `start`, round its end."""
    frames = len(track)
    at = (start + np.arange(len(sound))) % frames
    np.add.at(track, at, sound)


def stereo(mono, pan):
    """`mono` placed at `pan`, -1 left to 1 right, at equal power."""
    angle = (pan + 1) * np.pi / 4
    return np.stack([mono * np.cos(angle), mono * np.sin(angle)], axis=1)


def calm():
    """24 seconds: four slow chords, six seconds each, crossing over."""
    seconds, chord = 24, 6
    track = np.zeros((RATE * seconds, 2))
    chords = [[57, 60, 64, 71], [53, 57, 60, 67], [48, 55, 59, 64], [55, 59, 62, 69]]
    length = chord + 2
    t = np.arange(RATE * length) / RATE
    swell = np.minimum(t / 2.0, 1) * np.minimum((length - t) / 2.5, 1)
    for c, notes in enumerate(chords):
        for n, note in enumerate(notes):
            tone = sum(np.sin(2 * np.pi * hz(note) * (1 + d) * t + n) for d in (-0.002, 0, 0.0025))
            tone += 0.3 * np.sin(4 * np.pi * hz(note) * t)
            wobble = 1 + 0.15 * np.sin(2 * np.pi * (0.2 + 0.05 * n) * t)
            add(track, c * chord * RATE, stereo(0.045 * tone * swell * wobble, (n - 1.5) / 2))
    # A bell every three seconds, high and quiet.
    bell = [76, 79, 74, 72, 76, 81, 79, 74]
    t = np.arange(int(RATE * 2.5)) / RATE
    for b, note in enumerate(bell):
        ring = np.sin(2 * np.pi * hz(note) * t) + 0.25 * np.sin(2 * np.pi * hz(note) * 2.76 * t)
        add(track, b * 3 * RATE, stereo(0.06 * ring * np.exp(-2.2 * t) * np.minimum(t / 0.004, 1), 0.4 - 0.1 * b))
    return track


def chase():
    """16 seconds at 120 beats a minute: a beat, a bass and a run of notes."""
    seconds, beat = 16, RATE // 2
    track = np.zeros((RATE * seconds, 2))
    random = np.random.default_rng(7)
    # A kick on every beat, a hat between.
    t = np.arange(beat // 2) / RATE
    kick = np.sin(2 * np.pi * (45 * t + 60 * (1 - np.exp(-30 * t)) / 30)) * np.exp(-9 * t)
    hat = random.uniform(-1, 1, len(t))
    hat = np.diff(hat, prepend=0) * np.exp(-60 * t)
    for b in range(seconds * 2):
        add(track, b * beat, stereo(0.5 * kick, 0))
        add(track, b * beat + beat // 2, stereo(0.12 * hat, 0.3))
    # A bass in eighths, the chord's root, each bar of four beats.
    roots = [45, 45, 41, 43, 45, 45, 48, 43]
    eighth = beat // 2
    t = np.arange(eighth) / RATE
    for bar, root in enumerate(roots):
        for e in range(8):
            note = root + (12 if e % 4 == 3 else 0)
            saw = sum(np.sin(2 * np.pi * hz(note) * h * t) / h for h in range(1, 7))
            body = saw * np.minimum(t / 0.003, 1) * np.exp(-6 * t) * np.minimum((eighth - np.arange(eighth)) / 200, 1)
            add(track, (bar * 8 + e) * eighth, stereo(0.16 * body, -0.2))
    # A run of sixteenths up and down the bar's chord.
    sixteenth = beat // 4
    t = np.arange(sixteenth * 2) / RATE
    for bar, root in enumerate(roots):
        chord = [root + 24, root + 27 if root == 45 else root + 28, root + 31, root + 36]
        run = chord + chord[::-1][1:3]
        for s in range(16):
            note = run[s % len(run)]
            pluck = np.sin(2 * np.pi * hz(note) * t) + 0.5 * np.sin(4 * np.pi * hz(note) * t)
            add(track, (bar * 16 + s) * sixteenth, stereo(0.07 * pluck * np.exp(-14 * t), 0.5 if s % 2 else -0.5))
    return track


def write(name, track):
    track = track / max(1.0, np.abs(track).max() / 0.9)
    OUT.mkdir(parents=True, exist_ok=True)
    raw = track.astype("<f4").tobytes()
    subprocess.run(
        ["ffmpeg", "-y", "-loglevel", "error", "-f", "f32le", "-ar", str(RATE), "-ac", "2", "-i", "-",
         "-c:a", "libvorbis", "-q:a", "3", str(OUT / f"{name}.ogg")],
        input=raw,
        check=True,
    )
    print(f"{name}.ogg: {len(track) / RATE:.0f} s, {(OUT / f'{name}.ogg').stat().st_size // 1024} KB")


write("calm", calm())
write("chase", chase())
