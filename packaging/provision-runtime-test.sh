#!/bin/sh
# Provision a Debian/Ubuntu *runtime* test image: only what a desktop user would already have
# (X11/GL client libraries, GTK3 for REAPER's SWELL layer, ALSA) plus capture tools for the QA harness.
# Nothing here is needed to build TAPEWORM.
set -eu
apt-get -o APT::Sandbox::User=root update
pick() { for p in "$@"; do apt-cache show "$p" >/dev/null 2>&1 && { echo "$p"; return; }; done; }
apt-get -o APT::Sandbox::User=root install -y --no-install-recommends \
    libx11-6 libx11-xcb1 libxcb1 libgl1 libglx-mesa0 libgl1-mesa-dri \
    "$(pick libgtk-3-0t64 libgtk-3-0)" "$(pick libasound2t64 libasound2)" libstdc++6 \
    python3 python3-numpy imagemagick procps fonts-dejavu-core
apt-get clean
