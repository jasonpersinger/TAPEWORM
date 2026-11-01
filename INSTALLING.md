# Installing TAPEWORM on Linux

## System requirements

* x86_64 (64-bit Intel/AMD) Linux with **glibc 2.35 or newer**. Check with `ldd --version`.
  Tested with REAPER: Debian 12 (2.36), Ubuntu 24.04 LTS (2.39), CachyOS (2.44); loading and audio
  processing also checked on Ubuntu 22.04 LTS (2.35).
* An X11 or XWayland desktop with OpenGL. The plug-in uses `libGL.so.1`, `libX11.so.6`,
  `libX11-xcb.so.1` and `libxcb.so.1`, which every desktop installation already has.
* A CLAP or VST3 host. Tested with REAPER 7.83.

## Install for your user (recommended, no root needed)

**CLAP**

```sh
tar -xzf tapeworm-v0.1.0-linux-x86_64-clap.tar.gz
mkdir -p ~/.clap
cp tapeworm-v0.1.0-linux-x86_64-clap/tapeworm.clap ~/.clap/
```

**VST3**

```sh
tar -xzf tapeworm-v0.1.0-linux-x86_64-vst3.tar.gz
mkdir -p ~/.vst3
cp -r tapeworm-v0.1.0-linux-x86_64-vst3/tapeworm.vst3 ~/.vst3/
```

The VST3 bundle must keep its directory structure:
`~/.vst3/tapeworm.vst3/Contents/x86_64-linux/tapeworm.so`.

Install one format or both. If both are installed, your host lists TAPEWORM twice (once per format);
they sound identical.

**Updating:** quit your host first, then copy the new files over the old ones. Replacing the plug-in
file while a host has it loaded can crash that host.

System-wide installation (`/usr/lib/clap`, `/usr/lib/vst3`) also works but needs root and is not
necessary.

## Make your host find it

**REAPER:** *Options → Preferences → Plug-ins → CLAP* (or *VST*), check that `~/.clap` (or `~/.vst3`) is
in the path list, then click **Re-scan** (or *Clear cache/re-scan* if an older TAPEWORM was installed).
Open the FX browser and search for **TAPEWORM**. It appears as `CLAP: TAPEWORM (MESSYQUEST)` and
`VST3: TAPEWORM (MESSYQUEST)`.

Other hosts: use the host's plug-in rescan function; `~/.clap` and `~/.vst3` are the standard per-user
locations.

## Verify the installation

```sh
sha256sum -c SHA256SUMS                      # in the download folder, before extracting
ldd ~/.clap/tapeworm.clap | grep "not found" # prints nothing when all libraries are present
```

Then insert TAPEWORM on a track, play audio and open its editor.

## Uninstall

```sh
rm ~/.clap/tapeworm.clap
rm -r ~/.vst3/tapeworm.vst3
```

then rescan plug-ins in your host. Projects that used TAPEWORM will show it as missing.

## Troubleshooting

* **TAPEWORM does not appear after a rescan.** Check the file location (above) and that the scan paths
  include `~/.clap` / `~/.vst3`. In REAPER, *Clear cache/re-scan* forces a fresh scan.
* **`version 'GLIBC_2.xx' not found`** (shown in a terminal-started host or in `ldd -v`): your system's C
  library is older than 2.35. Update the distribution, or build TAPEWORM yourself (see BUILDING.md).
* **`libGL.so.1` (or `libX11…`, `libxcb…`) `=> not found`** on a minimal or server system: install the
  X11/OpenGL client libraries, e.g. on Debian/Ubuntu
  `sudo apt install libgl1 libx11-6 libx11-xcb1 libxcb1`.
* **The editor window stays black or does not open.** The editor needs a working OpenGL driver on
  X11/XWayland. Check `glxinfo -B` (package `mesa-utils`). Audio processing works without the editor.
* **Wayland:** the editor is an X11 window, so it needs XWayland (enabled by default on GNOME and KDE).
