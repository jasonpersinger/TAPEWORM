# TAPEWORM release build environment: Debian 12 (glibc 2.36), Rust 1.99.1, pinned by digest.
# Used by packaging/make-release.sh --runner docker. The rootless runner (run-rootless.sh) extracts the
# same image digest and runs the same provision-debian12.sh, so both produce the same environment.
FROM debian:bookworm@sha256:a0039cc6bb3eb2215aabcd3437cce8808a3858d86bf483eee2ee14fe4d2551c2
ENV DEBIAN_FRONTEND=noninteractive LANG=C.UTF-8 TZ=UTC HOME=/root
COPY provision-debian12.sh /tmp/provision-debian12.sh
RUN sh /tmp/provision-debian12.sh && rm /tmp/provision-debian12.sh
COPY build-release.sh /usr/local/bin/build-release.sh
WORKDIR /build
