FROM docker.io/library/ubuntu:24.04

# Ubuntu 24.04 has rustc 1.75; the workspace needs 1.89. The versioned rustc-1.91
# and cargo-1.91 packages (noble-updates, universe) install to /usr/lib/rust-1.91/bin,
# which debian/rules puts first in PATH.
# libdistro-info-perl lets dch in scripts/build-deb check the release name.
RUN export DEBIAN_FRONTEND=noninteractive \
 && apt-get update \
 && apt-get install -y --no-install-recommends \
      build-essential devscripts lintian fakeroot debhelper pkgconf \
      libpulse-dev cmake extra-cmake-modules gettext \
      libfcitx5core-dev libfcitx5config-dev libfcitx5utils-dev fcitx5-modules-dev \
      binutils tini libdistro-info-perl rustc-1.91 cargo-1.91 \
 && rm -rf /var/lib/apt/lists/* \
 && /usr/lib/rust-1.91/bin/rustc --version \
 && /usr/lib/rust-1.91/bin/cargo --version \
 && dpkg --compare-versions "$(/usr/lib/rust-1.91/bin/rustc --version | cut -d' ' -f2)" ge 1.89
