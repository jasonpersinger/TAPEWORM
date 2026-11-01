"""Sonic-equivalence battery: compare two TAPEWORM CLAP binaries (reference vs candidate build).

usage: python3 -I sonic_equivalence.py <reference.clap> <candidate.clap> <outdir> [quick]

Both binaries are driven through tw_clap_host (independent CLAP host, CLAP ABI only). TAPEWORM's two
xorshift generators use fixed seeds, so every configuration -- including hiss, dropouts and flutter
jitter -- is deterministic per fresh instance and can be compared sample-for-sample.

Writes <outdir>/results.csv, <outdir>/block_invariance.csv and representative WAV pairs.
"""
import csv, math, os, subprocess, sys, tempfile
import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
HOST = os.environ.get("TW_HOST", os.path.join(HERE, "tw_clap_host", "target", "release", "tw_clap_host"))
PREFIX = os.environ.get("TW_PREFIX", "").split()      # candidate runs under this (e.g. run-rootless.sh ...)
REF_PREFIX = os.environ.get("TW_REF_PREFIX", os.environ.get("TW_PREFIX", "")).split()  # reference runs under this
TMP = os.environ.get("TW_TMP")                         # temp dir visible inside PREFIX environment

sys.path.insert(0, HERE)
import twparams as T  # parameter normalisation + WAV writer


def render(plugin, sr, x, init=None, events=(), block=512, prefix=None):
    x = np.ascontiguousarray(x, dtype=np.float32)
    with tempfile.TemporaryDirectory(dir=TMP) as d:
        fi, fo, fs = f"{d}/in.f32", f"{d}/out.f32", f"{d}/s.txt"
        x.tofile(fi)
        with open(fs, "w") as f:
            for k, v in (init or {}).items():
                f.write(f"init {k}={v!r}\n")
            for t, k, v in events:
                f.write(f"at {int(t)} {k}={v!r}\n")
        subprocess.run((PREFIX if prefix is None else prefix) + [HOST, "render", plugin, str(sr), str(block), fi, fo, fs],
                       check=True, capture_output=True)
        return np.fromfile(fo, dtype=np.float32).reshape(-1, 2)


# ------------------------------------------------------------------ signals
def signals(sr, dur):
    n = int(sr * dur); t = np.arange(n) / sr
    rng = np.random.default_rng(2026)
    out = {"silence": np.zeros(n)}
    out["sine_1k_-12dB"] = 0.251 * np.sin(2 * np.pi * 1000 * t)
    out["sine_100_+6dB_hot"] = 2.0 * np.sin(2 * np.pi * 100 * t)
    synth = np.zeros(n)                                  # band-limited detuned saw pad, slow swell
    for f0 in (110.0, 110.6, 164.8, 220.0, 261.6):
        for k in range(1, int(0.45 * sr / f0)):
            synth += np.sin(2 * np.pi * k * f0 * t) / k
    out["synth_pad"] = 0.12 * synth * (0.3 + 0.7 * np.sin(np.pi * t / dur) ** 2)
    out["white_noise_-12dB"] = 0.251 * rng.standard_normal(n) / 1.0
    burst = np.zeros(n)                                  # transient train: 5 ms bursts every 250 ms
    for s in range(0, n, int(0.25 * sr)):
        m = min(int(0.005 * sr), n - s)
        burst[s:s + m] = rng.standard_normal(m) * np.exp(-np.arange(m) / (0.001 * sr))
    out["transients"] = 0.9 * burst
    f1, f2 = 20.0, min(20000.0, 0.45 * sr)
    out["log_sweep"] = 0.5 * np.sin(2 * np.pi * f1 * dur / math.log(f2 / f1) * (np.exp(t / dur * math.log(f2 / f1)) - 1))
    return {k: T.stereo(v) for k, v in out.items()}


# ------------------------------------------------------------------ configurations (CLAP-normalised)
N = T.NEUTRAL
def cfg(**kw):
    d = dict(N); d.update(kw); return d

