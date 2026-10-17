FROM docker.io/library/debian:trixie

# Debian 13 has rustc 1.85; the workspace needs 1.89, which trixie-backports has.
RUN echo 'deb http://deb.debian.org/debian trixie-backports main' \
      >/etc/apt/sources.list.d/trixie-backports.list \
 && apt-get update \
 && apt-get install -y --no-install-recommends \
      build-essential devscripts lintian fakeroot debhelper pkgconf \
      libpulse-dev cmake extra-cmake-modules gettext \
      libfcitx5core-dev libfcitx5config-dev libfcitx5utils-dev fcitx5-modules-dev \
      binutils tini \
 && apt-get install -y --no-install-recommends -t trixie-backports cargo rustc \
 && rm -rf /var/lib/apt/lists/* \
 && rustc --version \
 && dpkg --compare-versions "$(rustc --version | cut -d' ' -f2)" ge 1.89
