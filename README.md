# KoeInput

KoeInput is local Japanese dictation for Linux.
You hold a key, speak, and the text appears in the app you are typing in.
It works through fcitx5, so Mozc stays active and you keep typing as usual.
Speech recognition runs on your computer (sherpa-onnx, CPU only). Audio and text do not leave it.

[日本語版はこちら](README.ja.md)

## What you need

- Linux on amd64 (x86-64) with fcitx5 and a user session bus (systemd user session).
- A microphone.
- About 2 GB of free RAM for the default model, and about 1.2 GB of free disk space to install it (the model takes about 0.7 GB; the archive is saved before it is extracted).

arm64 is not supported yet.

## Install

KoeInput has two packages:

- `koeinput`: the daemon, the `koeinput` command, the systemd user unit, the D-Bus service file and the model catalog.
- `fcitx5-koeinput`: the fcitx5 addon. It needs `koeinput` of the same version.

Packages are built for Debian 13, Debian testing, Ubuntu 24.04, Ubuntu 26.04, Fedora 44 and Arch Linux.
Build them from a source checkout as described in [docs/development.md](docs/development.md#packages); the file names below are what the build produces.

### Debian and Ubuntu

```sh
sudo apt install ./koeinput_0.1.0-1_amd64.deb ./fcitx5-koeinput_0.1.0-1_amd64.deb
```

Ubuntu builds carry the release in the version, for example `koeinput_0.1.0-1~noble1_amd64.deb`.
In the release downloads, the packages for Debian testing end in `_amd64.testing.deb`; the plain names are the Debian 13 packages.

### Fedora

```sh
sudo dnf install ./koeinput-0.1.0-1.fc44.x86_64.rpm ./fcitx5-koeinput-0.1.0-1.fc44.x86_64.rpm
```

### Arch Linux

```sh
sudo pacman -U koeinput-0.1.0-1-x86_64.pkg.tar.zst fcitx5-koeinput-0.1.0-1-x86_64.pkg.tar.zst
```

The daemon is not running after install. D-Bus starts it when fcitx5 loads the addon, or when you run a command such as `koeinput status`.

### Install a speech model

No model is in the packages. Install the default model once per user (about 490 MB download):

```sh
koeinput model install parakeet-ja
```

This also downloads the voice activity detector (Silero VAD).
If you downloaded the archive yourself, add `--from-file <archive>`. This still downloads Silero VAD when it is missing; for a fully offline install, install it first with `koeinput model install silero-vad --from-file silero_vad.onnx`.
If the daemon was already running, run `systemctl --user restart koeinput`; it looks for the model only when it starts.

Then restart fcitx5 so that it loads the addon. In a Plasma session, choose "None" and then "Fcitx 5" again under Virtual Keyboard in System Settings.
Check the result:

```sh
koeinput status
```

It prints `ready: yes` and the model id once the model is loaded (this takes a few seconds).

## Use

Focus any text field and use the hotkey (Right Alt by default):

- Hold it, speak, and release. The text arrives shortly after you release the key.
- Or tap it, speak, and press it again. Use this for long dictation.
- Esc cancels. So does any other key while recording; Shift or Ctrl alone does not. After you finish, another key only clears the display and the text still arrives.
- A recording stops by itself after 2 minutes (`max_length_ms`).
- While Mozc is composing, the hotkey does nothing. Password fields do not start recording.

The status ("Recording") always shows in the fcitx5 panel. The text recognized so far can show in the app or in the panel, as set below.
If Right Alt acts as AltGr on your keyboard layout, choose another hotkey in the settings.

### Settings

Open System Settings, then Input Method, then Configure add-ons, then KoeInput (or run `fcitx5-configtool`).

- Hotkey, tap mode and the tap threshold (how short a press counts as a tap).
- Partial text: "In the app" (the default) shows it underlined in the text field, "In the panel" shows it in the fcitx5 panel, "Off" shows only the status.
- Apps that show partial text in the panel: program names that always use the panel. Use it for apps that commit their own composition when they lose focus, such as Chrome and Discord. To find the name of an app, focus a text field in it and run `fcitx5-diagnose`; in the debug information from D-Bus, the lines with `program:` name the program of each input context.
- Apps that show only the end of partial text, and Characters shown at the end (30 by default): terminals draw partial text on one row and cut it off at the window edge, so a long text hides its end. For the listed programs, a partial text longer than the set length shows as `…` and its last characters. The final text is always entered in full. Find the program name as above; for WezTerm it is `org.wezfurlong.wezterm`.

### Dictionary

Fix terms the recognizer gets wrong. The daemon uses changes at the start of the next session, with no restart:

```sh
koeinput dict add WRONG RIGHT
koeinput dict remove WRONG
koeinput dict list
```

The entries are in `~/.config/koeinput/dictionary.tsv`.

### Models

```sh
koeinput model list                  # catalog, installed models and licenses
koeinput model install ID            # download and install
koeinput model use ID                # make an installed model the default
koeinput model use ID --final        # a second model that decodes the final text
koeinput model use --final none      # turn the final-stage model off
```

After `model use`, with or without `--final`, run `systemctl --user restart koeinput`.

A final-stage model re-decodes the whole utterance after you finish, while the default model keeps producing the partial text.
It can be more accurate but is slower and uses more memory.
It is used only for utterances up to 30 s (`final_full_max_ms`); longer ones use the default model's segments.
[docs/spike-results.md](docs/spike-results.md) compares the models on one speaker's recordings. Treat it as a rough guide, not a benchmark.
Models are listed with their origin and license in [docs/models.md](docs/models.md).

## Privacy

- Recognition runs locally. KoeInput sends no audio or text anywhere.
- The only network use is the model download, from the sherpa-onnx project's GitHub release page (github.com/k2-fsa/sherpa-onnx, release `asr-models`).
- Recognized text is not written to logs. Setting `KOEINPUT_LOG_TEXT=1` for the daemon turns that on; use it for debugging only.

## Troubleshooting

```sh
koeinput status                  # daemon, model and final-stage model state
journalctl --user -u koeinput    # daemon log
systemctl --user restart koeinput
```

- `ready: no`: the model is still loading, is not installed, or the daemon needs a restart after you installed it. `journalctl --user -u koeinput` gives the reason.
- No status or no recording: restart fcitx5, and check that `fcitx5-koeinput` is installed and enabled in the input method settings.
- The addon's messages are in fcitx5's output; with KWin look in `journalctl --user -b | grep -i koeinput`.
- `man koeinput` describes every command.

## License and attributions

KoeInput is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.

Binaries link statically with [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) (Apache-2.0), ONNX Runtime (MIT) and other third-party code under permissive licenses, plus MPL-2.0 for Eigen (file-level copyleft).
No GPL code is in the binaries: builds use the sherpa-onnx archive without text to speech, and `scripts/check-no-gpl` checks every build.
The license texts and copyright notices are installed in `/usr/share/doc/koeinput/copyright` (Debian and Ubuntu) or `/usr/share/licenses/koeinput/` (Fedora and Arch), and summarized in [debian/copyright](debian/copyright).

The speech models are not part of the packages and have their own licenses. The default model is CC-BY-4.0; if you share the model, you need the attribution given in [docs/models.md](docs/models.md). The `koeinput` package installs the same file as `models.md` in `/usr/share/doc/koeinput/`.

## More

- [docs/development.md](docs/development.md): build, test and packaging notes.
- [docs/release.md](docs/release.md): how to cut a release.
- [docs/decisions.md](docs/decisions.md): design decisions.
