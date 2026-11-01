#!/usr/bin/env bash
# Run a command inside an extracted OCI root filesystem without Docker/Podman or root,
# using bubblewrap inside an unprivileged user namespace created by `unshare --map-root-user --map-auto`
# (uid 0 inside = invoking user; uids 1..65536 inside = the user's /etc/subuid range, so dpkg can chown).
#
#   run-rootless.sh ROOTFS [--no-net] [--bind HOST:GUEST]... [--x11] -- COMMAND...
#
# --no-net   isolate the network namespace (used to prove offline builds)
# --x11      share the host X server ($DISPLAY, /tmp/.X11-unix, $XAUTHORITY) for GUI/DAW tests; keeps the
#            host IPC namespace, because X11 MIT-SHM needs shared SysV memory with the X server
set -euo pipefail
ROOTFS=$(realpath "$1"); shift
args=(--bind "$ROOTFS" / --proc /proc --dev /dev --tmpfs /tmp --tmpfs /run
      --ro-bind /etc/resolv.conf /etc/resolv.conf
      --unshare-pid --unshare-uts --hostname tapeworm-build
      --die-with-parent --clearenv
      --setenv HOME /root --setenv PATH /root/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
      --setenv LANG C.UTF-8 --setenv TZ UTC --setenv SOURCE_DATE_EPOCH "${SOURCE_DATE_EPOCH:-0}"
      --setenv DEBIAN_FRONTEND noninteractive)
x11=0
while [ $# -gt 0 ]; do
  case "$1" in
    --no-net) args+=(--unshare-net); shift ;;
    --bind) args+=(--bind "${2%%:*}" "${2#*:}"); shift 2 ;;
    --x11) x11=1; args+=(--ro-bind /tmp/.X11-unix /tmp/.X11-unix --setenv DISPLAY "${DISPLAY:-:1}")
           if [ -n "${XAUTHORITY:-}" ]; then args+=(--ro-bind "$XAUTHORITY" /tmp/.Xauthority --setenv XAUTHORITY /tmp/.Xauthority); fi
           shift ;;
    --) shift; break ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done
[ "$x11" = 1 ] || args+=(--unshare-ipc)
exec unshare --map-root-user --map-auto -- bwrap "${args[@]}" "$@"
