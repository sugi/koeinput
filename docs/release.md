# Release steps

A release is a tag `vX.Y.Z`. CI builds the packages for every target and creates a draft GitHub release; you review and publish it.
The repository is private for now, so the release is private too.

## Before you tag

1. Merge the work you want into `main`.
2. Set the new version in these places (`scripts/make-orig` takes the version from `debian/changelog`):
   - `Cargo.toml`: `workspace.package.version` (build once so `Cargo.lock` follows);
   - the `koeinput-dbus-api = { version = "..." }` dependency lines in `crates/cli/Cargo.toml` and `crates/daemon/Cargo.toml`;
   - `addon/CMakeLists.txt`: `project(fcitx5-koeinput VERSION ...)`;
   - the man page date in `crates/cli/src/main.rs` (`.date("YYYY-MM")`);
   - `debian/changelog`: a new entry `X.Y.Z-1` at the top, in the same format as the existing one;
   - `packaging/rpm/koeinput.spec`: `Version:` (keep `Release: 1%{?dist}`) and a `%changelog` entry;
   - `packaging/arch/PKGBUILD`: `pkgver=` (keep `pkgrel=1`).
   The committed PKGBUILD pins the SHA-256 of the sherpa-onnx archive and uses `SKIP` for the main and vendor orig tarballs, because the main tarball contains the PKGBUILD itself and both change with commits and `Cargo.lock`.
   `scripts/build-arch` fills in the real sums on a copy with `updpkgsums` before `makepkg`.
3. Update the version in the file names in `README.md` and `README.ja.md`.
4. If the sherpa-onnx pin changed, follow "sherpa-onnx libraries" in [development.md](development.md) first.
5. Commit the version bump, push, and wait for green CI.
6. Optionally build locally before you tag (each takes a while):

   ```sh
   nice -n 10 scripts/make-orig
   nice -n 10 scripts/build-deb trixie && scripts/test-deb trixie
   nice -n 10 scripts/build-rpm && scripts/test-rpm
   nice -n 10 scripts/build-arch && scripts/test-arch
   ```

   The other Debian and Ubuntu targets use the same scripts with `testing`, `noble` or `resolute` (Ubuntu 26.04).
   Builds run without network from the orig tarballs. They run `scripts/check-no-gpl` before stripping.
   Debian builds also produce `-dbgsym` packages; they hold debug symbols, and the install needs only the two packages per target.

## Tag

Tag only after the version bump is pushed and CI is green.

```sh
git tag -a vX.Y.Z -m "KoeInput X.Y.Z"
git push origin vX.Y.Z
```

The tag must match the version in `Cargo.toml`.

## What CI does

`.github/workflows/release.yml` runs on a `v*` tag and on manual dispatch:

1. `scripts/make-orig` makes the source tarballs.
2. A matrix over the six targets builds the packages and runs the install test in a fresh container of each distribution.
3. On a tag only, it creates a draft release with all packages and a `SHA256SUMS` file.
   A manual dispatch uploads the packages as workflow artifacts only.

## After the build

1. Open the draft release on GitHub and check the package list: two packages per target.
2. Download `SHA256SUMS` and the packages and run `sha256sum -c SHA256SUMS`.
3. Install one package set on a real machine and dictate once.
4. Write the release notes, then publish the draft.

Uploading to distribution archives (Debian, Fedora, AUR) and signing packages are not part of this process.
