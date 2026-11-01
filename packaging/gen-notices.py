#!/usr/bin/env python3
"""Generate THIRD_PARTY_NOTICES.md and DEPENDENCY_MANIFEST.md from the exact locked dependency sources.

usage: gen-notices.py <linked.txt> <metadata.json> <Cargo.lock> <container-cargo-home> <host-cargo-home>
                      <upstream-licenses-dir> <out-dir>

linked.txt     `cargo tree -p tapeworm -e normal --target x86_64-unknown-linux-gnu --prefix none -f "{p}|{l}"`
metadata.json  `cargo metadata --locked --offline --format-version 1` (paths as seen inside the build container)
upstream-licenses-dir  licence files fetched from upstream at the exact released tag/revision for crates whose
               published package omits them (see upstream-licenses/SOURCES.txt); nothing here is invented.
"""
import glob, json, os, re, sys, tomllib

linked_txt, meta_json, lock, c_home, h_home, upstream_dir, out = sys.argv[1:8]
meta = json.load(open(meta_json))
pk = {}
for p in meta["packages"]:
    pk.setdefault((p["name"], p["version"]), []).append(p)
lock_pk = {}
for p in tomllib.load(open(lock, "rb"))["package"]:
    lock_pk.setdefault((p["name"], p["version"]), []).append(p)

linked = []
for line in open(linked_txt):
    spec, lic = line.rstrip("\n").split("|", 1)
    lic = lic.replace(" (*)", "")
    m = re.match(r"(\S+) v(\S+)(?: \(proc-macro\))?(?: \((.+)\))?", spec)
    name, ver, src = m.group(1), m.group(2), m.group(3) or ""
    if name == "tapeworm":
        continue
    linked.append((name, ver, lic, src, "(proc-macro)" in spec))
linked = sorted(set(linked))

def host(p):
    return p.replace(c_home, h_home)

def lic_files(d):
    pats = ["LICEN[CS]E*", "licen[cs]e*", "COPYING*", "NOTICE*", "COPYRIGHT*", "UNLICENSE*"]
    return sorted({f for pat in pats for f in glob.glob(os.path.join(d, pat)) if os.path.isfile(f)})

def find_pkg(name, ver, src):
    cands = pk.get((name, ver), [])
    if len(cands) > 1 and src:  # e.g. two baseview git revisions
        rev = src.split("#")[-1]
        cands = [c for c in cands if rev in (c.get("source") or "")] or cands
    return cands[0] if cands else None

upstream_sources = {}
if os.path.exists(os.path.join(upstream_dir, "SOURCES.txt")):
    for l in open(os.path.join(upstream_dir, "SOURCES.txt")):
        if l.strip() and not l.startswith("#"):
            key, url, sha = l.split()
            upstream_sources.setdefault(key, []).append((url, sha))

rows, sections, gaps, apache_text = [], [], [], None
for name, ver, lic, src, macro in linked:
    p = find_pkg(name, ver, src)
    d = os.path.dirname(host(p["manifest_path"]))
    files = lic_files(d)
    origin = "crate package"
    if not files and src and "github.com" in src:  # workspace member of a git repo: licence at repo root
        up = d
        for _ in range(3):
            up = os.path.dirname(up)
            files = lic_files(up)
            if files:
                origin = "git repository root (same pinned revision)"
                break
    key = f"{name}-{ver}"
    if not files and os.path.isdir(os.path.join(upstream_dir, key)):
        files = sorted(glob.glob(os.path.join(upstream_dir, key, "*")))
        origin = "upstream repository at the released tag (" + ", ".join(u for u, _ in upstream_sources.get(key, [])) + ")"
    if not files:
        gaps.append(f"{name} {ver} ({lic})")
    for f in files:
        t = open(f, encoding="utf-8", errors="replace").read()
        if apache_text is None and "Apache License" in t and "Version 2.0, January 2004" in t and "END OF TERMS" in t:
            apache_text = t.strip()
    src_disp = src or (p.get("source") or "")
    lk = next((l for l in lock_pk.get((name, ver), []) if not src or src.split("#")[-1] in l.get("source", "")), {})
    rows.append(dict(name=name, ver=ver, lic=lic, src=src_disp or "crates.io", macro=macro,
                     checksum=lk.get("checksum", ""), lock_source=lk.get("source", ""), files=files, origin=origin))

