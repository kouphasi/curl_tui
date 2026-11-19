# curl_tui

Rust で作った Postman 風の TUI HTTP クライアントです。

- TUI: [Ratatui](https://ratatui.rs)
- Backend: crossterm
- Async Runtime: Tokio (HTTP は reqwest + rustls)

## 機能

- メソッド切り替え (GET / POST / PUT / PATCH / DELETE / HEAD / OPTIONS)
- URL・クエリパラメータ・ヘッダ・ボディの編集 (`#` で始まる行は無効化)
- 外部エディタでの編集 (`Ctrl+E` で選択中の欄を `$EDITOR` (nvim など) で開き、保存して閉じると反映。lazygit と同じ方式)
- 非同期送信 (UI をブロックしない / スピナー表示 / 30 秒タイムアウト)
- レスポンスのステータス・所要時間・サイズ・ヘッダ表示、JSON の自動整形
- HTML レスポンスのプレビュー (Preview タブ。ヘッドレス Chromium で描画し、kitty graphics protocol で端末に表示)
- リクエストに対応する curl コマンドの表示 (cURL タブ、bash / zsh / sh 向け)
- Visual モードでの文字選択とクリップボードへのコピー (`v` / `V` → `y`。cURL タブやレスポンスの Body / Headers でも可)
- コレクション管理 (新規 / 複製 / 削除 / 名前変更) と JSON ファイルへの保存
- キーバインドのカスタマイズ (JSONC の設定ファイル。`:config` でエディタが開き、保存して閉じると反映)
- Vim 形式のキーマッピング (`~/.config/curl_tui/vimrc` に `imap jj <Esc>` のように書ける。`:vimrc` で編集)
- 表示言語は英語 (既定) / 日本語 (設定ファイルに `"lang": "ja"`)
- マウス操作 (クリックでペイン / リクエスト / タブ / カーソル位置を選択、ホイールでスクロール)
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
tar xzf curl_tui-0.2.1-x86_64-unknown-linux-musl.tar.gz
sudo mv curl_tui-0.2.1-x86_64-unknown-linux-musl/curl_tui /usr/local/bin/
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
| `Shift+Esc` / `Ctrl+W z` | 選択中のペインを全画面表示 / 元に戻す |
| `Ctrl+R` / `F5` / `:send` | 送信 |
| `:w` / `:q` / `:wq` (`:x`) | 保存 / 終了 / 保存して終了 (`Ctrl+S` でも保存) |
| `m` / `M` | メソッド切り替え |
| `H` / `L`, `gT` / `gt`, `1`-`4` | タブ切り替え |
| `?` | ヘルプ |
| `Ctrl+E` / `:edit` (`:e`) | 選択中のテキスト欄 (URL / Params / Headers / Body) を `$EDITOR` で開き、保存して閉じると反映 (`u` で元に戻せる) |
| `:config` | キーバインドの設定ファイルをエディタで開く |
| `:vimrc` | Vim 形式のマッピング (vimrc) をエディタで開く |
| `q` / `Ctrl+C` | 終了 |

#### マウス

| 操作 | 動作 |
| --- | --- |
| ペインをクリック | そのペインにフォーカス |
| Collection のリクエストをクリック | リクエストを選択 |
| タブ (Params / Headers / Body / cURL、Body / Headers / Preview) をクリック | タブ切り替え |
| テキスト欄 (URL / Params / Headers / Body / cURL、Response の Body / Headers) をクリック | クリックした位置にカーソルを移動 |
| Response / cURL タブ上でホイール | スクロール |

マウス操作を受け取るため、端末の文字選択 (コピー) は `Shift` を押しながらドラッグしてください
(端末によっては `Option` / `Alt`)。

ペインは左に Collection、右に上から URL → Request → Response と並んでいます。
Collection から `Ctrl+L` で戻ると、直前にいた右側のペインに戻ります。

`Shift+Esc` は kitty keyboard protocol に対応した端末 (kitty / Ghostty / WezTerm / foot など) で使えます。
それ以外の端末では `Shift+Esc` が `Esc` と区別できないため、`Ctrl+W z` を使ってください。
全画面表示中もペイン移動はでき、移動先のペインが全画面で表示されます。

#### テキスト欄 (URL / Params / Headers / Body)

| キー | 動作 |
| --- | --- |
| `h` `j` `k` `l`, `w` `b` `e`, `0` `^` `$`, `gg` `G` | カーソル移動 |
| `i` `a` `I` `A` `o` `O` (`Enter` = `A`) | Insert モードへ (`Esc` で Normal に戻る) |
| `x` `X` `s` `S` `D` `C` `r{文字}` | 1 文字削除 / 置換 / 行末まで削除 など |
| `d` `c` `y` + モーション | `dw` `cw` `d$` `dj` `dgg` `dd` `cc` `yy` など |
| `p` / `P`, `Y` | 貼り付け (後 / 前)、行ヤンク |
| `v` / `V` | Visual モード (文字単位 / 行単位) で選択。`y` でクリップボードにコピー |
| `u` | 元に戻す (Insert 1 回分をまとめて戻す) |
| `Ctrl+E` (Normal 中) / `:edit` | 欄の内容を `$EDITOR` で開く。保存して閉じると反映 (`u` で編集前に戻せる)。Body は中身に応じて `.json` / `.html` / `.xml` の一時ファイルになるのでハイライトが効く |
| `Ctrl+W` / `Ctrl+H` (Insert 中) | 直前の単語 / 1 文字を削除 |
| `Ctrl+A` / `Ctrl+E` / `Ctrl+K` / `Ctrl+U` (Insert 中) | 行頭 / 行末 / 行末まで削除 / 全消去 |

#### 選択とコピー (Visual モード)

URL / Params / Headers / Body に加えて、編集できない cURL タブと Response の Body / Headers タブでも使えます。

| キー | 動作 |
| --- | --- |
| `v` / `V` | 文字単位 / 行単位の選択を始める (もう一度押すと解除、`v` ⇔ `V` で切り替え) |
| `h` `j` `k` `l`, `w` `b` `e`, `0` `^` `$`, `gg` `G` | 選択範囲を伸縮 |
| `o` | 選択範囲の反対側の端へカーソルを移す |
| `y` | 選択範囲をクリップボードにコピーして Normal に戻る |
| `Esc` | 選択を解除 |

- キーは設定ファイルで変えられます (コンテキスト `Visual` / `TextView`、コマンド `vim::Visual` / `vim::VisualLine` / `vim::VisualSwapEnds` / `vim::Yank`)。
- Visual モードでは文字列を変更できません (`x` `d` `c` などは無視されます)。
- Normal モードの `yy` `yw` `y$` `Y` などのヤンクもクリップボードにコピーされます (`d` / `x` での削除はコピーされません)。
- cURL タブと Response の Body / Headers タブは読み取り専用です。カーソル移動・選択・ヤンクだけができ、
  `x` `r` `i` `p` などの文字列を変えるキーは無視されます。
- コピーは OS のコピーコマンド (macOS: `pbcopy`、Wayland: `wl-copy`、X11: `xclip` / `xsel`) と、
  端末への OSC 52 エスケープシーケンスの両方で行います。SSH 越しや Windows では OSC 52 に対応した端末
  (kitty / Ghostty / WezTerm / iTerm2 / Windows Terminal など) が必要です。tmux では `set -g set-clipboard on` にしてください。

#### Collection / Response

| キー | 動作 |
| --- | --- |
| `j` / `k`, `gg` / `G` | (Collection) 選択移動 |
| `o` (`n`) / `O` | (Collection) 下 / 上に新規作成 |
| `yy` / `dd` / `p` / `P` | (Collection) ヤンク / 削除 / 貼り付け (`yyp` で複製) |
| `r`, `Enter` / `l` | (Collection) 名前変更、URL ペインへ |
| `h` `j` `k` `l`, `w` `b` `e`, `0` `^` `$`, `gg` `G` | (Response の Body / Headers) カーソル移動 (画面端でスクロール) |
| `v` / `V` → `y`, `yy` など | (Response の Body / Headers) 選択 / クリップボードにコピー |
| `Ctrl+E` / `Ctrl+Y` | (Response) 1 行スクロール |
| `Ctrl+D` / `Ctrl+U`, `Ctrl+F` / `Ctrl+B` | (Response) 半ページ / 1 ページ送り |
| `j` / `k`, `gg` / `G` | (Preview タブ) スクロール、先頭 / 末尾 |
| `1` / `2` / `3`, `H` / `L` | (Response) Body / Headers / Preview タブ (Preview は HTML のときだけ) |
| `+` / `-` / `0` | (Preview タブ) 拡大 / 縮小 / 倍率リセット |

入力途中のキー (`d`, `g`, `^W` など) はヘッダ行の右端に表示されます。`Esc` で取り消せます。

## 表示言語

画面やメッセージの言語は英語 (既定) と日本語から選べます。設定ファイル
`~/.config/curl_tui/config.jsonc` (`XDG_CONFIG_HOME` があればその下。`:config` で開けます) に
`"lang"` を書いてください。

```jsonc
{
  "lang": "ja"   // "en" (既定) または "ja"
}
```

保存してエディタを終了すると、その場で切り替わります。設定ファイル後半のリファレンス (コメント) も、
次に `:config` で開いたときにその言語で書き直されます。

## キーバインドの設定

キーバインドは `~/.config/curl_tui/config.jsonc` (`XDG_CONFIG_HOME` があればその下) で変更できます。
何も書かなければ上の表の既定のキーのままです。

1. TUI で `:config` を実行する (`app::OpenConfig` をキーに割り当てることもできます)
2. `$VISUAL` / `$EDITOR` (未設定なら `vi`) で設定ファイルが開く。初回は雛形が作られます
3. 編集して保存し、エディタを終了すると、その場で反映される

GUI のエディタを使う場合は、`EDITOR="code --wait"` のように閉じるまで待つオプションを付けてください。
設定にエラーがあるとステータス行に表示され、それまでのキーバインドがそのまま使われます。

書式は [Zed のキーマップ](https://zed.dev/docs/key-bindings) にならっています。
「どの環境 (コンテキスト) で」「どのキーを押すと」「どのコマンドが動くか」を並べます。

```jsonc
{
  "keymap": [
    {
      "context": "Normal",
      "bindings": {
        "ctrl-g": "app::Send",   // Ctrl+G でも送信
        "space w": "app::Save",  // Space → w で保存
        "q": null                // q では終了しない
      }
    },
    {
      "context": "Insert",
      "bindings": { "j k": "vim::NormalMode" }  // jk で Normal モードへ
    }
  ]
}
```

- 設定は既定のキーバインドに追加・上書きされます。同じキーが複数当てはまるときは後に書いたものが優先です
- `null` を指定すると、そのキーの割り当てを無効にできます
- `"context"` は `Normal` / `Insert` / `Collection` / `Url` / `Request` / `Response` / `Editor` などの名前を
  `&&` `||` `!` `( )` で組み合わせて書きます (例: `"Normal && Collection"`)。省略するとどこでも有効です
- キーは `ctrl-r` / `shift-g` / `escape` / `f5` のように書き、続けて押すキーは `"g g"` のように空白で区切ります

コンテキスト・コマンド名とその動作・既定のキーバインドの一覧は、設定ファイルの後半にコメントで入っています
(`:config` で開くたびに最新の内容に更新されます)。既定のキーバインドは設定と同じ書式なので、
必要な行をコピーして `"keymap"` に貼り付ければそのまま使えます。

| 環境変数 | 意味 |
| --- | --- |
| `CURL_TUI_CONFIG` | 設定ファイルの場所を変える |
| `CURL_TUI_VIMRC` | vimrc の場所を変える (既定は設定ファイルと同じディレクトリの `vimrc`) |
| `VISUAL` / `EDITOR` | `:config` / `:edit` (`Ctrl+E`) で使うエディタ (未設定なら `vi`) |

## Vim 形式のマッピング (vimrc)

`~/.config/curl_tui/vimrc` (`config.jsonc` と同じディレクトリ) に、`.vimrc` と同じ書き方でキーマッピングを書けます。
TUI で `:vimrc` を実行するとエディタで開き、保存して閉じるとその場で反映されます (初回は例とリファレンスをコメントで書いた雛形が作られます)。
`"keymap"` が「キー → コマンド」なのに対し、こちらは Vim と同じ「キー → キー列」の置き換えです。

```vim
" コメント
imap jj <Esc>
let mapleader = ' '
nnoremap <Leader>w :w<CR>
nnoremap <Leader>s <C-r>
nnoremap q <Nop>
vnoremap Y y
set timeoutlen=500
```

| コマンド | 有効なモード |
| --- | --- |
| `imap` / `inoremap` | Insert |
| `nmap` / `nnoremap` | Normal |
| `vmap` / `xmap` / `vnoremap` / `xnoremap` | Visual |
| `omap` / `onoremap` | オペレータ (`d` / `c` / `y`) の後 |
| `cmap` / `cnoremap` | `:` コマンドライン・リクエスト名の入力欄 |
| `map` / `noremap` | Normal + Visual + オペレータの後 |
| `map!` / `noremap!` | Insert + コマンドライン |
| `unmap` / `nunmap` / `iunmap` / … | 前に書いたマッピングを取り消す |

- 右辺は「代わりに押すキー」なので、`:w<CR>` のような `:` コマンドや `"keymap"` で割り当てたキーも含め、手で打てるものは何でも書けます
- `map` 系は展開後のキーにもマッピングを適用し、`noremap` 系は適用しません (Vim と同じ)
- 続きのキーが `timeoutlen` (既定 1000 ミリ秒) の間に来なければ、押したキーはそのまま (または短いマッピングとして) 処理されます。
  `jj` のマッピングがあっても `j` を 1 回だけ打てば、少し待つと `j` が入力されます。`set notimeout` で待ち続けます
- `<Leader>` はマッピングを書いた時点の `mapleader` で展開されます (既定は `\`)。`let mapleader` はマッピングより前に書いてください
- キーの表記: `<Esc>` `<CR>` `<Tab>` `<S-Tab>` `<BS>` `<Del>` `<Space>` `<Up>` `<F5>` `<C-x>` `<M-x>` (`<A-x>`)
  `<lt>` `<Bar>` `<Bslash>` `<Leader>` `<LocalLeader>`。`<silent>` などの引数は書けますが無視します (`<nowait>` は有効)
- 既定のキーを無効にするには `nnoremap q <Nop>` のように `<Nop>` に割り当てます
- 対応していないコマンド (`set number` / `autocmd` / `<expr>` など) はエラーになり、ステータス行に何行目かが表示されます
- 場所は環境変数 `CURL_TUI_VIMRC` で変えられます

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
- 文字の大きさは端末のセルの大きさに合わせて自動で決めます (Retina などの高解像度画面では拡大して描画)。
  `+` / `-` で 25% 刻み (25%〜400%) に拡大・縮小でき、`0` で 100% に戻ります。
  拡大するとページ側のレイアウト幅がその分狭くなります (ブラウザのズームと同じ)

必要なもの:

- kitty graphics protocol 対応端末 (kitty / Ghostty / WezTerm など)。tmux 内では使えません
- Chromium 系ブラウザ (`chrome-headless-shell` / `chromium` / `google-chrome` / Microsoft Edge / Brave を自動で探します)

| 環境変数 | 意味 |
| --- | --- |
| `CURL_TUI_BROWSER` | 使うブラウザの実行ファイル (例: `/usr/bin/chromium`) |
| `CURL_TUI_PREVIEW_ZOOM` | プレビュー倍率の初期値 (%)。例: `150` |
| `CURL_TUI_GRAPHICS` | `kitty` で端末判定を飛ばして強制有効化、`none` で無効化 |

## cURL タブについて

出力は bash / zsh / sh 向けです。値は `'…'` で囲み、中の `'` は `'\''` に置き換えています。
fish・PowerShell・コマンドプロンプトはクォートの規則が違うため、そのまま貼ると正しく動かないことがあります。

コマンド全体をコピーするには、cURL タブで `ggVGy` と入力します。

## 構成

```
src/
├── main.rs        # Tokio のイベントループ (キー入力 / HTTP 結果 / tick を select!)
├── app.rs         # アプリ状態と Vim 風キー処理 (I/O を持たない状態機械)
├── keymap.rs      # コマンド名 / コンテキスト / 既定のキーバインド
├── config.rs      # 設定ファイル (JSONC) の読み込みと雛形の生成
├── vimrc.rs       # vimrc ファイル (Vim 形式のキーマッピング) の読み込みと雛形の生成
├── i18n.rs        # 表示言語 (英語 / 日本語) の切り替え
├── ui.rs          # Ratatui による描画
├── editor.rs      # 単一行 / 複数行テキストエディタ (単語移動・範囲削除・undo)
├── clipboard.rs   # クリップボードへのコピー (コピーコマンド / OSC 52)
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

## リリース手順

`Cargo.toml` の `version` を更新してコミットし、バージョン番号のタグ (例: `0.2.1`。先頭に `v` は付けない) を push すると
GitHub Actions (`.github/workflows/release.yml`) が各 OS 向けにビルドし、Release にバイナリを添付します。

```sh
git tag 0.2.1
git push origin 0.2.1
```
