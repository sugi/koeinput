FROM docker.io/library/debian:testing

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      build-essential devscripts lintian fakeroot debhelper pkgconf \
      libpulse-dev cmake extra-cmake-modules gettext \
      libfcitx5core-dev libfcitx5config-dev libfcitx5utils-dev fcitx5-modules-dev \
      binutils tini cargo rustc \
 && rm -rf /var/lib/apt/lists/* \
 && rustc --version \
 && dpkg --compare-versions "$(rustc --version | cut -d' ' -f2)" ge 1.89
