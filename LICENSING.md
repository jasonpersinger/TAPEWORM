# Licensing

## TAPEWORM

Copyright (C) 2026 Jason Persinger.

TAPEWORM is free software: you can redistribute it and/or modify it under the terms of the GNU General
Public License as published by the Free Software Foundation, either version 3 of the License, or (at
your option) any later version (SPDX: `GPL-3.0-or-later`).

TAPEWORM is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the
implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public
License in [LICENSE](LICENSE) for more details.

This applies to TAPEWORM's original material:

* the Rust source code (`src/`, including the DSP in `src/dsp/` and the editor),
* the editor artwork (`assets/tapeworm_layout_clean.png` and its compressed form
  `assets/tapeworm_layout.bin`, which is compiled into the plug-in),
* the 5×7 bitmap lettering in `src/editor_pixel_font.rs`,
* the build and packaging scripts (`packaging/`, `xtask/`, `build_and_install.sh`), tests and
  documentation.

## Third-party components

TAPEWORM is built on third-party libraries and embeds four fonts. They keep their own licences, which
are reproduced in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) and, for the fonts, in
`licenses/fonts/`. Distributing TAPEWORM under the GPL does not relicense any of them.

Two points matter for anyone redistributing TAPEWORM:

* **The VST3 bindings (`vst3-sys`) are licensed GPL-3.0-or-later.** The CLAP and VST3 versions of
  TAPEWORM are the same compiled library, so every TAPEWORM binary — including the CLAP file on its
  own — is a combined work that must be distributed under the GPL version 3 or later.
* **Fonts compiled into the editor:** Ubuntu Light (Ubuntu Font Licence 1.0), Hack (Hack / MIT licence
  with Bitstream Vera and DejaVu terms), Noto Emoji (SIL Open Font License 1.1) and emoji-icon-font
  (MIT). These licences permit embedding and redistribution; keep their licence texts with the binary.

All other dependencies use permissive licences (MIT, Apache-2.0, ISC, BSD, Zlib, Unicode-3.0,
BlueOak-1.0.0, Unlicense) that are compatible with GPL version 3.

## Your rights as a recipient

If you receive TAPEWORM, you receive the rights the GPL grants: you may run it for any purpose
(including commercial music production — the GPL places no conditions on the audio you make with it),
study and modify the source, and share original or modified copies. When you distribute binaries,
you must license them under the GPL, keep these notices, and make the complete corresponding source
available to your recipients.

## Selling TAPEWORM

The GPL allows charging for copies of TAPEWORM, for downloads, bundles or support. Anyone who receives a
copy, paid or free, receives the full GPL rights described above, including the right to redistribute
it. No licence keys, copy protection or terms that restrict those rights may be added to the GPL-covered
work.

## Corresponding source

Every official binary release is published together with `tapeworm-v<version>-source.tar.gz`, which
contains the exact TAPEWORM source the binaries were built from, the build scripts, and the source of
every dependency pinned by `Cargo.lock`, so the release can be rebuilt without network access. See
[BUILDING.md](BUILDING.md). The Rust toolchain (1.99.1) and the Debian 12 build image are publicly
available and identified by version and digest in `packaging/`.

## Trademarks

"TAPEWORM" and the vendor name shown by the plug-in identify this project; the GPL does not grant
trademark rights. VST is a registered trademark of Steinberg Media Technologies GmbH. No Steinberg VST3
SDK source code is used.
