# KoeInput

KoeInput は、Linux 向けのローカル日本語音声入力です。
キーを押しながら話すと、入力中のアプリに文字が入ります。
fcitx5 のアドオンとして動くので、Mozc はそのまま使えます。
音声認識（sherpa-onnx、CPU のみ）はすべて手元のパソコンで行い、音声も文字も外に送りません。

[English version](README.md)

## 必要なもの

- amd64 (x86-64) の Linux、fcitx5、ユーザーセッションバス（systemd のユーザーセッション）。
- マイク。
- 標準モデルのための空きメモリ約 2 GB と、ディスクの空き約 1.2 GB（モデルは約 0.7 GB ですが、アーカイブを保存してから展開します）。

arm64 には対応していません。

## インストール

パッケージは 2 つあります。

- `koeinput`: デーモン、`koeinput` コマンド、systemd ユーザーユニット、D-Bus サービスファイル、モデルカタログ。
- `fcitx5-koeinput`: fcitx5 アドオン。同じバージョンの `koeinput` が必要です。

Debian 13、Debian testing、Ubuntu 24.04、Ubuntu 26.04、Fedora 44、Arch Linux 向けにビルドします。
ビルド方法は [docs/development.md](docs/development.md#packages) にあります（英語）。下のファイル名はビルドで作られるものです。

### Debian / Ubuntu

```sh
sudo apt install ./koeinput_0.1.0-1_amd64.deb ./fcitx5-koeinput_0.1.0-1_amd64.deb
```

Ubuntu 向けは、バージョンにリリース名が付きます（例: `koeinput_0.1.0-1~noble1_amd64.deb`）。
リリースのダウンロードでは、Debian testing 向けのパッケージ名は `_amd64.testing.deb` で終わります。この印のない名前は Debian 13 向けです。

### Fedora

```sh
sudo dnf install ./koeinput-0.1.0-1.fc44.x86_64.rpm ./fcitx5-koeinput-0.1.0-1.fc44.x86_64.rpm
```

### Arch Linux

```sh
sudo pacman -U koeinput-0.1.0-1-x86_64.pkg.tar.zst fcitx5-koeinput-0.1.0-1-x86_64.pkg.tar.zst
```

インストールしただけでは常駐しません。fcitx5 がアドオンを読み込んだとき（または `koeinput status` などを実行したとき）に D-Bus がデーモンを起動します。

### 音声認識モデルのインストール

パッケージにモデルは入っていません。ユーザーごとに 1 回、標準モデル（約 490 MB）をインストールします。

```sh
koeinput model install parakeet-ja
```

音声区間検出（Silero VAD）も一緒にダウンロードします。
ダウンロード済みのアーカイブを使うときは `--from-file <アーカイブ>` を付けます。この場合も、Silero VAD がなければダウンロードします。完全にオフラインで入れるときは、先に `koeinput model install silero-vad --from-file silero_vad.onnx` を実行してください。
デーモンが動いているときは `systemctl --user restart koeinput` を実行してください。デーモンはモデルを起動時にしか探しません。

次に fcitx5 を再起動して、アドオンを読み込ませます。Plasma では、システム設定の「仮想キーボード」で一度「なし」にしてから「Fcitx 5」を選び直します。
確認します。

```sh
koeinput status
```

モデルの読み込みが終わると（数秒かかります）、`ready: yes` とモデルの ID が出ます。

## 使い方

文字を入力できる場所にフォーカスして、ホットキー（標準は右 Alt）を使います。

- 押したまま話して、離します。キーを離すとすぐに確定します。
- 短く押して話し、もう一度押します。長い文章向けです。
- Esc で取り消します。録音中に他のキーを押しても取り消されます（Shift や Ctrl だけなら取り消されません）。話し終えたあとに他のキーを押すと、表示が消えるだけで、文字は入ります。
- 録音は 2 分で自動的に止まります（`max_length_ms`）。
- Mozc で変換中のときは、ホットキーは何もしません。パスワード欄では録音しません。

「録音中」の表示は、いつも fcitx5 のパネルに出ます。認識できたところまでの文字は、下の設定に従って、アプリまたはパネルに出ます。
キーボード配列で右 Alt が AltGr になる場合は、設定で別のホットキーを選んでください。

### 設定

システム設定の「入力メソッド」から「アドオンの設定」を開き、KoeInput を選びます（`fcitx5-configtool` でも開けます）。

- ホットキー、「短く押したら録音を続ける」、「短押しとみなす時間（ミリ秒）」。
- 話している途中の文字: 「アプリの中」（標準）は文字欄に下線付きで出します。「パネル」は fcitx5 のパネルに出します。「表示しない」は状態だけを出します。
- 途中の文字をパネルに出すアプリ: 必ずパネルを使うアプリのプログラム名です。フォーカスを外すと入力中の文字を確定してしまうアプリ（Chrome や Discord など）に使います。アプリの名前は、そのアプリの文字欄にフォーカスして `fcitx5-diagnose` を実行し、D-Bus から取得したデバッグ情報の `program:` の行で確認できます。

### 辞書

認識を間違えやすい語を直せます。変更は次の録音から反映され、再起動は要りません。

```sh
koeinput dict add 誤認識される形 正しい形
koeinput dict remove 誤認識される形
koeinput dict list
```

内容は `~/.config/koeinput/dictionary.tsv` に保存されます。

### モデル

```sh
koeinput model list                  # カタログ、インストール済みモデル、ライセンス
koeinput model install ID            # ダウンロードしてインストール
koeinput model use ID                # インストール済みモデルを標準にする
koeinput model use ID --final        # 確定テキストを仕上げる第 2 のモデル（仕上げ用モデル）
koeinput model use --final none      # 仕上げ用モデルを使うのをやめる
```

`model use` は、`--final` の有無にかかわらず、実行後に `systemctl --user restart koeinput` が必要です。

仕上げ用モデルは、話し終えたあとに発話全体をもう一度認識します。途中の文字は標準モデルのままです。
精度が上がることがありますが、遅くなり、メモリも多く使います。
使われるのは 30 秒までの発話だけです（`final_full_max_ms`）。それより長い発話は、標準モデルの区間ごとの結果になります。
モデルの比較は [docs/spike-results.md](docs/spike-results.md)（英語）にあります。1 人の録音での結果なので、目安として見てください。
各モデルの出どころとライセンスは [docs/models.md](docs/models.md)（英語）にあります。

## プライバシー

- 認識はすべてローカルで行います。音声も文字も外に送りません。
- ネットワークを使うのは、モデルのダウンロードだけです。sherpa-onnx プロジェクトの GitHub リリースページ（github.com/k2-fsa/sherpa-onnx、リリース `asr-models`）から取得します。
- 認識した文字はログに書きません。デーモンに `KOEINPUT_LOG_TEXT=1` を設定すると書くようになります。デバッグ専用です。

## 困ったとき

```sh
koeinput status                  # デーモン、モデル、仕上げ用モデルの状態
journalctl --user -u koeinput    # デーモンのログ
systemctl --user restart koeinput
```

- `ready: no`: モデルを読み込み中か、未インストールか、インストール後にデーモンを再起動していません。理由は `journalctl --user -u koeinput` で分かります。
- 状態表示が出ない、または録音も始まらない: fcitx5 を再起動し、`fcitx5-koeinput` がインストールされ、入力メソッドの設定で有効になっているか確認してください。
- アドオンのメッセージは fcitx5 の出力にあります。KWin が fcitx5 を起動している場合は `journalctl --user -b | grep -i koeinput` で探せます。
- `man koeinput` に全コマンドの説明があります。

## ライセンスと帰属表示

KoeInput は [MIT](LICENSE-MIT) または [Apache-2.0](LICENSE-APACHE) のどちらかを選べるデュアルライセンスです。

実行ファイルは [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx)（Apache-2.0）、ONNX Runtime（MIT）などの、寛容なライセンスのコードを静的にリンクしています。Eigen だけは MPL-2.0（ファイル単位のコピーレフト）です。
GPL のコードは含みません。音声合成なしの sherpa-onnx アーカイブでビルドし、`scripts/check-no-gpl` が毎回確認します。
ライセンス文と著作権表示は、Debian / Ubuntu では `/usr/share/doc/koeinput/copyright`、Fedora / Arch では `/usr/share/licenses/koeinput/` に入ります。[debian/copyright](debian/copyright) にまとめてもあります。

音声認識モデルはパッケージに含まれず、それぞれ別のライセンスです。標準モデルは CC-BY-4.0 で、モデルを配布するときは [docs/models.md](docs/models.md) の帰属表示が必要です。`koeinput` パッケージは同じファイルを `/usr/share/doc/koeinput/models.md` に入れます。

## 関連ドキュメント

- [docs/development.md](docs/development.md): ビルド、テスト、パッケージング（英語）。
- [docs/release.md](docs/release.md): リリース手順（英語）。
- [docs/decisions.md](docs/decisions.md): 設計の判断（英語）。
