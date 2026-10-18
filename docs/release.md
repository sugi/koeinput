# Release steps

A release is a tag `vX.Y.Z`. CI builds the packages for every target and creates a draft GitHub release; you review and publish it.
The repository is private for now, so the release is private too.

## Before you tag

1. Make sure `main` is green in CI and the work you want is merged.
2. Set the new version in all four places:
   - `Cargo.toml`: `workspace.package.version` (then run `cargo update -w` or build once so `Cargo.lock` follows);
   - `debian/changelog`: a new entry `X.Y.Z-1` at the top, in the same format as the existing one;
   - `packaging/rpm/koeinput.spec`: `Version:` (keep `Release: 1%{?dist}`) and a `%changelog` entry;
   - `packaging/arch/PKGBUILD`: `pkgver=` (keep `pkgrel=1`) and the `sha256sums` of the new orig tarballs.
3. Update the version in the file names in `README.md` and `README.ja.md`.
4. If the sherpa-onnx pin changed, follow "sherpa-onnx libraries" in [development.md](development.md) first.
5. Optionally build locally before you tag (each takes a while; run under `nice -n 10`):

   ```sh
   scripts/make-orig
   scripts/build-deb trixie && scripts/test-deb trixie
   scripts/build-rpm && scripts/test-rpm
   scripts/build-arch && scripts/test-arch
   ```

   The other Debian and Ubuntu targets use the same scripts with `testing`, `noble` or the Ubuntu 26.04 code name.
   Builds run without network from the orig tarballs. They run `scripts/check-no-gpl` before stripping.

## Tag

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
