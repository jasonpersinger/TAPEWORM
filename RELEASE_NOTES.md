# TAPEWORM 0.1.0 — release notes

First public release of TAPEWORM, a cassette-tape degradation and lo-fi effect for dungeon synth, dark
ambient, lo-fi electronic and retro synthesizer music.

## Downloads

| File | Contents |
|---|---|
| `tapeworm-v0.1.0-linux-x86_64-clap.tar.gz` | `tapeworm.clap` + licence, notices, install guide |
| `tapeworm-v0.1.0-linux-x86_64-vst3.tar.gz` | `tapeworm.vst3` bundle + licence, notices, install guide |
| `tapeworm-v0.1.0-source.tar.gz` | complete corresponding source, including all dependency sources for an offline rebuild |
| `SHA256SUMS` | SHA-256 checksums of the three archives |

## Supported platforms

* Linux x86_64, glibc 2.35 or newer, X11 or XWayland with OpenGL.
* Tested with REAPER 7.83 (CLAP and VST3) on CachyOS and in Debian 12 and Ubuntu 24.04 LTS userlands;
  loading and audio processing also checked on Ubuntu 22.04 LTS (glibc 2.35).
* Windows and macOS builds are not available in this release.

## Features

* Three tape types with saturation, bias shaping, head bump and tape-age high-frequency loss.
* Wow, flutter, motor bog (pitch sag under loud material), pinch-roller bumps, azimuth wander.
* 1–4 dub generations with click-free switching; dropouts, hiss, 50 / 60 Hz hum.
* Dark spring reverb, routable before or after the tape.
* Dry / Wet with a latency-matched dry path; 21 automatable parameters; pixel-art editor.

## Installation

Unpack and copy `tapeworm.clap` to `~/.clap/` and/or `tapeworm.vst3` to `~/.vst3/`, then rescan
plug-ins in your host. No root access needed. See `INSTALLING.md`.

## Compatibility

* CLAP ID `com.messyquest.tapeworm`; VST3 and CLAP versions are the same binary and sound identical.
* 15 ms latency, reported to the host. Tested at 44.1, 48, 88.2, 96 and 192 kHz.

## Known limitations

* **No oversampling:** high-frequency material at high Drive aliases audibly. Lower Drive or darker
  input keeps aliasing low (about −60 dB or lower up to 5 kHz at +12 dB Drive).
* **Bias** adds odd harmonics only. **Negative Bias with high Drive folds the waveform**, giving a harsh,
  buzzy distortion and a quieter fundamental — intended as a special effect, not as conventional bias.
* **Azimuth Skew** causes comb filtering when summed to mono (first notch about 2.8 kHz at 100 %, above
  about 11 kHz at the default 25 %).
* **Generations** changes produce a faint, brief (≈ 25 ms) stepping artefact far below the signal
  (measured at −49 to −83 dBFS with a −6 dBFS test tone); no clicks.
* **Renders are not sample-repeatable within a session**, because the random generators for flutter
  jitter, dropouts and hiss keep running. A freshly loaded instance always starts the same way.
* Output differs between Linux distributions at the float-rounding level (≈ −85 dB or lower), because
  math functions come from the system's C library — far below the signal level.
* Sample rates above 192 kHz are not supported. The editor needs X11 or XWayland.
* Tested with REAPER only; other hosts are expected to work but have not been verified.

## Licence

GPL-3.0-or-later. See `LICENSING.md` and `THIRD_PARTY_NOTICES.md`. The corresponding source is published
alongside the binaries as `tapeworm-v0.1.0-source.tar.gz`.
