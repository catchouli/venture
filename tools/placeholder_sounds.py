#!/usr/bin/env python3
"""Makes the placeholder sounds in content/sounds/.

These stand in until real sounds are recorded: each is a short mix of
simple parts (tones that slide in pitch, and filtered noise) with a quick
attack and a fading tail. Only the standard library is used, so it runs
anywhere:

    python3 tools/placeholder_sounds.py

To add a sound, add a function returning its samples to SOUNDS below.
Samples are floats from -1 to 1, at RATE per second. Randomness is seeded,
so the files come out the same every time.
"""

import math
import os
import random
import struct
import wave

RATE = 22050
FOLDER = os.path.join(os.path.dirname(__file__), "..", "content", "sounds")


# Parts ------------------------------------------------------------------------

def length(seconds):
    return int(seconds * RATE)


def envelope(n, attack=0.005, curve=3.0):
    """A quick rise, then a fade to nothing (steeper with a bigger curve)."""
    rise = max(1, length(attack))
    return [min(1.0, i / rise) * (1.0 - i / n) ** curve for i in range(n)]


def tone(seconds, start, end, shape="sine", curve=3.0):
    """A tone sliding from `start` to `end` Hz."""
    n = length(seconds)
    env = envelope(n, curve=curve)
    phase, out = 0.0, []
    for i in range(n):
        frequency = start * (end / start) ** (i / n)
        phase += frequency / RATE
        x = phase % 1.0
        if shape == "sine":
            value = math.sin(2 * math.pi * x)
        elif shape == "square":
            value = 1.0 if x < 0.5 else -1.0
        else:  # saw
            value = 2.0 * x - 1.0
        out.append(value * env[i])
    return out


def noise(seconds, smoothing_start, smoothing_end=None, curve=3.0, seed=1):
    """Noise through a one-pole low-pass filter. Smoothing near 1 is dull and
    low; near 0 is bright and hissy. It slides from start to end."""
    rng = random.Random(seed)
    smoothing_end = smoothing_start if smoothing_end is None else smoothing_end
    n = length(seconds)
    env = envelope(n, attack=0.01, curve=curve)
    level, out = 0.0, []
    for i in range(n):
        smoothing = smoothing_start + (smoothing_end - smoothing_start) * i / n
        level = smoothing * level + (1 - smoothing) * rng.uniform(-1, 1)
        out.append(level * env[i] / max(0.05, 1 - smoothing) ** 0.5)
    return out


def swell(samples):
    """Fades in over the first half (for whooshes that build up)."""
    n = len(samples)
    return [s * min(1.0, 2 * i / n) for i, s in enumerate(samples)]


def mix(*parts, volume=1.0):
    """Adds parts together (each a list of samples, or (samples, gain, delay))."""
    parts = [p if isinstance(p, tuple) else (p, 1.0, 0.0) for p in parts]
    n = max(len(samples) + length(delay) for samples, _, delay in parts)
    out = [0.0] * n
    for samples, gain, delay in parts:
        offset = length(delay)
        for i, s in enumerate(samples):
            out[i + offset] += s * gain
    peak = max(1e-9, max(abs(s) for s in out))
    return [s / peak * volume for s in out]


# Sounds -------------------------------------------------------------------------

def hit():
    return mix((noise(0.12, 0.85, seed=2), 1.0, 0), (tone(0.1, 180, 70), 0.8, 0), volume=0.7)


def critical():
    return mix((noise(0.16, 0.8, seed=3), 1.0, 0), (tone(0.16, 200, 60), 1.0, 0),
               (tone(0.35, 1400, 1300, curve=4), 0.25, 0.02), volume=0.85)


def miss():
    return mix(swell(noise(0.2, 0.6, 0.3, curve=2, seed=4)), volume=0.35)


def death():
    return mix((tone(0.45, 330, 110, "saw", curve=2), 0.5, 0), (noise(0.3, 0.9, seed=5), 0.5, 0), volume=0.5)


