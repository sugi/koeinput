FROM docker.io/library/archlinux:latest

# The packages that packaging/arch/PKGBUILD needs, so that scripts/build-arch can
# build without network. makepkg refuses to run as root, so the build runs as
# "builder". pacman-contrib has updpkgsums.
RUN pacman -Syu --noconfirm --needed \
      base-devel cmake extra-cmake-modules fcitx5 libpulse rust namcap git pacman-contrib \
 && pacman -Scc --noconfirm \
 && useradd -m builder \
 && rustc --version
