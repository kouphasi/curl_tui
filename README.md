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
- リクエストに対応する curl コマンドの表示 (cURL タブ、`s` で出力先のシェルを切り替え)
- コレクション管理 (新規 / 複製 / 削除 / 名前変更) と JSON ファイルへの保存
- ブラケットペースト対応、日本語などの全角文字の入力に対応

## 使い方

```sh
cargo run --release                   # 既定: ~/.config/curl_tui/collection.json
cargo run --release -- my_api.json    # 保存先ファイルを指定
```

### キー操作

| キー | 動作 |
| --- | --- |
| `Tab` / `Shift+Tab` | ペイン移動 (Collection → URL → Request → Response) |
| `Ctrl+R` / `F5` | 送信 |
| `Ctrl+S` | コレクションを保存 |
| `m` / `M` | メソッド切り替え |
| `i` / `Enter` | フォーカス中の欄を編集 (`Esc` で終了) |
| `h` / `l`, `1`-`4` | タブ切り替え |
| `s` | (cURL タブ) 出力先のシェルを切り替え |
| `j` / `k`, `g` / `G`, `d` / `u` | 選択・スクロール |
| `n` / `y` / `d` / `r` | (Collection) 新規 / 複製 / 削除 / 名前変更 |
| `Ctrl+A` / `Ctrl+E` / `Ctrl+K` / `Ctrl+U` | (編集中) 行頭 / 行末 / 行末まで削除 / 全消去 |
| `?` | ヘルプ |
| `q` / `Ctrl+C` | 終了 |

## cURL タブとシェルの違い

シェルごとにクォートの規則が違うため、出力先を選べるようにしています。既定は環境変数 `SHELL` から推測します。

| 出力先 | クォート | 行継続 | 備考 |
| --- | --- | --- | --- |
| bash / zsh / sh | `'…'`、`'` は `'\''` | `\` | |
| fish | `'…'`、`\` と `'` は `\\` / `\'` | `\` | fish は `'…'` の中でも `\\` を `\` に変えるため別扱い |
| PowerShell 7.3+ | `'…'`、`'` は `''` | `` ` `` | `curl.exe` を呼ぶ形で出力 |

- Windows PowerShell 5.1 は外部コマンドに渡す引数の `"` を落とすため、JSON ボディは正しく渡りません。PowerShell 7.3 以降を使ってください。
- コマンドプロンプト (cmd.exe) は `%` の展開を止められないなど安全に出力できないため対応していません。

## 構成

```
src/
├── main.rs        # Tokio のイベントループ (キー入力 / HTTP 結果 / tick を select!)
├── app.rs         # アプリ状態とキー処理 (I/O を持たない状態機械)
├── ui.rs          # Ratatui による描画
├── editor.rs      # 単一行 / 複数行テキストエディタ
├── curl.rs        # シェル別の curl コマンド生成
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
- シェルテスト: 生成した curl コマンドを bash / sh / dash / zsh / fish で実行し、curl が受け取る引数が一致するか確認 (未インストールのシェルはスキップ)
