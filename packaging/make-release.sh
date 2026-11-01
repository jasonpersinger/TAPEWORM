#!/usr/bin/env bash
# Build and package a TAPEWORM Linux x86_64 release in a pinned Debian 12 environment.
#
#   packaging/make-release.sh [--out DIR] [--runner rootless|docker|native] [--rev GIT_REV]
#
# Runners: docker (Dockerfile), rootless (bubblewrap; same image + provisioning), native (already inside the
# provisioned Debian 12 image, e.g. a CI container job; the offline step relies on --offline, not a
# disabled network).
#
# Produces in DIR (default: ./dist):
#   tapeworm-v<VER>-linux-x86_64-clap.tar.gz
#   tapeworm-v<VER>-linux-x86_64-vst3.tar.gz
#   tapeworm-v<VER>-source.tar.gz      complete Corresponding Source: TAPEWORM tree + every locked
#                                      dependency (cargo-home/) for an offline rebuild
#   SHA256SUMS, BUILD_MANIFEST.txt, logs/
#
# Nothing is uploaded or published. Requires Linux x86_64 and either Docker, or bubblewrap + util-linux
# `unshare` with unprivileged user namespaces and a /etc/subuid range (the "rootless" runner).
set -euo pipefail

IMAGE_REPO=library/debian
IMAGE_DIGEST=sha256:a0039cc6bb3eb2215aabcd3437cce8808a3858d86bf483eee2ee14fe4d2551c2   # debian:bookworm (12.15), linux/amd64
GLIBC_MAX=2.36
ALLOWED_NEEDED="libxcb.so.1 libGL.so.1 libX11-xcb.so.1 libX11.so.6 libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2"

HERE=$(cd "$(dirname "$0")" && pwd); ROOT=$(cd "$HERE/.." && pwd)
OUT=$ROOT/dist; RUNNER=; REV=
while [ $# -gt 0 ]; do
  case "$1" in
    --out) OUT=$(realpath -m "$2"); shift 2 ;;
    --runner) RUNNER=$2; shift 2 ;;
    --rev) REV=$2; shift 2 ;;
    *) echo "usage: $0 [--out DIR] [--runner rootless|docker|native] [--rev GIT_REV]" >&2; exit 2 ;;
  esac
done
[ -n "$RUNNER" ] || { command -v docker >/dev/null 2>&1 && RUNNER=docker || RUNNER=rootless; }
VER=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)
NAME=tapeworm-v$VER
WORK=$OUT/.work; mkdir -p "$OUT/logs"
[ ! -e "$WORK" ] || { echo "refusing: $WORK exists from an earlier run; remove it first" >&2; exit 1; }
mkdir -p "$WORK"/{build/src,build/cargo-home,build/target,build/out}
log() { echo "== $*" | tee -a "$OUT/logs/make-release.log"; }

# ------------------------------------------------------------ 1. source export
if [ -n "$REV" ] || { git -C "$ROOT" rev-parse --git-dir >/dev/null 2>&1 && [ -z "$(git -C "$ROOT" status --porcelain)" ]; }; then
  REV=${REV:-HEAD}
  SRC_REV=$(git -C "$ROOT" rev-parse "$REV^{commit}")
  GIT_TREE=$(git -C "$ROOT" rev-parse "$SRC_REV^{tree}")
  SOURCE_DATE_EPOCH=$(git -C "$ROOT" log -1 --format=%ct "$SRC_REV")
  git -C "$ROOT" archive --format=tar "$SRC_REV" | tar -x -C "$WORK/build/src"
elif git -C "$ROOT" rev-parse --git-dir >/dev/null 2>&1 && git -C "$ROOT" diff --quiet \
     && [ -z "$(git -C "$ROOT" ls-files --others --exclude-standard)" ]; then
  # Staged but not yet committed: the git *tree* id is independent of author/date, so a later commit of
  # exactly this index has the same tree id and can be matched to these binaries.
  GIT_TREE=$(git -C "$ROOT" write-tree)
  SRC_REV="staged-tree-$GIT_TREE"
  : "${SOURCE_DATE_EPOCH:?set SOURCE_DATE_EPOCH for an uncommitted build}"
  git -C "$ROOT" archive --format=tar "$GIT_TREE" | tar -x -C "$WORK/build/src"
else
  echo "refusing: commit or stage (git add -A) the source first, so the build has a recorded identity" >&2; exit 1
