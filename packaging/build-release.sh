#!/usr/bin/env bash
# Build TAPEWORM CLAP + VST3 inside the Debian 12 build environment.
# Expects (bind-mounted or COPY'd):
#   /build/src         the TAPEWORM source tree (Cargo.toml, Cargo.lock, src/, xtask/, assets/)
#   /build/cargo-home  CARGO_HOME; populated by `fetch`, reused offline by `build`
#   /build/out         output directory
#
#   build-release.sh fetch   download all locked dependencies (network)
#   build-release.sh build   test + bundle, strictly offline (--locked --offline)
#   build-release.sh all     fetch, then build
set -euo pipefail
export CARGO_HOME=/build/cargo-home
export CARGO_TARGET_DIR=/build/target
export PATH="/root/.cargo/bin:$PATH"   # rustup proxies (RUSTUP_HOME=/root/.rustup)
cd /build/src

fetch() {
  # All platforms: nih_plug_xtask runs `cargo metadata` without a platform filter, which needs every
  # package in Cargo.lock to be present for an offline build.
  cargo fetch --locked
}

build() {
  export CARGO_NET_OFFLINE=true
  cargo test --locked --offline --lib 2>&1 | tee /build/out/test.log
  cargo xtask bundle tapeworm --release --locked --offline 2>&1 | tee /build/out/bundle.log
  rm -rf /build/out/bundled && cp -a "$CARGO_TARGET_DIR/bundled" /build/out/bundled
  cp /root/toolchain-manifest.txt /build/out/toolchain-manifest.txt
  ( cd /build/out/bundled && find . -type f -exec sha256sum {} + | sort -k2 ) > /build/out/bundled.sha256
  cat /build/out/bundled.sha256
}

case "${1:-all}" in
  fetch) fetch ;;
  build) build ;;
  all) fetch; build ;;
  *) echo "usage: $0 fetch|build|all" >&2; exit 2 ;;
esac
