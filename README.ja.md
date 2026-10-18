# KoeInput

KoeInput は、Linux 向けのローカル日本語音声入力です。
キーを押しながら話すと、入力中のアプリに文字が入ります。
fcitx5 のアドオンとして動くので、Mozc はそのまま使えます。
音声認識（sherpa-onnx、CPU のみ）はすべて手元のパソコンで行い、音声も文字も外に送りません。

[English version](README.md)

## 必要なもの

- amd64 (x86-64) の Linux、fcitx5、ユーザーセッションバス（systemd のユーザーセッション）。
- マイク。
- 標準モデルのための空きメモリ約 2 GB と、ディスク約 0.5 GB。

arm64 には対応していません。

## インストール

パッケージは 2 つあります。

- `koeinput`: デーモン、`koeinput` コマンド、systemd ユーザーユニット、D-Bus サービスファイル、モデルカタログ。
- `fcitx5-koeinput`: fcitx5 アドオン。同じバージョンの `koeinput` が必要です。

Debian 13、Debian testing、Ubuntu 24.04、Ubuntu 26.04、Fedora 44、Arch Linux 向けにビルドします。
ビルド方法は [docs/release.md](docs/release.md) にあります（英語）。下のファイル名はビルドで作られるものです。

### Debian / Ubuntu

```sh
sudo apt install ./koeinput_0.1.0-1_amd64.deb ./fcitx5-koeinput_0.1.0-1_amd64.deb
```

Ubuntu 向けは、バージョンにリリース名が付きます（例: `koeinput_0.1.0-1~noble1_amd64.deb`）。

### Fedora

```sh
sudo dnf install ./koeinput-0.1.0-1.fc44.x86_64.rpm ./fcitx5-koeinput-0.1.0-1.fc44.x86_64.rpm
```

### Arch Linux

```sh
sudo pacman -U koeinput-0.1.0-1-x86_64.pkg.tar.zst fcitx5-koeinput-0.1.0-1-x86_64.pkg.tar.zst
```

インストールしてもデーモンは自動起動しません。fcitx5 がアドオンを読み込んだとき、または `koeinput` コマンドを実行したときに、D-Bus が起動します。

### 音声認識モデルのインストール

パッケージにモデルは入っていません。ユーザーごとに 1 回、標準モデル（約 490 MB）をインストールします。

```sh
koeinput model install parakeet-ja
```

音声区間検出（Silero VAD）も一緒にダウンロードします。
ダウンロード済みのアーカイブを使うときは `--from-file <アーカイブ>` を付けます。
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
- Esc で取り消します。録音中に他のキーを押しても取り消されます。
- Mozc で変換中のときは、ホットキーは何もしません。パスワード欄では録音しません。

話している間は、パネルまたはアプリに「録音中」と、認識できたところまでの文字が出ます。
キーボード配列で右 Alt が AltGr になる場合は、設定で別のホットキーを選んでください。

### 設定

システム設定の「入力メソッド」から「アドオンの設定」を開き、KoeInput を選びます（`fcitx5-configtool` でも開けます）。

- ホットキー、短押しで録音を続けるか、短押しとみなす時間。
- 話している途中の文字: 「アプリの中」（標準）は文字欄に下線付きで出します。「パネル」は fcitx5 のパネルに出します。「表示しない」は状態だけを出します。
- 途中の文字をパネルに出すアプリ: 必ずパネルを使うアプリのプログラム名です。フォーカスを外すと入力中の文字を確定してしまうアプリ（Chrome や Discord など）に使います。アプリの名前は、そのアプリの文字欄にフォーカスして `fcitx5-diagnose` を実行し、「Input Context」の一覧で確認できます。

### 辞書

認識を間違えやすい語を直せます。変更は次の録音から反映され、再起動は要りません。

```sh
koeinput dict add 間違い 正しい形
koeinput dict remove 間違い
koeinput dict list
```

内容は `~/.config/koeinput/dictionary.tsv` に保存されます。

### モデル

```sh
koeinput model list                  # カタログ、インストール済みモデル、ライセンス
koeinput model install ID            # ダウンロードしてインストール
koeinput model use ID                # インストール済みモデルを標準にする
koeinput model use ID --final        # 確定テキストだけを処理する第 2 のモデル
koeinput model use --final none      # 第 2 のモデルをやめる
```

第 2 のモデル（最終段モデル）は、話し終えたあとに発話全体をもう一度認識します。途中の文字は標準モデルのままです。
精度が上がることがありますが、遅くなり、メモリも多く使います。
モデルの比較は [docs/spike-results.md](docs/spike-results.md)（英語）にあります。1 人の録音での結果なので、目安として見てください。
各モデルの出どころとライセンスは [docs/models.md](docs/models.md)（英語）にあります。

## プライバシー

- 認識はすべてローカルで行います。音声も文字も外に送りません。
- ネットワークを使うのは、モデルをインストールするときだけです。配布元からダウンロードします。
- 認識した文字はログに書きません。

## 困ったとき

```sh
koeinput status                  # デーモン、モデル、最終段モデルの状態
journalctl --user -u koeinput    # デーモンのログ
systemctl --user restart koeinput
```

- `ready: no` とインストール方法が出る: モデルが未インストールか、インストール後にデーモンを再起動していません。
- 状態表示も録音も出ない: fcitx5 を再起動し、`fcitx5-koeinput` がインストールされ、入力メソッドの設定で有効になっているか確認してください。
- アドオンのメッセージは fcitx5 の出力にあります。KWin が fcitx5 を起動している場合は `journalctl --user -b | grep -i koeinput` で探せます。
- `man koeinput` に全コマンドの説明があります。

## ライセンスと帰属表示

KoeInput は [MIT](LICENSE-MIT) または [Apache-2.0](LICENSE-APACHE) のどちらかを選べるデュアルライセンスです。

実行ファイルは [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx)（Apache-2.0）、ONNX Runtime（MIT）などの、寛容なライセンスのコードを静的にリンクしています。
GPL のコードは含みません。音声合成なしの sherpa-onnx アーカイブでビルドし、`scripts/check-no-gpl` が毎回確認します。
ライセンス文と著作権表示は `/usr/share/doc/koeinput/` に入り、[debian/copyright](debian/copyright) にまとめてあります。

音声認識モデルはパッケージに含まれず、それぞれ別のライセンスです。標準モデルは CC-BY-4.0 で、[docs/models.md](docs/models.md) の帰属表示が必要です。

## 関連ドキュメント

- [docs/development.md](docs/development.md): ビルド、テスト、パッケージング（英語）。
- [docs/release.md](docs/release.md): リリース手順（英語）。
- [docs/decisions.md](docs/decisions.md): 設計の判断（英語）。
