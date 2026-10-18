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
   nice -n 10 scripts/build-rpm fedora44 && scripts/test-rpm fedora44
   nice -n 10 scripts/build-arch && scripts/test-arch
   ```

   The other Debian and Ubuntu targets use the same scripts with `testing`, `noble` or `resolute` (Ubuntu 26.04).
   Builds run without network from the orig tarballs. Each build runs `scripts/check-no-gpl` on the files that go into the package, before they are stripped.
   Each build also makes debug packages: `-dbgsym` on Debian, `.ddeb` on Ubuntu, `-debuginfo` and `-debugsource` on Fedora, and `-debug` on Arch. They are for local debugging; a release uploads only the two user packages per target.
   [Packages](development.md#packages) in the development notes describes the scripts and their checks.

## Tag

Tag only after the version bump is pushed and CI is green.

```sh
git tag -a vX.Y.Z -m "KoeInput X.Y.Z"
git push origin vX.Y.Z
```

The tag must match the upstream version in `debian/changelog`; `release.yml` checks that. The install test checks that `Cargo.toml` has the same version.

## What CI does

`.github/workflows/release.yml` runs on a `v*` tag and on manual dispatch. All jobs run the repository's scripts on `ubuntu-24.04` runners with rootless podman.

1. **Orig tarballs**: `scripts/make-orig` makes the three orig tarballs from a full clone and uploads them as the workflow artifact `orig`.
   On a tag, the job stops when the tag is not `v` followed by the upstream version in `debian/changelog`.
   The RPM spec's `Source0` to `Source2` are release download URLs under that tag, with these file names.
2. **Packages**: a matrix over the six targets (`trixie`, `testing`, `noble`, `resolute`, `fedora44`, `arch`). Each job:
   - downloads the orig tarballs into `target/deb/`. They match HEAD, so the build scripts use them instead of making new ones, and every target builds from the same tarballs;
   - runs the build script and the install test of its format: `scripts/build-deb <dist>` and `scripts/test-deb <dist>`, `scripts/build-rpm fedora44` and `scripts/test-rpm fedora44`, or `scripts/build-arch` and `scripts/test-arch`;
   - fails when the install test passed with warnings. On your machine, a test where fcitx5 cannot run without a display passes with a warning and leaves the addon load unchecked; a release needs that check;
   - uploads the two packages that users install as the artifact `packages-<target>`. The debug symbol packages (`-dbgsym`, `.ddeb`, `-debuginfo`, `-debugsource`, `-debug`) stay out.

   A failed target does not stop the others.
3. **Draft release**: on a tag push only, after every target has passed. The job:
   - collects the 12 packages and the three orig tarballs;
   - writes `SHA256SUMS` over them;
   - creates a draft release for the tag with `gh release create --draft`. It never publishes the release.

   The Debian testing packages have the same file names as the Debian 13 ones, and the files of a release need different names. So the release names the testing packages `koeinput_X.Y.Z-1_amd64.testing.deb` and `fcitx5-koeinput_X.Y.Z-1_amd64.testing.deb`. Only the file name changes; the package version stays `X.Y.Z-1`.

   If a release for the tag already exists, the job stops. Delete the draft on GitHub, then run the job again.

A manual dispatch, from a branch or a tag, uploads the packages as workflow artifacts only. GitHub starts a manual run only when `release.yml` is on `main`; the run then uses the workflow file of the branch you name:

```sh
gh workflow run release.yml --ref main
gh run list --workflow release.yml    # wait for the run to finish
gh run download <run-id>
```

The artifacts are kept for 7 days, because they count against the storage quota of the private repository.

Only the draft release job can write to the repository (`contents: write`); the other jobs can only read it.

## After the build

1. Open the draft release on GitHub and check the file list: two packages for each of the six targets (no debug packages), the three orig tarballs and `SHA256SUMS`.
2. Download `SHA256SUMS` and the packages and run `sha256sum -c SHA256SUMS`.
3. Install one package set on a real machine and dictate once.
4. Write the release notes, then publish the draft.

Uploading to distribution archives (Debian, Fedora, AUR) and signing packages are not part of this process.
