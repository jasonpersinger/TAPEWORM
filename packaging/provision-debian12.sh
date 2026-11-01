#!/bin/sh
# Provision the TAPEWORM release build environment inside a Debian 12 (bookworm) image.
# Used verbatim by the Dockerfile and by the rootless (bubblewrap) runner, so both paths are identical.
set -eu

RUST_VERSION=1.99.1                       # the compiler that built the certified 753cb63 RC
RUSTUP_VERSION=1.29.2
RUSTUP_INIT_SHA256=5be191e8c70a5007074254cb74d78dbfdd25cd5a61c27de7b8e5410122ec24f4

apt-get -o APT::Sandbox::User=root update
apt-get -o APT::Sandbox::User=root install -y --no-install-recommends \
    build-essential pkg-config ca-certificates curl git xz-utils file binutils python3 \
    libx11-dev libx11-xcb-dev libxcb1-dev libxcb-icccm4-dev libxcb-dri2-0-dev \
    libgl-dev libxcursor-dev libasound2-dev libjack-jackd2-dev
apt-get clean

curl -fsSLo /tmp/rustup-init \
    "https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/x86_64-unknown-linux-gnu/rustup-init"
echo "${RUSTUP_INIT_SHA256}  /tmp/rustup-init" | sha256sum -c -
chmod +x /tmp/rustup-init
/tmp/rustup-init -y --no-modify-path --profile minimal --default-toolchain "${RUST_VERSION}"
rm /tmp/rustup-init

# Toolchain manifest (copied into the build evidence by build-release.sh)
{
  echo "image: $(cat /etc/debian_version 2>/dev/null) ($(. /etc/os-release; echo "$PRETTY_NAME"))"
  ldd --version | head -1
  "$HOME/.cargo/bin/rustc" -vV
  "$HOME/.cargo/bin/cargo" -V
  gcc --version | head -1
  ld --version | head -1
  echo "--- dpkg"
  dpkg-query -W -f '${Package} ${Version}\n'
} > /root/toolchain-manifest.txt