fi
export SOURCE_DATE_EPOCH
TREE_SHA=$(cd "$WORK/build/src" && find . -type f -print0 | LC_ALL=C sort -z | xargs -0 sha256sum | sha256sum | cut -d' ' -f1)
log "version $VER  source revision $SRC_REV  git tree $GIT_TREE  tree-sha256 $TREE_SHA  runner $RUNNER"

# ------------------------------------------------------------ 2. build environment
if [ "$RUNNER" = docker ]; then
  docker build -t tapeworm-build:debian12 -f "$HERE/Dockerfile" "$HERE" 2>&1 | tee "$OUT/logs/provision.log"
  run() { local net=$1; shift
          docker run --rm ${net:+--network=none} -e SOURCE_DATE_EPOCH -v "$WORK/build:/build" tapeworm-build:debian12 "$@"; }
elif [ "$RUNNER" = native ]; then
  [ -f /root/toolchain-manifest.txt ] && [ "$(id -u)" = 0 ] || { echo "native runner: run as root inside the provisioned image" >&2; exit 1; }
  ln -sfn "$WORK/build" /build
  run() { shift; "$@"; }
else
  ROOTFS=$OUT/.rootfs-debian12
  if [ ! -f "$ROOTFS/root/toolchain-manifest.txt" ]; then
    [ ! -e "$ROOTFS" ] || { echo "refusing: incomplete $ROOTFS; remove it first" >&2; exit 1; }
    python3 -I "$HERE/oci-fetch.py" "$IMAGE_REPO" "$IMAGE_DIGEST" "$ROOTFS" | tee "$OUT/logs/provision.log"
    "$HERE/run-rootless.sh" "$ROOTFS" --bind "$HERE:/be" -- sh /be/provision-debian12.sh >> "$OUT/logs/provision.log" 2>&1
  fi
  run() { local net=$1; shift
          "$HERE/run-rootless.sh" "$ROOTFS" ${net:+--no-net} --bind "$HERE:/be" --bind "$WORK/build:/build" -- "$@"; }
fi
BR=/be/build-release.sh; [ "$RUNNER" = docker ] && BR=/usr/local/bin/build-release.sh; [ "$RUNNER" = native ] && BR=$HERE/build-release.sh

# ------------------------------------------------------------ 3-4. fetch (online), test + build (offline)
log "fetching locked dependencies"
run "" bash $BR fetch > "$OUT/logs/fetch.log" 2>&1 || { tail -40 "$OUT/logs/fetch.log"; exit 1; }
log "testing and building offline"
run offline bash $BR build > "$OUT/logs/build.log" 2>&1 || { tail -40 "$OUT/logs/build.log"; exit 1; }
grep -E '^test result' "$WORK/build/out/test.log" | tee -a "$OUT/logs/make-release.log"
CLAP=$WORK/build/out/bundled/tapeworm.clap
VSO=$WORK/build/out/bundled/tapeworm.vst3/Contents/x86_64-linux/tapeworm.so

# ------------------------------------------------------------ 5. ELF checks
log "checking ELF requirements"
for f in "$CLAP" "$VSO"; do
  file -b "$f" | grep -q 'ELF 64-bit LSB shared object, x86-64' || { echo "FAIL: $f is not an x86-64 shared object"; exit 1; }
  max=$(objdump -T "$f" | grep -oE 'GLIBC_[0-9]+\.[0-9]+(\.[0-9]+)?' | sed 's/GLIBC_//' | sort -uV | tail -1)
  [ "$(printf '%s\n%s\n' "$max" "$GLIBC_MAX" | sort -V | tail -1)" = "$GLIBC_MAX" ] || { echo "FAIL: $f needs GLIBC_$max > $GLIBC_MAX"; exit 1; }
  for n in $(readelf -d "$f" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p'); do
    case " $ALLOWED_NEEDED " in *" $n "*) ;; *) echo "FAIL: unexpected dependency $n in $f"; exit 1 ;; esac
  done
  ! readelf -d "$f" | grep -qE 'RPATH|RUNPATH' || { echo "FAIL: RPATH/RUNPATH in $f"; exit 1; }
  echo "$(basename "$f"): max GLIBC_$max, NEEDED: $(readelf -d "$f" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p' | tr '\n' ' ')" \
    | tee -a "$OUT/logs/make-release.log"
done

