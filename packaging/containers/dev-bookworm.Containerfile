FROM docker.io/library/debian:bookworm

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      build-essential pkg-config cmake extra-cmake-modules gettext dpkg-dev file dbus \
      libfcitx5core-dev libfcitx5config-dev libfcitx5utils-dev \
      fcitx5-modules-dev fcitx5-modules fcitx5 \
 && rm -rf /var/lib/apt/lists/*
