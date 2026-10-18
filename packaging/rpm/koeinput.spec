# The License of koeinput: the binaries link the vendored crates and the static
# libraries of the pinned sherpa-onnx archive (Apache-2.0 AND MIT AND
# BSD-2-Clause AND BSD-3-Clause AND BSL-1.0 AND MPL-2.0 AND Zlib; details in
# debian/copyright). %%build fails when cargo tree shows a crate license that
# is not listed here.
%global koeinput_license (MIT OR Apache-2.0) AND Apache-2.0 AND MIT AND BSD-2-Clause AND BSD-3-Clause AND BSL-1.0 AND CDLA-Permissive-2.0 AND ISC AND MPL-2.0 AND Unicode-3.0 AND Zlib AND bzip2-1.0.6 AND (0BSD OR MIT OR Apache-2.0) AND (Apache-2.0 OR ISC OR MIT) AND (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT) AND (MIT OR Zlib OR Apache-2.0) AND (Unlicense OR MIT)

# The components of the pinned sherpa-onnx archive; %%prep checks them against
# packaging/sherpa-onnx.env and packaging/licenses/.
%global sherpa_onnx_version 1.13.8
%global onnxruntime_version 1.28.2

%global _vpath_srcdir addon
# The addon is a plugin that fcitx5 loads, not a library to link against.
%global __provides_exclude_from ^%{_libdir}/fcitx5/.*\\.so$
# CARGO_TARGET_DIR and SHERPA_ONNX_LIB_DIR must be absolute: the sherpa-onnx-sys
# build script reads them as they are. The remap keeps the build directory out of
# the panic messages; it is the directory that find-debuginfo puts the sources in.
%global cargo_env export CARGO_HOME="$PWD/.cargo-home" CARGO_TARGET_DIR="$PWD/target" SHERPA_ONNX_LIB_DIR="$PWD/sherpa-lib" RUSTFLAGS="$RUSTFLAGS --remap-path-prefix=$PWD=/usr/src/debug/%{name}-%{version}-%{release}.%{_arch}"
%global cargo_flags --release --frozen --offline --workspace -j%{_smp_build_ncpus}

Name:           koeinput
Version:        0.1.0
Release:        1%{?dist}
Summary:        Local Japanese dictation daemon and command-line tool
License:        %{koeinput_license}
URL:            https://github.com/sugi/koeinput
# The orig tarballs of the Debian source package, made by scripts/make-orig:
# git archive without debian/, the crates of Cargo.lock, and the pinned
# no-TTS sherpa-onnx archive byte for byte (packaging/sherpa-onnx.env).
Source0:        %{url}/releases/download/v%{version}/koeinput_%{version}.orig.tar.xz
Source1:        %{url}/releases/download/v%{version}/koeinput_%{version}.orig-vendor.tar.xz
Source2:        %{url}/releases/download/v%{version}/koeinput_%{version}.orig-sherpaonnx.tar.bz2
# debian/copyright: the copyright notices and license texts of the crates and
# the sherpa-onnx libraries, which MIT and BSD ask to keep with the binaries.
Source3:        koeinput-copyright

# sherpa-onnx publishes the no-TTS static libraries for x86_64 only.
ExclusiveArch:  x86_64

BuildRequires:  cargo >= 1.89
BuildRequires:  rust >= 1.89
BuildRequires:  gcc
BuildRequires:  gcc-c++
BuildRequires:  cmake >= 3.25
BuildRequires:  extra-cmake-modules
BuildRequires:  gettext
BuildRequires:  pkgconfig
BuildRequires:  pkgconfig(libpulse-simple)
BuildRequires:  cmake(Fcitx5Core)
BuildRequires:  cmake(Fcitx5Config)
BuildRequires:  cmake(Fcitx5Utils)
BuildRequires:  cmake(Fcitx5Module)
BuildRequires:  binutils
BuildRequires:  systemd-rpm-macros

Requires:       dbus
Requires:       dbus-common
Suggests:       fcitx5-koeinput
Provides:       bundled(sherpa-onnx) = %{sherpa_onnx_version}
Provides:       bundled(onnxruntime) = %{onnxruntime_version}

%description
KoeInput turns Japanese speech from the microphone into text. Recognition
runs locally on the CPU, so the audio never leaves the computer.

This package contains the daemon, which D-Bus starts when it is first
needed, and the koeinput command, which shows the daemon's status,
transcribes WAV files and manages recognition models.

No model is included. "koeinput model list" shows the models, and
"koeinput model install" downloads one.

%package -n fcitx5-koeinput
Summary:        KoeInput dictation addon for fcitx5
License:        MIT OR Apache-2.0
Requires:       %{name}%{?_isa} = %{version}-%{release}
Requires:       fcitx5

%description -n fcitx5-koeinput
KoeInput turns Japanese speech from the microphone into text. Recognition
runs locally on the CPU, so the audio never leaves the computer.

This fcitx5 addon adds push-to-talk dictation: hold the hotkey and speak,
and the recognized text goes into the focused input field. The current
input method, such as Mozc, stays active.

Models are downloaded separately with "koeinput model install".