def configs(sr):
    blk = lambda sec: int(sec * sr) // 512 * 512
    gen_ev = [(blk(k * 0.2), "Generations", float(g)) for k, g in enumerate([0, 3, 1, 2, 0, 3, 0])]
    drive_ev = [(blk(k * 0.1), "Drive", T.drive_db(db)) for k, db in enumerate([0, 24, 6, 18, 0, 12, 24, 3])]
    return [
        ("shipped_defaults", {}, ()),                                   # everything at factory defaults
        ("neutral", cfg(), ()),
        ("sat_ferric_24dB", cfg(**{"Tape Type": 0, "Drive": T.drive_db(24)}), ()),
        ("sat_chrome_24dB", cfg(**{"Tape Type": 1, "Drive": T.drive_db(24)}), ()),
        ("sat_micro_24dB", cfg(**{"Tape Type": 2, "Drive": T.drive_db(24)}), ()),
        ("bias_-100_drive18", cfg(**{"Bias": T.bias(-1), "Drive": T.drive_db(18)}), ()),
        ("bias_+100_drive18", cfg(**{"Bias": T.bias(1), "Drive": T.drive_db(18)}), ()),
        ("max_modulation", cfg(**{"Wow Depth": 1.0, "Flutter Depth": 1.0, "Motor Bog": 1.0,
                                   "Pinch Bump": 1.0, "Azimuth Skew": 1.0}), ()),
        ("motor_bog_only", cfg(**{"Motor Bog": 1.0, "Drive": T.drive_db(12)}), ()),
        ("noise_dropouts_age", cfg(**{"Tape Hiss": 1.0, "Ground Hum": 1.0, "Dropouts": 1.0, "Tape Age": 1.0,
                                       "Generations": T.gen(4)}), ()),
        ("spring_post_full", cfg(**{"Spring Level": 1.0, "Spring Decay": 1.0, "Spring Routing": 1,
                                     "Dry / Wet": 0.5, "Output Gain": T.out_db(-6)}), ()),
        ("generation_switching", cfg(), gen_ev),
        ("drive_automation", cfg(**{"Wow Depth": 0.35, "Flutter Depth": 0.2}), drive_ev),
    ]


# ------------------------------------------------------------------ metrics
def third_octave_db(x, sr):
    m = x.mean(axis=1)
    spec = np.abs(np.fft.rfft(m * np.hanning(len(m)))) ** 2
    f = np.fft.rfftfreq(len(m), 1 / sr)
    bands = []
    fc = 25.0
    while fc < min(20000, sr / 2 / 1.13):
        sel = (f >= fc / 2 ** (1 / 6)) & (f < fc * 2 ** (1 / 6))
        bands.append(10 * np.log10(spec[sel].sum() + 1e-30))
        fc *= 2 ** (1 / 3)
    return np.array(bands)

def lag(a, b, maxlag=64):
    """Integer lag of b relative to a (0 = aligned)."""
    am, bm = a.mean(axis=1), b.mean(axis=1)
    if np.abs(am).max() < 1e-9:
        return 0
    best, bl = -1, 0
    for L in range(-maxlag, maxlag + 1):
        c = np.dot(am[max(0, -L):len(am) - max(0, L)], bm[max(0, L):len(bm) - max(0, -L)])
        if c > best:
            best, bl = c, L
    return bl

def compare(a, b, sr):
    a64, b64 = a.astype(np.float64), b.astype(np.float64)
    d = b64 - a64
    rms_a = math.sqrt(np.mean(a64 ** 2)); rms_b = math.sqrt(np.mean(b64 ** 2)); rms_d = math.sqrt(np.mean(d ** 2))
    if rms_a > 1e-9:
        ba, bb = third_octave_db(a64, sr), third_octave_db(b64, sr)
        live = ba > ba.max() - 90
        spec = float(np.abs(bb - ba)[live].max())
        gain = 20 * math.log10(rms_b / rms_a)
        rel = 20 * math.log10(rms_d / rms_a) if rms_d > 0 else -math.inf
    else:
        spec = gain = 0.0; rel = -math.inf
    return dict(max_abs_diff=float(np.abs(d).max()), diff_rel_db=rel, gain_db=gain, spec_max_db=spec,
                lag=lag(a64, b64), finite=bool(np.isfinite(b).all()), rms_ref=rms_a, identical=bool((a == b).all()))