def level_up():
    notes = [523, 659, 784, 1047]  # C E G C
    parts = [(tone(0.5, f, f, curve=2.5), 0.6, i * 0.09) for i, f in enumerate(notes)]
    parts.append((tone(0.9, 2093, 2093, curve=5), 0.15, 0.27))
    return mix(*parts, volume=0.6)


def whack():
    return mix((tone(0.25, 140, 45), 1.0, 0.03), (noise(0.2, 0.9, seed=6), 0.9, 0.03),
               (swell(noise(0.06, 0.5, seed=7)), 0.3, 0), volume=0.85)


def cleave():
    return mix((swell(noise(0.18, 0.5, 0.2, curve=1.5, seed=8)), 0.6, 0), (tone(0.2, 160, 60), 0.9, 0.12),
               (noise(0.15, 0.88, seed=9), 0.6, 0.12), volume=0.8)


def air_blast():
    return mix((swell(noise(0.45, 0.8, 0.35, curve=1.5, seed=10)), 1.0, 0),
               (tone(0.4, 300, 700, curve=2), 0.15, 0.05), volume=0.55)


def frost_shard():
    parts = [(tone(0.3, f, f * 1.02, curve=4), 0.4, i * 0.03) for i, f in enumerate([1760, 2349, 2637, 3520])]
    parts.append((noise(0.2, 0.2, seed=11), 0.15, 0))
    return mix(*parts, volume=0.45)


def arcane_bolt():
    return mix((tone(0.4, 220, 440, "square", curve=2), 0.35, 0), (tone(0.4, 223, 447, "saw", curve=2), 0.35, 0),
               (noise(0.35, 0.3, seed=12), 0.2, 0), volume=0.45)


def heal():
    notes = [392, 523, 659]
    return mix(*[(tone(0.7, f, f * 1.01, curve=2), 0.5, i * 0.12) for i, f in enumerate(notes)], volume=0.5)


def rest():
    return mix(swell(noise(0.6, 0.93, 0.97, curve=1.5, seed=13)), volume=0.3)


def cast():
    """A rod's whoosh, then the plop of the line going in."""
    return mix((swell(noise(0.3, 0.75, 0.4, curve=1.5, seed=14)), 0.7, 0), (tone(0.12, 520, 180), 0.5, 0.42),
               (noise(0.2, 0.93, seed=15), 0.5, 0.42), volume=0.55)


def nibble():
    """A tiny plip."""
    return mix((tone(0.06, 900, 500, curve=4), 0.5, 0), (noise(0.05, 0.9, seed=16), 0.2, 0), volume=0.4)


def bite():
    """A splash, and a bell's tink to say: now!"""
    return mix((noise(0.25, 0.85, seed=17), 0.9, 0), (tone(0.15, 300, 120), 0.6, 0),
               (tone(0.5, 2349, 2349, curve=5), 0.35, 0.02), volume=0.7)


def catch():
    """A splash out of the water, and two happy notes."""
    return mix((noise(0.3, 0.8, seed=18), 0.8, 0), (tone(0.35, 659, 659, curve=2.5), 0.4, 0.12),
               (tone(0.5, 988, 988, curve=2.5), 0.4, 0.24), volume=0.6)


SOUNDS = {
    "hit": hit,
    "critical": critical,
    "miss": miss,
    "death": death,
    "level_up": level_up,
    "whack": whack,
    "cleave": cleave,
    "air_blast": air_blast,
    "frost_shard": frost_shard,
    "arcane_bolt": arcane_bolt,
    "heal": heal,
    "rest": rest,
    "cast": cast,
    "nibble": nibble,
    "bite": bite,
    "catch": catch,
}


def write(path, samples):
    with wave.open(path, "wb") as file:
        file.setnchannels(1)
        file.setsampwidth(2)
        file.setframerate(RATE)
        file.writeframes(b"".join(struct.pack("<h", int(max(-1.0, min(1.0, s)) * 32767)) for s in samples))


def main():
    os.makedirs(FOLDER, exist_ok=True)
    for name, make in SOUNDS.items():
        path = os.path.join(FOLDER, name + ".wav")
        write(path, make())
        print("wrote", os.path.relpath(path))


if __name__ == "__main__":
    main()
