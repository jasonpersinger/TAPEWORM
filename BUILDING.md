# Building TAPEWORM

## Quick development build

Requirements: Linux x86_64, Rust (stable; releases use **1.99.1**), a C toolchain, `pkg-config`, and the
X11 / OpenGL / ALSA / JACK development headers. On Debian or Ubuntu:

```sh
sudo apt install build-essential pkg-config libx11-dev libx11-xcb-dev libxcb1-dev libxcb-icccm4-dev \
    libxcb-dri2-0-dev libgl-dev libxcursor-dev libasound2-dev libjack-jackd2-dev
```

Then:

```sh
cargo test --locked --lib                       # 27 unit tests
cargo xtask bundle tapeworm --release --locked  # -> target/bundled/tapeworm.clap, target/bundled/tapeworm.vst3
./build_and_install.sh                          # build, then copy into ~/.clap and ~/.vst3 (overwrites!)
```

A binary built this way links against **your** system's glibc and will usually not load on older
distributions. Use the release build below for anything you distribute.

## Release build (portable, glibc ≤ 2.36)

Official binaries are built in a pinned Debian 12 environment (glibc 2.36):

| Item | Value |
|---|---|
| Base image | `debian:bookworm@sha256:a0039cc6bb3eb2215aabcd3437cce8808a3858d86bf483eee2ee14fe4d2551c2` (Debian 12.15, linux/amd64) |
| Rust | 1.99.1 (`58cf8f9c3 2026-10-13`), installed by rustup-init 1.29.2 (SHA-256 checked), profile `minimal` |
| C toolchain | Debian `build-essential` (gcc 12.2, binutils 2.40) |
| Dependencies | exactly as pinned by `Cargo.lock` (`--locked`), compiled offline (`--offline`) |
| Provisioning | `packaging/provision-debian12.sh` (used by both runners) |

```sh
packaging/make-release.sh --out dist            # uses Docker if installed, otherwise the rootless runner
packaging/make-release.sh --runner rootless     # force bubblewrap + user namespaces (no root, no daemon)
packaging/make-release.sh --rev v0.1.0          # build a specific commit (via git archive)
```

The script:

1. exports the source (`git archive` of a clean commit, or the working tree marked `uncommitted-tree`);
2. fetches every locked dependency into a fresh `CARGO_HOME` (the only step with network access);
3. runs the unit tests and builds both formats with networking disabled (`--locked --offline`);
4. checks both binaries: x86-64 shared object, highest `GLIBC_` symbol version ≤ 2.36, only the expected
   shared-library dependencies, no RPATH;
5. writes `tapeworm-v<ver>-linux-x86_64-{clap,vst3}.tar.gz` and the complete
   `tapeworm-v<ver>-source.tar.gz` (deterministic tar: sorted, fixed mtime, numeric owner, `gzip -n`);
6. writes `SHA256SUMS` and `BUILD_MANIFEST.txt` (revision, image digest, tool and package versions, hashes).

The rootless runner needs bubblewrap (`bwrap`), util-linux `unshare` with `--map-auto`, unprivileged user
namespaces and a subordinate uid range for your user in `/etc/subuid` and `/etc/subgid`. It downloads the
image by digest from Docker Hub with `packaging/oci-fetch.py` (each layer is SHA-256-verified).

Building is never the same as publishing: the script uploads nothing.

## Rebuilding from the source archive

`tapeworm-v<ver>-source.tar.gz` is the complete corresponding source of a release:

```
tapeworm-v0.1.0-source/
├── tapeworm-0.1.0/     the TAPEWORM tree (this repository at the release revision)
└── cargo-home/         every dependency pinned by Cargo.lock: registry index + .crate files, git databases
```

To rebuild offline, in the pinned environment:

```sh
tar -xzf tapeworm-v0.1.0-source.tar.gz && cd tapeworm-v0.1.0-source
mkdir -p out target
P=tapeworm-0.1.0/packaging
python3 -I $P/oci-fetch.py library/debian sha256:a0039cc6bb3eb2215aabcd3437cce8808a3858d86bf483eee2ee14fe4d2551c2 rootfs
$P/run-rootless.sh rootfs --bind $PWD/$P:/be -- sh /be/provision-debian12.sh       # needs network once
$P/run-rootless.sh rootfs --no-net --bind $PWD/$P:/be \
    --bind $PWD/tapeworm-0.1.0:/build/src --bind $PWD/cargo-home:/build/cargo-home \
    --bind $PWD/target:/build/target --bind $PWD/out:/build/out -- bash /be/build-release.sh build
sha256sum out/bundled/tapeworm.clap
```

Or with any Rust 1.99.1 installation (the result then depends on your system's glibc):

```sh
cd tapeworm-0.1.0
CARGO_HOME=$PWD/../cargo-home CARGO_NET_OFFLINE=true cargo xtask bundle tapeworm --release --locked --offline
```

Why not `cargo vendor`? `Cargo.lock` contains two git revisions of `baseview` with the same version,
which `cargo vendor` cannot represent. Shipping Cargo's own cache (`cargo-home/`) avoids that limitation
and keeps every dependency byte-identical to what the release used.

## Reproducibility

Rebuilding the same revision in the pinned environment has produced byte-identical binaries (see the
release's `BUILD_MANIFEST.txt`); the build uses fixed in-container paths (`/build/src`,
`/build/cargo-home`), so the location of your checkout does not leak into the binary. This has been
demonstrated on one build machine, not across machines with different kernels or CPUs.

## Tests

* `cargo test --locked --lib` — unit tests for the DSP (generation crossfades, azimuth scaling, motor
  bog bounds, dry/wet, …).
* `tests/regression/` — black-box comparison of two built `.clap` binaries through an independent CLAP
  host (364 renders: 7 signals × 13 settings × 4 sample rates, plus a buffer-size check). Use it before
  and after any change that might affect the sound:

  ```sh
  (cd tests/regression/tw_clap_host && cargo build --release --locked)
  python3 -I tests/regression/compare_builds.py reference.clap candidate.clap /tmp/compare
  ```

  Needs Python 3 with numpy. Renders of the same binary on systems with different glibc versions differ
  at the float-rounding level (≈ −85 dB or lower), because the math functions come from the system's libm.

## Troubleshooting

* **`failed to load source for dependency` / network errors with `--offline`** — the dependency cache is
  incomplete; run `packaging/build-release.sh fetch` (or use the release source archive's `cargo-home/`).
* **`pkg-config` cannot find `x11`, `xcb`, `gl` or `alsa`** — install the `-dev` packages listed above.
* **`unshare: failed … --map-auto`** — your user has no `/etc/subuid` range, or util-linux is older than 2.38.
  Use Docker, or ask your administrator for a subordinate id range.
* **The built plug-in reports `GLIBC_2.xx not found` on another machine** — it was built outside the
  pinned environment; use `packaging/make-release.sh`.