# ------------------------------------------------------------ 6. packages
TAR() { tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner --format=gnu "$@"; }
docs() { local d=$1
  cp "$WORK/build/src/"{LICENSE,LICENSING.md,THIRD_PARTY_NOTICES.md,README.md,INSTALLING.md,RELEASE_NOTES.md} "$d/"
  cp -r "$WORK/build/src/licenses" "$d/"
  printf 'TAPEWORM %s for Linux x86_64\nSource revision: %s\nSource git tree: %s\nSource file-list sha256: %s\n\nThe complete corresponding source code is %s-source.tar.gz,\npublished in the same place as this file. See LICENSING.md.\n' \
    "$VER" "$SRC_REV" "$GIT_TREE" "$TREE_SHA" "$NAME" > "$d/SOURCE.txt"
}
P=$WORK/pkg; mkdir -p "$P/$NAME-linux-x86_64-clap" "$P/$NAME-linux-x86_64-vst3"
cp "$CLAP" "$P/$NAME-linux-x86_64-clap/"; docs "$P/$NAME-linux-x86_64-clap"
cp -r "$WORK/build/out/bundled/tapeworm.vst3" "$P/$NAME-linux-x86_64-vst3/"; docs "$P/$NAME-linux-x86_64-vst3"
chmod -R u=rwX,go=rX "$P"
# shared objects 0755 (the bundler copies them 0644; dlopen does not need +x, but ldd and file managers expect it)
chmod 0755 "$P/$NAME-linux-x86_64-clap/tapeworm.clap" "$P/$NAME-linux-x86_64-vst3/tapeworm.vst3/Contents/x86_64-linux/tapeworm.so"
for k in clap vst3; do (cd "$P" && TAR -cf - "$NAME-linux-x86_64-$k" | gzip -n -9 > "$OUT/$NAME-linux-x86_64-$k.tar.gz"); done

log "packaging corresponding source (+ locked dependency sources)"
S=$WORK/srcpkg/$NAME-source; mkdir -p "$S/cargo-home/registry" "$S/cargo-home/git"
cp -a "$WORK/build/src" "$S/tapeworm-$VER"
cp -a "$WORK/build/cargo-home/registry/index" "$WORK/build/cargo-home/registry/cache" "$S/cargo-home/registry/"
cp -a "$WORK/build/cargo-home/git/db" "$S/cargo-home/git/"
cat > "$S/README-SOURCE.txt" <<EOF
TAPEWORM $VER — complete corresponding source

tapeworm-$VER/   the TAPEWORM source tree (source revision $SRC_REV, git tree $GIT_TREE)
cargo-home/      every dependency pinned by tapeworm-$VER/Cargo.lock, as Cargo stores it
                 (crates.io packages and git object databases); no network access is needed.

Rebuild exactly as the release was built (see tapeworm-$VER/BUILDING.md, "Rebuilding from the source
archive"): bind tapeworm-$VER to /build/src and cargo-home to /build/cargo-home in the Debian 12 build
environment and run packaging/build-release.sh build. Without the container:
    CARGO_HOME=\$PWD/cargo-home CARGO_NET_OFFLINE=true \\
        cargo xtask bundle tapeworm --release --locked --offline     (run inside tapeworm-$VER)
EOF
chmod -R u=rwX,go=rX "$WORK/srcpkg"
(cd "$WORK/srcpkg" && TAR -cf - "$NAME-source" | gzip -n -9 > "$OUT/$NAME-source.tar.gz")

# ------------------------------------------------------------ 7. checksums + manifest
cp "$WORK/build/src/RELEASE_NOTES.md" "$OUT/RELEASE_NOTES.md"
(cd "$OUT" && sha256sum "$NAME"-*.tar.gz > SHA256SUMS && sha256sum -c SHA256SUMS)
{
  echo "TAPEWORM $VER release build manifest"
  echo "source_revision: $SRC_REV"; echo "source_git_tree: $GIT_TREE"; echo "source_tree_sha256: $TREE_SHA"; echo "source_date_epoch: $SOURCE_DATE_EPOCH"
  echo "runner: $RUNNER"; echo "image: $IMAGE_REPO@$IMAGE_DIGEST"; echo "glibc_ceiling_checked: $GLIBC_MAX"
  echo "--- binaries"; cat "$WORK/build/out/bundled.sha256"
  echo "--- archives"; cat "$OUT/SHA256SUMS"
  echo "--- toolchain"; cat "$WORK/build/out/toolchain-manifest.txt"
} > "$OUT/BUILD_MANIFEST.txt"
cp "$WORK/build/out/"{test.log,bundle.log} "$OUT/logs/"
log "done: $OUT"; cat "$OUT/SHA256SUMS"