def main(ref, cand, outdir, quick=False):
    os.makedirs(outdir, exist_ok=True)
    rates = (48000,) if quick else (44100, 48000, 96000, 192000)
    dur = 1.6
    rows, worst = [], 0.0
    wav_cases = {("synth_pad", "shipped_defaults"), ("sine_100_+6dB_hot", "sat_ferric_24dB"),
                 ("white_noise_-12dB", "noise_dropouts_age"), ("synth_pad", "generation_switching"),
                 ("transients", "max_modulation"), ("sine_100_+6dB_hot", "bias_-100_drive18")}
    for sr in rates:
        sigs = signals(sr, dur)
        for cname, init, ev in configs(sr):
            for sname, x in sigs.items():
                a = render(ref, sr, x, init=init, events=ev, prefix=REF_PREFIX)
                b = render(cand, sr, x, init=init, events=ev)
                r = compare(a, b, sr)
                r.update(sr=sr, config=cname, signal=sname)
                rows.append(r); worst = max(worst, r["max_abs_diff"])
                if sr == 48000 and (sname, cname) in wav_cases:
                    base = os.path.join(outdir, f"{cname}__{sname}__48k")
                    T.write_wav(base + "__reference_rc.wav", a, sr)
                    T.write_wav(base + "__portable.wav", b, sr)
            print(f"{sr} {cname:22s} worst so far {worst:.3e}", flush=True)
    keys = ["sr", "config", "signal", "identical", "max_abs_diff", "diff_rel_db", "gain_db", "spec_max_db",
            "lag", "finite", "rms_ref"]
    with open(os.path.join(outdir, "results.csv"), "w", newline="") as f:
        w = csv.DictWriter(f, keys); w.writeheader(); [w.writerow({k: r[k] for k in keys}) for r in rows]

    # Buffer-boundary test: block 61 vs block 512 within each binary, and cross-binary at block 61.
    brows = []
    sr = 48000; sigs = signals(sr, dur)
    for cname, init, ev in configs(sr):
        if ev:
            continue  # events are applied at block starts by nih-plug; block size legitimately matters
        for sname in ("synth_pad", "white_noise_-12dB", "transients"):
            x = sigs[sname]
            r512, r61 = render(ref, sr, x, init, block=512, prefix=REF_PREFIX), render(ref, sr, x, init, block=61, prefix=REF_PREFIX)
            c512, c61 = render(cand, sr, x, init, block=512), render(cand, sr, x, init, block=61)
            brows.append(dict(config=cname, signal=sname,
                              ref_61_vs_512=float(np.abs(r61.astype(float) - r512).max()),
                              cand_61_vs_512=float(np.abs(c61.astype(float) - c512).max()),
                              ref61_vs_cand61=float(np.abs(c61.astype(float) - r61).max())))
    with open(os.path.join(outdir, "block_invariance.csv"), "w", newline="") as f:
        w = csv.DictWriter(f, list(brows[0])); w.writeheader(); w.writerows(brows)

    n_ident = sum(r["identical"] for r in rows)
    print(f"cases={len(rows)} bit_identical={n_ident} worst_max_abs_diff={worst:.3e}")
    print(f"max |gain| dB={max(abs(r['gain_db']) for r in rows):.3e}  max spectral dB={max(r['spec_max_db'] for r in rows):.3e}"
          f"  lags={sorted(set(r['lag'] for r in rows))}  all_finite={all(r['finite'] for r in rows)}")
    print(f"block invariance: max ref61-vs-cand61 = {max(b['ref61_vs_cand61'] for b in brows):.3e}; "
          f"max within-binary 61-vs-512: ref {max(b['ref_61_vs_512'] for b in brows):.3e} "
          f"cand {max(b['cand_61_vs_512'] for b in brows):.3e}")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2], sys.argv[3], quick=len(sys.argv) > 4)
