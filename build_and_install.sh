#!/usr/bin/env bash
set -e

# Ensure Cargo is on PATH
if [ -f "$HOME/.cargo/env" ]; then
    source "$HOME/.cargo/env"
fi

echo "=== Building TAPEWORM (CLAP & VST3) ==="
cargo xtask bundle tapeworm --release

echo "=== Installing to ~/.clap and ~/.vst3 ==="
mkdir -p ~/.clap ~/.vst3
# Use cp --remove-destination to unlink existing file and prevent SIGBUS (BUS_ADRERR)
cp --remove-destination target/bundled/tapeworm.clap ~/.clap/
rm -rf ~/.vst3/tapeworm.vst3
cp -r target/bundled/tapeworm.vst3 ~/.vst3/

echo "=== Installation complete! ==="
echo "CLAP: ~/.clap/tapeworm.clap"
echo "VST3: ~/.vst3/tapeworm.vst3"
echo "Open Reaper and search for 'TAPEWORM' in the FX browser."
