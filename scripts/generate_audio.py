"""Regenerate hurt, beam and ambience prototype WAVs (Python stdlib only).

These are deliberately synthetic placeholders, not sampled third-party recordings.
Fixed seeds and periodic loop harmonics make regeneration and loop seams repeatable.
The sampled combat/movement cues are documented in assets/audio/README.md.
"""

import math
from pathlib import Path
import random
import struct
import wave

RATE = 24000
ROOT = Path(__file__).resolve().parents[1] / "assets" / "audio"


def write(name, seconds, sample, stereo=False):
    rng = random.Random(731)
    channels = 2 if stereo else 1
    data = bytearray()
    for i in range(round(seconds * RATE)):
        t = i / RATE
        for channel in range(channels):
            value = sample(t, rng.uniform(-1, 1), channel)
            assert math.isfinite(value) and abs(value) < 1
            data.extend(struct.pack("<h", round(value * 32767)))
    with wave.open(str(ROOT / name), "wb") as output:
        output.setparams((channels, 2, RATE, 0, "NONE", "not compressed"))
        output.writeframes(data)


def envelope(t, duration, decay):
    return min(t / 0.008, 1) * min((duration - t) / 0.02, 1) * math.exp(-decay * t)


def main():
    ROOT.mkdir(parents=True, exist_ok=True)
    tau = math.tau
    write("hurt.wav", 0.28, lambda t, n, c:
          envelope(t, 0.28, 10) * (0.30*math.sin(tau*(240*t - 170*t*t)) + 0.13*n))
    # Integer numbers of cycles over each loop avoid a discontinuity at wrap.
    write("beam.wav", 1.0, lambda t, n, c:
          (0.19*math.sin(tau*110*t) + 0.13*math.sin(tau*330*t)
           + 0.09*math.sin(tau*660*t + 1.5*math.sin(tau*13*t)))
          * (0.75 + 0.25*math.cos(tau*12*t)))
    write("ambience.wav", 4.0, lambda t, n, c:
          sum(0.015 * math.sin(tau*f*t + c*(j+1)*0.4)
              for j, f in enumerate((41, 59.25, 83.5, 117.75, 167, 233.25)))
          * (0.8 + 0.2*math.cos(tau*0.25*t)), stereo=True)


if __name__ == "__main__":
    main()