# ---------------------------------------------------------------- THIRD_PARTY_NOTICES.md
N = ["# Third-Party Notices", "",
     "TAPEWORM 0.1.0 for Linux x86_64 (`tapeworm.clap` and `tapeworm.vst3/Contents/x86_64-linux/tapeworm.so`,",
     "which are the same binary) contains the third-party components listed below. Each component remains under",
     "its own licence; distributing TAPEWORM under GPL-3.0-or-later does not change those licences.", "",
     "This file was generated from the exact dependency sources pinned by `Cargo.lock` and used for the release",
     "build (`build_environment/gen-notices.py`). Build-time-only tools (`xtask`, `nih_plug_xtask`, build scripts)",
     "are not part of the binary and are not listed; procedural-macro crates are listed for completeness although",
     "they run only at compile time.", "",
     "## Summary of notable licences", "",
     "* **vst3-sys** (fork `robbert-vdh/vst3-sys`, branch `fix/drop-box-from-raw`, rev `b3ff4d77`) is licensed under",
     "  the **GNU GPL version 3 or later**. Because the VST3 and CLAP entry points are in one binary, every TAPEWORM",
     "  binary is a GPL-3.0-or-later combined work. vst3-sys is an independent Rust binding; no Steinberg VST3 SDK",
     "  source code is used. VST is a registered trademark of Steinberg Media Technologies GmbH.",
     "* **nih-plug**, **nih_plug_egui**, **nih_plug_derive** (rev `de421011`): ISC licence.",
     "* **CLAP bindings** (`clap-sys`): MIT OR Apache-2.0.",
     "* **egui** 0.31.1 family: MIT OR Apache-2.0.",
     "* **Fonts compiled into the binary** (from `epaint_default_fonts` 0.31.1, used by the egui text renderer):",
     "  Ubuntu-Light (Ubuntu Font Licence 1.0), Hack Regular (MIT-style Hack licence incorporating the Bitstream",
     "  Vera and DejaVu terms), Noto Emoji Regular (SIL Open Font License 1.1), emoji-icon-font (MIT). Their full",
     "  licence texts are in the `licenses/fonts/` directory distributed with TAPEWORM and are reproduced below.",
     "  These fonts are not relicensed under the GPL.",
     "* Apache-2.0-only crates (`ab_glyph`, `ab_glyph_rasterizer`, `owned_ttf_parser`, `cpal`, `gethostname`):",
     "  the Apache License 2.0 text is reproduced once at the end of this file.",
     "* Where a crate is offered under a choice of licences (for example `MIT OR Apache-2.0`), the licence files",
     "  shipped with that crate are reproduced as provided; the full Apache-2.0 text appears once at the end.", "",
     "## Component inventory", "",
     "| Component | Version | Licence (as declared) | Source |", "|---|---|---|---|"]
for r in rows:
    N.append(f"| {r['name']}{' (proc-macro)' if r['macro'] else ''} | {r['ver']} | {r['lic']} | {r['src']} |")
N += ["", "## Licence and notice texts", ""]
for r in rows:
    N.append(f"### {r['name']} {r['ver']}")
    N.append("")
    N.append(f"Declared licence: `{r['lic']}`. Notice files taken from: {r['origin']}.")
    N.append("")
    for f in r["files"]:
        t = open(f, encoding="utf-8", errors="replace").read().strip()
        if apache_text and "Version 2.0, January 2004" in t and "END OF TERMS" in t and len(t) > 5000:
            t = "(Apache License, Version 2.0 — full text reproduced once at the end of this file.)"
        N += [f"**{os.path.basename(f)}**", "", "```text", t, "```", ""]
fd = glob.glob(os.path.join(h_home, "registry/src/*/epaint_default_fonts-0.31.1/fonts"))[0]
N += ["## Embedded fonts (epaint_default_fonts 0.31.1)", "",
      "| Font file compiled into the binary | Licence | Licence text |", "|---|---|---|",
      "| Ubuntu-Light.ttf | Ubuntu Font Licence 1.0 | `licenses/fonts/UFL.txt` |",
      "| Hack-Regular.ttf | Hack licence (MIT) + Bitstream Vera + DejaVu changes | `licenses/fonts/Hack-Regular.txt` |",
      "| NotoEmoji-Regular.ttf | SIL Open Font License 1.1 | `licenses/fonts/OFL.txt` |",
      "| emoji-icon-font.ttf | MIT | `licenses/fonts/emoji-icon-font-mit-license.txt` |", ""]
for f in ("UFL.txt", "Hack-Regular.txt", "OFL.txt", "emoji-icon-font-mit-license.txt"):
    N += [f"### {f}", "", "```text", open(os.path.join(fd, f), encoding="utf-8", errors="replace").read().strip(), "```", ""]
N += ["## Apache License, Version 2.0", "", "```text", apache_text, "```", ""]
os.makedirs(out, exist_ok=True)
open(os.path.join(out, "THIRD_PARTY_NOTICES.md"), "w").write("\n".join(N) + "\n")
os.makedirs(os.path.join(out, "licenses", "fonts"), exist_ok=True)
for f in ("UFL.txt", "Hack-Regular.txt", "OFL.txt", "emoji-icon-font-mit-license.txt"):
    open(os.path.join(out, "licenses", "fonts", f), "w").write(open(os.path.join(fd, f), encoding="utf-8").read())

# ---------------------------------------------------------------- DEPENDENCY_MANIFEST.md
M = ["# Dependency Manifest", "",
     "Every package compiled into the TAPEWORM 0.1.0 Linux x86_64 binary, exactly as pinned by `Cargo.lock`.",
     "crates.io packages are identified by the SHA-256 recorded in `Cargo.lock`; git packages by full commit.",
     "All of them are included in `tapeworm-v0.1.0-source.tar.gz` under `cargo-home/` so the release can be",
     "rebuilt offline (see BUILDING.md).", "",
     f"Packages: {len(rows)} (crates.io: {sum(1 for r in rows if r['lock_source'].startswith('registry'))}, "
     f"git: {sum(1 for r in rows if r['lock_source'].startswith('git'))}).", "",
     "| Package | Version | Licence | Source (Cargo.lock) | Checksum / commit |", "|---|---|---|---|---|"]
for r in rows:
    s = r["lock_source"]
    ident = r["checksum"] or (s.split("#")[-1] if "#" in s else "")
    disp = "crates.io" if s.startswith("registry") else s.split("#")[0]
    M.append(f"| {r['name']} | {r['ver']} | {r['lic']} | {disp} | `{ident}` |")
open(os.path.join(out, "DEPENDENCY_MANIFEST.md"), "w").write("\n".join(M) + "\n")
print(f"packages={len(rows)} gaps={len(gaps)} apache_text={'yes' if apache_text else 'NO'}")
for g in gaps:
    print("GAP", g)
