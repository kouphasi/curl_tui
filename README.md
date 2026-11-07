# curl_tui

Rust で作った Postman 風の TUI HTTP クライアントです。

- TUI: [Ratatui](https://ratatui.rs)
- Backend: crossterm
- Async Runtime: Tokio (HTTP は reqwest + rustls)

## 機能

- メソッド切り替え (GET / POST / PUT / PATCH / DELETE / HEAD / OPTIONS)
- URL・クエリパラメータ・ヘッダ・ボディの編集 (`#` で始まる行は無効化)
- 非同期送信 (UI をブロックしない / スピナー表示 / 30 秒タイムアウト)
- レスポンスのステータス・所要時間・サイズ・ヘッダ表示、JSON の自動整形
- リクエストに対応する curl コマンドの表示 (cURL タブ、bash / zsh / sh 向け)
- コレクション管理 (新規 / 複製 / 削除 / 名前変更) と JSON ファイルへの保存
- ブラケットペースト対応、日本語などの全角文字の入力に対応

## インストール

[Releases](https://github.com/kouphasi/curl_tui/releases) から OS に合ったファイルをダウンロードして展開し、
中の `curl_tui` (Windows は `curl_tui.exe`) を PATH の通った場所に置いてください。

| OS | ファイル |
| --- | --- |
| Linux (x86_64) | `curl_tui-<version>-x86_64-unknown-linux-musl.tar.gz` |
| Linux (ARM64) | `curl_tui-<version>-aarch64-unknown-linux-musl.tar.gz` |
| macOS (Apple Silicon) | `curl_tui-<version>-aarch64-apple-darwin.tar.gz` |
| macOS (Intel) | `curl_tui-<version>-x86_64-apple-darwin.tar.gz` |
| Windows (x86_64) | `curl_tui-<version>-x86_64-pc-windows-msvc.zip` |

```sh
tar xzf curl_tui-v0.1.0-x86_64-unknown-linux-musl.tar.gz
sudo mv curl_tui-v0.1.0-x86_64-unknown-linux-musl/curl_tui /usr/local/bin/
```

Linux 版は musl で静的リンクしているので、glibc のバージョンに関係なく動きます。
macOS でダウンロードしたバイナリが「開発元を検証できない」とブロックされる場合は
`xattr -d com.apple.quarantine curl_tui` を実行してください。

ソースからビルドする場合は `cargo install --path .` でインストールできます。

## 使い方

```sh
cargo run --release                   # 既定: ~/.config/curl_tui/collection.json
cargo run --release -- my_api.json    # 保存先ファイルを指定
```

### キー操作

Vim 風の操作感になっています。モードは **NORMAL** / **INSERT** / **COMMAND** の 3 つで、
Normal はブロックカーソル、Insert はバーカーソルで表示されます。

#### 全体

| キー | 動作 |
| --- | --- |
| `Ctrl+H` / `Ctrl+J` / `Ctrl+K` / `Ctrl+L` | ペイン移動 (左 / 下 / 上 / 右) |
| `Ctrl+W` `h`/`j`/`k`/`l` | ペイン移動 (`Ctrl+W w` / `W` で順送り / 逆送り) |
| `Tab` / `Shift+Tab` | ペインを順番に移動 (従来の操作) |
| `Ctrl+R` / `F5` / `:send` | 送信 |
| `:w` / `:q` / `:wq` (`:x`) | 保存 / 終了 / 保存して終了 (`Ctrl+S` でも保存) |
| `m` / `M` | メソッド切り替え |
| `H` / `L`, `gT` / `gt`, `1`-`4` | タブ切り替え |
| `?` | ヘルプ |
| `q` / `Ctrl+C` | 終了 |

ペインは左に Collection、右に上から URL → Request → Response と並んでいます。
Collection から `Ctrl+L` で戻ると、直前にいた右側のペインに戻ります。

#### テキスト欄 (URL / Params / Headers / Body)

| キー | 動作 |
| --- | --- |
| `h` `j` `k` `l`, `w` `b` `e`, `0` `^` `$`, `gg` `G` | カーソル移動 |
| `i` `a` `I` `A` `o` `O` (`Enter` = `A`) | Insert モードへ (`Esc` で Normal に戻る) |
| `x` `X` `s` `S` `D` `C` `r{文字}` | 1 文字削除 / 置換 / 行末まで削除 など |
| `d` `c` `y` + モーション | `dw` `cw` `d$` `dj` `dgg` `dd` `cc` `yy` など |
| `p` / `P`, `Y` | 貼り付け (後 / 前)、行ヤンク |
| `u` | 元に戻す (Insert 1 回分をまとめて戻す) |
| `Ctrl+W` / `Ctrl+H` (Insert 中) | 直前の単語 / 1 文字を削除 |
| `Ctrl+A` / `Ctrl+E` / `Ctrl+K` / `Ctrl+U` (Insert 中) | 行頭 / 行末 / 行末まで削除 / 全消去 |

#### Collection / Response

| キー | 動作 |
| --- | --- |
| `j` / `k`, `gg` / `G` | (Collection) 選択移動 |
| `o` (`n`) / `O` | (Collection) 下 / 上に新規作成 |
| `yy` / `dd` / `p` / `P` | (Collection) ヤンク / 削除 / 貼り付け (`yyp` で複製) |
| `r`, `Enter` / `l` | (Collection) 名前変更、URL ペインへ |
| `j` / `k`, `Ctrl+E` / `Ctrl+Y` | (Response) 1 行スクロール |
| `Ctrl+D` / `Ctrl+U`, `Ctrl+F` / `Ctrl+B` | (Response) 半ページ / 1 ページ送り |
| `gg` / `G` | (Response) 先頭 / 末尾 |

入力途中のキー (`d`, `g`, `^W` など) はヘッダ行の右端に表示されます。`Esc` で取り消せます。

## cURL タブについて

出力は bash / zsh / sh 向けです。値は `'…'` で囲み、中の `'` は `'\''` に置き換えています。
fish・PowerShell・コマンドプロンプトはクォートの規則が違うため、そのまま貼ると正しく動かないことがあります。

## 構成

```
src/
├── main.rs        # Tokio のイベントループ (キー入力 / HTTP 結果 / tick を select!)
├── app.rs         # アプリ状態と Vim 風キー処理 (I/O を持たない状態機械)
├── ui.rs          # Ratatui による描画
├── editor.rs      # 単一行 / 複数行テキストエディタ (単語移動・範囲削除・undo)
├── curl.rs        # curl コマンド生成 (bash / zsh / sh 向け)
├── model.rs       # Request / Response / ヘッダ・パラメータのパース、curl 生成
├── http.rs        # reqwest による送信、JSON 整形
└── collection.rs  # コレクションの保存 / 読み込み
tests/
├── http_integration.rs  # ローカルのモック HTTP サーバに実際に送信する結合テスト
└── curl_shells.rs       # 生成した curl コマンドを実際のシェルで解釈させるテスト
```

## テスト

```sh
cargo test
```

- ユニットテスト: モデル・エディタ・キー操作・保存処理・描画 (`ratatui::backend::TestBackend`)
- 結合テスト: Tokio で立てたモックサーバへの送信、タイムアウト、接続エラー、キー入力から送信までの流れ
- シェルテスト: 生成した curl コマンドを bash / sh / dash / zsh で実行し、curl が受け取る引数が一致するか確認 (未インストールのシェルはスキップ)

## リリース手順

`Cargo.toml` の `version` を更新してコミットし、`v` で始まるタグを push すると
GitHub Actions (`.github/workflows/release.yml`) が各 OS 向けにビルドし、Release にバイナリを添付します。

```sh
git tag v0.1.0
git push origin v0.1.0
```
