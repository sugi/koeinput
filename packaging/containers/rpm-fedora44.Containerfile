FROM docker.io/library/fedora:44

# The build tools and the BuildRequires of packaging/rpm/koeinput.spec; the build
# runs without network, and rpmbuild stops when one of them is missing.
RUN dnf install -y --setopt=install_weak_deps=False \
      rpm-build rpmlint tini glibc-langpack-en \
      gcc gcc-c++ cmake extra-cmake-modules gettext pkgconf-pkg-config \
      rust cargo binutils systemd-rpm-macros \
      fcitx5-devel pulseaudio-libs-devel \
 && dnf clean all \
 && rustc --version \
 && ver=$(rustc --version | cut -d' ' -f2) \
 && [ "$(printf '1.89\n%s\n' "$ver" | sort -V | head -1)" = 1.89 ]
