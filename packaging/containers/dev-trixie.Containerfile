FROM docker.io/library/debian:trixie

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      build-essential pkg-config curl ca-certificates git python3 \
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

# Makes the orig-vendor tarball for the Debian source package (scripts/make-orig).
# The install leaves root-owned cache and lock files that would stop cargo running
# as the user; scripts/dev mounts its own registry anyway.
RUN cargo install --locked cargo-vendor-filterer --version 0.5.18 \
 && rm -rf /usr/local/cargo/registry /usr/local/cargo/.package-cache* /usr/local/cargo/.global-cache*
