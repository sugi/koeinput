# KoeInput

KoeInput is local Japanese dictation for Linux.
You hold a key, speak, and the text appears in the app you are typing in.
It works through fcitx5, so Mozc stays active and you keep typing as usual.
Speech recognition runs on your computer (sherpa-onnx, CPU only). Audio and text do not leave it.

[日本語版はこちら](README.ja.md)

## What you need

- Linux on amd64 (x86-64) with fcitx5 and a user session bus (systemd user session).
- A microphone.
- About 2 GB of free RAM for the default model, and about 0.5 GB of disk space for it.

arm64 is not supported yet.

## Install

KoeInput has two packages:

- `koeinput`: the daemon, the `koeinput` command, the systemd user unit, the D-Bus service file and the model catalog.
- `fcitx5-koeinput`: the fcitx5 addon. It needs `koeinput` of the same version.

Packages are built for Debian 13, Debian testing, Ubuntu 24.04, Ubuntu 26.04, Fedora 44 and Arch Linux.
Build them from a source checkout as described in [docs/release.md](docs/release.md); the file names below are what the build produces.

### Debian and Ubuntu

```sh
sudo apt install ./koeinput_0.1.0-1_amd64.deb ./fcitx5-koeinput_0.1.0-1_amd64.deb
```

Ubuntu builds carry the release in the version, for example `koeinput_0.1.0-1~noble1_amd64.deb`.

### Fedora

```sh
sudo dnf install ./koeinput-0.1.0-1.fc44.x86_64.rpm ./fcitx5-koeinput-0.1.0-1.fc44.x86_64.rpm
```

### Arch Linux

```sh
sudo pacman -U koeinput-0.1.0-1-x86_64.pkg.tar.zst fcitx5-koeinput-0.1.0-1-x86_64.pkg.tar.zst
```

The daemon is not enabled at install. D-Bus starts it when fcitx5 loads the addon or when you run `koeinput`.

### Install a speech model

No model is in the packages. Install the default model once per user (about 490 MB download):

```sh
koeinput model install parakeet-ja
```

This also downloads the voice activity detector (Silero VAD).
If you downloaded the archive yourself, add `--from-file <archive>`.
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
- Esc cancels. So does any other key you press during a session.
- While Mozc is composing, the hotkey does nothing. Password fields do not start recording.

While you speak, the panel or the app shows the status ("Recording") and the text recognized so far.
If Right Alt acts as AltGr on your keyboard layout, choose another hotkey in the settings.

### Settings

Open System Settings, then Input Method, then Configure add-ons, then KoeInput (or run `fcitx5-configtool`).

- Hotkey, tap mode and the tap threshold (how short a press counts as a tap).
- Partial text: "In the app" (the default) shows it underlined in the text field, "In the panel" shows it in the fcitx5 panel, "Off" shows only the status.
- Apps that show partial text in the panel: program names that always use the panel. Use it for apps that commit their own composition when they lose focus, such as Chrome and Discord. To find the name of an app, focus a text field in it and run `fcitx5-diagnose`; the "Input Context" list names the program of each context.

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

A final-stage model re-decodes the whole utterance after you finish, while the default model keeps producing the partial text.
It can be more accurate but is slower and uses more memory.
[docs/spike-results.md](docs/spike-results.md) compares the models on one speaker's recordings. Treat it as a rough guide, not a benchmark.
Models are listed with their origin and license in [docs/models.md](docs/models.md).

## Privacy

- Recognition runs locally. KoeInput sends no audio or text anywhere.
- The network is used only when you install a model, which downloads it from its publisher's release page.
- Recognized text is not written to logs.

## Troubleshooting

```sh
koeinput status                  # daemon, model and final-stage model state
journalctl --user -u koeinput    # daemon log
systemctl --user restart koeinput
```

- `ready: no` with an install hint: the model is not installed yet, or the daemon needs a restart after you installed it.
- No status text or no recording: restart fcitx5, and check that `fcitx5-koeinput` is installed and enabled in the input method settings.
- The addon's messages are in fcitx5's output; with KWin look in `journalctl --user -b | grep -i koeinput`.
- `man koeinput` describes every command.

## License and attributions

KoeInput is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.

Binaries link statically with [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) (Apache-2.0), ONNX Runtime (MIT) and other third-party code under permissive licenses.
No GPL code is in the binaries: builds use the sherpa-onnx archive without text to speech, and `scripts/check-no-gpl` checks every build.
The license texts and copyright notices are installed in `/usr/share/doc/koeinput/` and summarized in [debian/copyright](debian/copyright).

The speech models are not part of the packages and have their own licenses. The default model is CC-BY-4.0 and needs the attribution given in [docs/models.md](docs/models.md).

## More

- [docs/development.md](docs/development.md): build, test and packaging notes.
- [docs/release.md](docs/release.md): how to cut a release.
- [docs/decisions.md](docs/decisions.md): design decisions.