%prep
%setup -q
# The pin is in the main tarball; the archive must be the pinned one.
. packaging/sherpa-onnx.env
echo "$SHERPA_ONNX_SHA256  %{SOURCE2}" | sha256sum --check --strict
case $SHERPA_ONNX_FILE in
  sherpa-onnx-v%{sherpa_onnx_version}-*) ;;
  *)
    echo "packaging/sherpa-onnx.env pins $SHERPA_ONNX_FILE, not sherpa-onnx %{sherpa_onnx_version}; update sherpa_onnx_version" >&2
    exit 1 ;;
esac
set -- packaging/licenses/onnxruntime-*-LICENSE
if [ "$*" != packaging/licenses/onnxruntime-%{onnxruntime_version}-LICENSE ]; then
  echo "packaging/licenses/ has $*, not only the ONNX Runtime %{onnxruntime_version} license; update onnxruntime_version" >&2
  exit 1
fi
mkdir vendor sherpaonnx
tar -xf %{SOURCE1} -C vendor --strip-components=1
tar -xf %{SOURCE2} -C sherpaonnx --strip-components=1
cp -p %{SOURCE3} copyright

# sherpaonnx/ is the no-TTS archive. The sherpa-onnx-sys build script still links
# espeak-ng, piper_phonemize and ucd (the GPL text-to-speech code), so they get
# empty archives here, as in scripts/sherpa-onnx.
mkdir sherpa-lib
ln -s "$PWD"/sherpaonnx/lib/*.a sherpa-lib/
for n in espeak-ng piper_phonemize ucd; do
  if [ -e sherpaonnx/lib/lib$n.a ]; then
    echo "sherpaonnx/lib/lib$n.a exists: not the no-TTS archive" >&2
    exit 1
  fi
  ar rcs sherpa-lib/lib$n.a
done

mkdir .cargo-home
cat >.cargo-home/config.toml <<EOF
[source.crates-io]
replace-with = "vendored-sources"
[source.vendored-sources]
directory = "$PWD/vendor"
[net]
offline = true
EOF

%build
%{cargo_env}
# Every license of a linked crate has to be in the License tag. cargo tree runs
# on its own so that a failure stops the build (rpm scripts have no pipefail).
cargo tree --frozen --offline --workspace -e no-build,no-dev,no-proc-macro \
  --target x86_64-unknown-linux-gnu --no-dedupe --prefix none -f '{p}|{l}' >crate-tree
grep -v '^$' crate-tree | sed 's/ (\*)$//' | sort -u >crate-list
if grep '|$' crate-list >&2; then
  echo "these linked crates have no license expression (license-file only?); check them and extend the License tag by hand" >&2
  exit 1
fi
sed 's/^[^|]*|//' crate-list | tr ' /()' '\n\n\n\n' | grep -vx -e '' -e OR -e AND -e WITH | sort -u >crate-licenses
[ -s crate-licenses ] && grep -qx MIT crate-licenses || {
  echo "cargo tree listed no crate licenses, or none with MIT; the License check did not work" >&2
  exit 1
}
while read -r id; do
  case " %{koeinput_license} " in
    *[\ \(]"$id"[\ \)]*) ;;
    *) echo "a linked crate has the license $id, which the License tag does not list" >&2; exit 1 ;;
  esac
done <crate-licenses
echo "License check: $(wc -l <crate-list) linked crates, licenses $(tr '\n' ' ' <crate-licenses)"

cargo build %{cargo_flags} --bins
scripts/check-no-gpl target/release/koeinput target/release/koeinput-daemon
target/release/koeinput manpage >koeinput.1

%cmake
%cmake_build

%install
%cmake_install
install -D -m 0755 target/release/koeinput %{buildroot}%{_bindir}/koeinput
install -D -m 0755 target/release/koeinput-daemon %{buildroot}%{_libexecdir}/koeinput/koeinput-daemon
install -D -m 0644 data/koeinput.service %{buildroot}%{_userunitdir}/koeinput.service
install -D -m 0644 data/org.nemui.KoeInput.service %{buildroot}%{_datadir}/dbus-1/services/org.nemui.KoeInput.service
install -D -m 0644 data/models.toml %{buildroot}%{_datadir}/koeinput/models.toml
install -D -m 0644 koeinput.1 %{buildroot}%{_mandir}/man1/koeinput.1
%find_lang fcitx5-koeinput

%check
# The addon's D-Bus and fcitx5 tests and the daemon's D-Bus tests need a session
# bus and a test daemon; they run in the development test runs only.
%{cargo_env}
%ctest -R controller
cargo test %{cargo_flags}

%files
# No %%systemd_user_post: the unit has no [Install] section, and D-Bus starts it.
%license LICENSE-MIT LICENSE-APACHE copyright packaging/licenses/*
%{_bindir}/koeinput
%dir %{_libexecdir}/koeinput
%{_libexecdir}/koeinput/koeinput-daemon
%{_userunitdir}/koeinput.service
%{_datadir}/dbus-1/services/org.nemui.KoeInput.service
%dir %{_datadir}/koeinput
%{_datadir}/koeinput/models.toml
%{_mandir}/man1/koeinput.1*

%files -n fcitx5-koeinput -f fcitx5-koeinput.lang
%license LICENSE-MIT LICENSE-APACHE
%{_libdir}/fcitx5/libkoeinput.so
%{_datadir}/fcitx5/addon/koeinput.conf

%changelog
* Sun Oct 18 2026 Tatsuki Sugiura <sugi@nemui.org> - 0.1.0-1
- Initial package.
