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
- HTML レスポンスのプレビュー (Preview タブ。ヘッドレス Chromium で描画し、kitty graphics protocol で端末に表示)
- リクエストに対応する curl コマンドの表示 (cURL タブ、bash / zsh / sh 向け)
- コレクション管理 (新規 / 複製 / 削除 / 名前変更) と JSON ファイルへの保存
- ブラケットペースト対応、日本語などの全角文字の入力に対応

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
| `1` / `2` / `3`, `H` / `L` | (Response) Body / Headers / Preview タブ (Preview は HTML のときだけ) |

入力途中のキー (`d`, `g`, `^W` など) はヘッダ行の右端に表示されます。`Esc` で取り消せます。

## HTML プレビュー (Preview タブ)

レスポンスの `Content-Type` が `text/html` (または `application/xhtml+xml`) のときだけ、
Response に **Preview** タブが出ます。JSON などそれ以外のレスポンスでは出ません。

Preview タブを開くと、ヘッドレス Chromium で HTML を Response ペインの幅に合わせて描画し、
その画像を [kitty graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/) で
ペイン内に表示します。`j` / `k` / `Ctrl+D` などでスクロールできます。

- 静止画のプレビューです。リンクのクリックやフォーム入力などの操作はできません
- `<base href>` を差し込むので、相対パスの画像や CSS はレスポンスの URL 基準で読み込まれます
- 撮影する高さはペインの 4 画面分 (最大 8000px) までです。それより長いページは切れます
- 端末の幅を変えると描き直します

必要なもの:

- kitty graphics protocol 対応端末 (kitty / Ghostty / WezTerm など)。tmux 内では使えません
- Chromium 系ブラウザ (`chrome-headless-shell` / `chromium` / `google-chrome` / Microsoft Edge / Brave を自動で探します)

| 環境変数 | 意味 |
| --- | --- |
| `CURL_TUI_BROWSER` | 使うブラウザの実行ファイル (例: `/usr/bin/chromium`) |
| `CURL_TUI_GRAPHICS` | `kitty` で端末判定を飛ばして強制有効化、`none` で無効化 |

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
├── preview.rs     # HTML をヘッドレス Chromium で PNG に描画
├── kitty.rs       # kitty graphics protocol のエスケープシーケンス生成・対応端末の判定
└── collection.rs  # コレクションの保存 / 読み込み
tests/
├── http_integration.rs  # ローカルのモック HTTP サーバに実際に送信する結合テスト
├── curl_shells.rs       # 生成した curl コマンドを実際のシェルで解釈させるテスト
└── preview_render.rs    # 実際のブラウザで HTML を描画するテスト (CURL_TUI_BROWSER 指定時のみ)
```

## テスト

```sh
cargo test
```

- ユニットテスト: モデル・エディタ・キー操作・保存処理・描画 (`ratatui::backend::TestBackend`)
- 結合テスト: Tokio で立てたモックサーバへの送信、タイムアウト、接続エラー、キー入力から送信までの流れ
- シェルテスト: 生成した curl コマンドを bash / sh / dash / zsh で実行し、curl が受け取る引数が一致するか確認 (未インストールのシェルはスキップ)
