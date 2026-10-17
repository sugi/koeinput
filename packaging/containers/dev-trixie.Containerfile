FROM docker.io/library/debian:trixie

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      build-essential pkg-config curl ca-certificates git \
      libpulse-dev dbus \
      cmake extra-cmake-modules gettext dpkg-dev file \
      libfcitx5core-dev libfcitx5config-dev libfcitx5utils-dev \
      fcitx5-modules-dev fcitx5-modules fcitx5 \
 && rm -rf /var/lib/apt/lists/*

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal \
      --default-toolchain 1.95.0 --component clippy,rustfmt \
 && chmod -R a+rwX /usr/local/cargo /usr/local/rustup
