"""Parameter normalisation and I/O helpers for the TAPEWORM regression harness (numpy only).

CLAP exposes nih-plug float parameters normalised to 0..1; these helpers convert plain units.
"""
import math, struct
import numpy as np


# ---- nih-plug normalisation (CLAP exposes floats as 0..1) -------------------
def _db(x): return 10 ** (x / 20)
def gain_skew_factor(min_db, max_db):
    mn, mx, mid = _db(min_db), _db(max_db), _db((min_db + max_db) / 2)
    return math.log(0.5) / math.log((mid - mn) / (mx - mn))
def skewed_norm(plain, mn, mx, f): return ((plain - mn) / (mx - mn)) ** f
def drive_db(db): return skewed_norm(_db(db), _db(0), _db(24), gain_skew_factor(0, 24))
def out_db(db): return skewed_norm(_db(db), _db(-24), _db(12), gain_skew_factor(-24, 12))
def bias(b): return (b + 1) / 2
def head_bump_db(x): return x / 9.0
def gen(g): return float(g - 1)                     # IntParam 1..4 -> index 0..3

TAPE = {"ferric": 0, "chrome": 1, "micro": 2}

# Neutral "isolate one stage" setting: no modulation, no noise, no reverb, no dropouts.
NEUTRAL = {
    "Tape Type": 0, "Drive": drive_db(0), "Bias": bias(0), "Head Bump": head_bump_db(0),
    "Tape Age": 0.0, "Wow Depth": 0.0, "Flutter Depth": 0.0, "Motor Bog": 0.0,
    "Pinch Bump": 0.0, "Azimuth Skew": 0.0, "Generations": gen(1), "Dropouts": 0.0,
    "Tape Hiss": 0.0, "Ground Hum": 0.0, "Spring Level": 0.0, "Dry / Wet": 1.0,
    "Output Gain": out_db(0),
}


def stereo(m): return np.stack([m, m], axis=1).astype(np.float32)


def write_wav(path, x, sr):
    """32-bit float WAV, no external deps."""
    x = np.asarray(x, dtype=np.float32)
    if x.ndim == 1: x = x[:, None]
    ch = x.shape[1]; data = x.tobytes()
    hdr = b"RIFF" + struct.pack("<I", 36 + len(data)) + b"WAVE"
    hdr += b"fmt " + struct.pack("<IHHIIHH", 16, 3, ch, sr, sr * ch * 4, ch * 4, 32)
    hdr += b"data" + struct.pack("<I", len(data))
    with open(path, "wb") as f: f.write(hdr + data)


