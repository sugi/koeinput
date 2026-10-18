# M5: More formats, CI and release

Status: approved for implementation, 2026-10-18 (the user asked for no review stop).

Part of [the overall design](2026-10-17-overall-design.md). Background decisions are in [decisions.md](../../decisions.md).
Builds on M3 (Debian packages) and M4.

## Goal

Packages build for every target from one source tree, CI checks every push, and a release can be cut from a tag.
The repository lives in a private GitHub repository; making it public stays the user's decision.

Done when:

- Packages build and pass an install test for: Debian 13, Debian testing, Ubuntu 24.04, Ubuntu 26.04 (`.deb`), Fedora 44 (`.rpm`) and Arch Linux (`PKGBUILD`).
- `README.md` (English) and `README.ja.md` (Japanese) explain what KoeInput is, how to install it, how to use it, and where the licenses and model attributions are.
- `docs/models.md` lists every catalog model with its origin and license, including the attribution CC-BY-4.0 requires for the default model.
- The repository is pushed to the private GitHub repository `sugi/koeinput`, and GitHub Actions runs the checks on push and builds all packages on a release tag (or on manual dispatch).
- `docs/release.md` gives the release steps.

Out of scope: publishing the repository, uploading to distribution archives (Debian, Fedora, AUR), arm64 packages (no no-TTS archive upstream), signing packages.

## 1. Targets

| Target | Format | Build image | Rust | fcitx5 |
|---|---|---|---|---|
| Debian 13 | `.deb` | `deb-trixie` (M3) | trixie-backports 1.96 | 5.1.12 |
| Debian testing | `.deb` | `deb-testing` (M3) | 1.96 | 5.1.23 |
| Ubuntu 24.04 | `.deb` | `deb-noble` (new) | `rustc-1.91` / `cargo-1.91` (noble-updates) | 5.1.7 |
| Ubuntu 26.04 | `.deb` | `deb-resolute` (new; use the real code name) | 1.93 | 5.1.19 |
| Fedora 44 | `.rpm` | `rpm-fedora44` (new) | 1.99 | 5.1.23 |
| Arch Linux | `PKGBUILD` | `arch` (new) | current | current |

Versions are from the M3 research (2026-10-18); the images pin the distribution release, not package versions.

### Ubuntu

- The same `debian/` directory. `debian/control` Build-Depends become `cargo (>= 1.89) | cargo-1.91, rustc (>= 1.89) | rustc-1.91`; `debian/rules` puts `/usr/lib/rust-1.91/bin` first in `PATH` when it exists.
- `scripts/build-deb` gains `noble` and the 26.04 code name. Each build gets a distribution-specific version without editing the committed changelog: the build adds a local entry `0.1.0-1~<dist>1` (for example with `dch --local ~noble --distribution <dist>` inside the build container) before `dpkg-buildpackage`. Debian 13 and testing keep `0.1.0-1` (they are the reference builds).
- `scripts/test-deb` supports the Ubuntu images.

### Fedora

- `packaging/rpm/koeinput.spec`: one source RPM, two binary RPMs `koeinput` and `fcitx5-koeinput`.
  - Sources: the same three orig tarballs as Debian (`Source0` main, `Source1` vendor, `Source2` the pinned no-TTS archive, checked by SHA-256 in `%prep`).
  - `%build`: cargo offline with the vendored crates and `SHERPA_ONNX_LIB_DIR` plus stubs (same as `debian/rules`), `scripts/check-no-gpl` on the unstripped binaries, `koeinput manpage`, the addon with `%cmake`.
  - `%check`: cargo tests and the addon's `controller` test.
  - Files: the paths of M3, with `%{_bindir}`, `%{_libexecdir}`, `%{_userunitdir}`, `%{_datadir}/dbus-1/services`, `%{_libdir}/fcitx5`, `%{_datadir}/fcitx5/addon`, `%{_mandir}`; `%license` for LICENSE-MIT, LICENSE-APACHE and the license texts; `%doc` for the README and ONNX Runtime notices.
  - No `%systemd_user_post` enabling: the unit has no `[Install]` section and is started by D-Bus activation.
  - `fcitx5-koeinput` requires `koeinput = %{version}-%{release}` and `fcitx5`.
- `scripts/build-rpm` builds in the Fedora image with `--network=none` (sources prepared by `scripts/make-orig`), runs `rpmlint` and fails on errors.
- `scripts/test-rpm`: install in a fresh `fedora:44` container, the same checks as `scripts/test-deb`.

### Arch Linux

- `packaging/arch/PKGBUILD` (split package `koeinput` and `fcitx5-koeinput`), sources: the three orig tarballs (local files for now; release URLs once public), `sha256sums` pinned; the same build steps.
- `scripts/build-arch` builds with `makepkg` as a non-root user in an `archlinux:latest` container and runs `namcap`.
- `scripts/test-arch`: `pacman -U` in a fresh container and the same checks.

## 2. Documents

- `README.md` (English) and `README.ja.md` (Japanese, natural plain Japanese): what it is (local Japanese dictation into any app through fcitx5, Mozc stays active), requirements (fcitx5, a session bus, amd64, a microphone; 2 GB RAM for the default model), install per format, model install, first use (Right Alt hold or tap; settings window), dictionary, final-stage model, privacy (everything local; recognized text never logged), troubleshooting (status, journal, restart), license and attributions, links to the developer docs.
- `docs/models.md`: per catalog model: name, origin (author/organization and the sherpa-onnx conversion), license, upstream URL; the CC-BY-4.0 attribution text for Parakeet TDT-CTC 0.6B Japanese (NVIDIA), the FunASR model license note for SenseVoice. `koeinput model list` already prints each license.
- `docs/release.md`: the release steps (versions in `Cargo.toml`, `debian/changelog`, the RPM spec, the PKGBUILD; tag `vX.Y.Z`; CI builds the packages and creates a draft GitHub release with all packages and checksums; the user reviews and publishes).

## 3. Repository and CI

- **Decided** (user, 2026-10-18): private GitHub repository `sugi/koeinput`.
  The controller creates it with `gh repo create sugi/koeinput --private` and pushes `main` (and later branches) over HTTPS with the `gh` credential helper (no SSH key prompt). No tags are pushed by the controller; releases are the user's step.
- `.github/workflows/ci.yml` on push and pull request:
  - Rust: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace`, the D-Bus tests under `dbus-run-session`, all in the `dev-trixie` image built from the repository's Containerfile;
  - MSRV: `cargo check --workspace` with Rust 1.89;
  - addon: `scripts/addon test` (Debian 13) and `KOEINPUT_DEV_IMAGE=bookworm scripts/addon test` (Debian 12);
  - GPL check on a release build.
- `.github/workflows/release.yml` on tags `v*` and `workflow_dispatch`: `scripts/make-orig`, then every package build and install test (matrix over the six targets), then (on a tag only) a draft release with the packages and a `SHA256SUMS` file. Manual dispatch uploads the packages as workflow artifacts only.
- Runners: `ubuntu-24.04` with rootless podman (the scripts already use podman). Caches for the cargo registry and the build images where it saves time.
- The controller verifies CI on GitHub (one push, one manual release dispatch) and fixes what fails.

## Testing

- Each package script has an install test in a fresh container of its distribution.
- CI runs on the pushed repository; its result is part of done.
