//! 設定ファイル (`~/.config/curl_tui/config.jsonc`) の読み込みと雛形の生成。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use serde::Deserialize;

use crate::i18n::{self, Lang};
use crate::keymap::{Command, Ctx, DEFAULTS, Keymap, Section};
use crate::vimrc::{self, Vimrc};
use crate::{t, tf};

/// 設定ファイルの場所: `$CURL_TUI_CONFIG` → `$XDG_CONFIG_HOME/curl_tui/config.jsonc`
/// → `~/.config/curl_tui/config.jsonc`。
pub fn default_path() -> PathBuf {
    let env = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    if let Some(path) = env("CURL_TUI_CONFIG") {
        return PathBuf::from(path);
    }
    let base = env("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("curl_tui").join("config.jsonc")
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    #[serde(default)]
    lang: Option<String>,
    #[serde(default)]
    keymap: Vec<SectionFile>,
    #[serde(default)]
    vimrc: Option<VimrcFile>,
}

/// `"vimrc"` は行の配列でも、改行を含む 1 つの文字列でもよい。
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum VimrcFile {
    Text(String),
    Lines(Vec<String>),
}

impl VimrcFile {
    fn lines(&self) -> Vec<&str> {
        match self {
            VimrcFile::Text(text) => text.lines().collect(),
            VimrcFile::Lines(lines) => lines.iter().map(String::as_str).collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SectionFile {
    #[serde(default)]
    context: Option<String>,
    #[serde(default)]
    bindings: BTreeMap<String, Option<String>>,
}

/// 設定ファイルの内容。
#[derive(Debug, Default)]
pub struct Config {
    /// 表示言語 (`"lang"`)。省略時は英語。
    pub lang: Lang,
    pub keymap: Keymap,
    /// Vim 形式のキーマッピング (`"vimrc"`)
    pub vimrc: Vimrc,
}

impl Config {
    /// 設定ファイルで追加したキーバインドとマッピングの数。
    pub fn user_count(&self) -> usize {
        self.keymap.user_count() + self.vimrc.maps.len()
    }
}

/// 設定ファイルを読む。ファイルが無ければ既定の設定を返す。
pub fn load(path: &Path) -> anyhow::Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let text = std::fs::read_to_string(path).with_context(|| cannot_read(path))?;
    parse(&text)
}

/// 設定ファイルの `"lang"` だけを読む。ファイルが無い・壊れている・`"lang"` が無い
/// (または不正な) ときは `None`。他の設定にエラーがあっても、そのエラーを指定された
/// 言語で表示できるようにするためのもの。
pub fn peek_lang(path: &Path) -> Option<Lang> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&strip_jsonc(&text)).ok()?;
    Lang::from_code(value.get("lang")?.as_str()?)
}

/// JSONC の設定を解釈する。
pub fn parse(text: &str) -> anyhow::Result<Config> {
    let json = strip_jsonc(text);
    if json.trim().is_empty() {
        return Ok(Config::default());
    }
    let file: ConfigFile =
        serde_json::from_str(&json).context(t!("invalid JSON", "JSON が不正です"))?;
    let lang = match file.lang.as_deref() {
        None => Lang::default(),
        Some(code) => Lang::from_code(code).ok_or_else(|| {
            let codes: Vec<String> = Lang::ALL
                .iter()
                .map(|l| format!("\"{}\"", l.code()))
                .collect();
            let codes = codes.join(" / ");
            anyhow!(tf!(
                "unknown \"lang\" \"{code}\" (available: {codes})",
                "不明な \"lang\" \"{code}\" (使える値: {codes})"
            ))
        })?,
    };
    let sections: Vec<Section> = file
        .keymap
        .into_iter()
        .map(|s| Section {
            context: s.context.unwrap_or_default(),
            bindings: s.bindings.into_iter().collect(),
        })
        .collect();
    let keymap = Keymap::with_user(&sections).map_err(|e| anyhow!(e))?;
    let vimrc = match &file.vimrc {
        None => Vimrc::default(),
        Some(v) => vimrc::parse(&v.lines()).map_err(|e| anyhow!(e))?,
    };
    Ok(Config {
        lang,
        keymap,
        vimrc,
    })
}

/// コメント (`//` と `/* */`) と末尾カンマを空白に置き換えて、普通の JSON にする。
/// 文字数と改行はそのままなので、エラーの行番号は元のファイルと一致する。
pub fn strip_jsonc(text: &str) -> String {
    let mut out: Vec<char> = Vec::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let blank = |c: char| if c == '\n' { '\n' } else { ' ' };
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push(c);
                while let Some(c) = chars.next() {
                    out.push(c);
                    match c {
                        '\\' => out.extend(chars.next()),
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '/' if chars.peek() == Some(&'/') => {
                out.push(' ');
                while let Some(c) = chars.next_if(|c| *c != '\n') {
                    out.push(blank(c));
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                out.extend([' ', ' ']);
                let mut prev = ' ';
                for c in chars.by_ref() {
                    out.push(blank(c));
                    if prev == '*' && c == '/' {
                        break;
                    }
                    prev = c;
                }
            }
            _ => out.push(c),
        }
    }
    // 末尾カンマ: `,` の次の (空白以外の) 文字が `]` か `}` なら消す
    let mut in_string = false;
    let mut i = 0;
    while i < out.len() {
        match out[i] {
            '\\' if in_string => i += 1,
            '"' => in_string = !in_string,
            ',' if !in_string => {
                let next = out[i + 1..].iter().find(|c| !c.is_whitespace());
                if matches!(next, Some(']' | '}')) {
                    out[i] = ' ';
                }
            }
            _ => {}
        }
        i += 1;
    }
    out.into_iter().collect()
}

/// リファレンスの始まりと終わりの目印 (英語, 日本語)。どちらの言語で書かれた
/// ファイルでも見つけられるよう、両方を探す。
const REFERENCE_BEGIN: [&str; 2] = [
    "// ==== Reference (auto-generated) ====",
    "// ==== リファレンス (自動生成) ====",
];
const REFERENCE_END: [&str; 2] = [
    "// ==== End of reference ====",
    "// ==== リファレンスここまで ====",
];

fn marker(markers: [&'static str; 2]) -> &'static str {
    t!(markers[0], markers[1])
}

/// 最初に現れる目印の (位置, 長さ)。
fn find_marker(text: &str, markers: [&str; 2]) -> Option<(usize, usize)> {
    markers
        .iter()
        .filter_map(|m| text.find(m).map(|at| (at, m.len())))
        .min()
}

/// エディタで開く前の準備。ファイルが無ければ雛形を作り、あればリファレンスの
/// コメント部分だけを最新の内容に更新する (目印の行が無ければ何も変えない)。
pub fn prepare(path: &Path) -> anyhow::Result<()> {
    let current = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| cannot_read(path)),
    };
    let updated = if current.trim().is_empty() {
        template()
    } else {
        refresh_reference(&current)
    };
    if updated == current {
        return Ok(());
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).with_context(|| {
            tf!(
                "cannot create {p}",
                "{p} を作成できません",
                p = parent.display()
            )
        })?;
    }
    std::fs::write(path, updated).with_context(|| {
        tf!(
            "cannot write to {p}",
            "{p} に書き込めません",
            p = path.display()
        )
    })
}

fn cannot_read(path: &Path) -> String {
    tf!(
        "cannot read {p}",
        "{p} を読み込めません",
        p = path.display()
    )
}

fn refresh_reference(text: &str) -> String {
    let Some((begin, _)) = find_marker(text, REFERENCE_BEGIN) else {
        return text.to_string();
    };
    let Some((end, len)) = find_marker(&text[begin..], REFERENCE_END) else {
        return text.to_string();
    };
    let end = begin + end + len;
    format!("{}{}{}", &text[..begin], reference(), &text[end..])
}

/// 新しく作る設定ファイルの内容 (現在の表示言語で書く)。
pub fn template() -> String {
    tf!(
        r#"// curl_tui config file (JSONC: comments and trailing commas are allowed)
//
// "lang" is the display language: "en" (English, default) or "ja" (Japanese).
//
// Key bindings in "keymap" are added on top of (and override) the defaults.
// "vimrc" takes Vim map commands such as "imap jj <Esc>" (see the reference below).
//   "context"  : where the bindings are active. Omit it to make them active everywhere
//   "bindings" : {{ "key": "command name" }}. Use null to disable a key
// When several bindings match the same key, the one written later wins.
// Changes take effect as soon as you save and quit the editor.
//
// Key syntax, contexts, commands and the default key bindings are listed in
// the second half of this file (the reference).
{{
  "lang": "{lang}",
  "keymap": [
    // Examples: remove the leading // to enable them
    // {{
    //   "context": "Normal",
    //   "bindings": {{
    //     "ctrl-g": "app::Send",        // Ctrl+G also sends
    //     "space w": "app::Save",       // Space then w saves
    //     "q": null                     // q no longer quits
    //   }}
    // }},
    // {{
    //   "context": "Insert",
    //   "bindings": {{ "j k": "vim::NormalMode" }}   // jk returns to Normal mode
    // }}
  ],
  // Vim-style mappings (like a .vimrc). Each line is one command
  "vimrc": [
    // Examples: remove the leading // to enable them
    // "imap jj <Esc>",                  // jj in Insert mode returns to Normal mode
    // "let mapleader = ' '",            // use Space as <Leader>
    // "nnoremap <Leader>w :w<CR>",      // Space then w saves
    // "nnoremap <Leader>s <C-r>",       // Space then s sends the request
    // "set timeoutlen=500"              // wait 500 ms for the rest of a mapping
  ]
}}

{reference}
"#,
        r#"// curl_tui の設定ファイル (JSONC: コメントと末尾のカンマが使えます)
//
// "lang" は表示言語です: "en" (英語、既定) または "ja" (日本語)。
//
// "keymap" にキーバインドを書くと、既定のキーバインドに追加・上書きされます。
// "vimrc" には "imap jj <Esc>" のような Vim の map コマンドを書けます (後半のリファレンス参照)。
//   "context"  : そのバインドが有効になる環境。省略するとどこでも有効です
//   "bindings" : {{ "キー": "コマンド名" }}。null を指定するとそのキーを無効にします
// 同じキーが複数当てはまるときは、後に書いたものが優先されます。
// 保存してエディタを終了すると、すぐに反映されます。
//
// キーの書き方・コンテキスト・コマンドと既定のキーバインドの一覧は、
// このファイルの後半 (リファレンス) にあります。
{{
  "lang": "{lang}",
  "keymap": [
    // 例: 先頭の // を外すと有効になります
    // {{
    //   "context": "Normal",
    //   "bindings": {{
    //     "ctrl-g": "app::Send",        // Ctrl+G でも送信
    //     "space w": "app::Save",       // Space → w で保存
    //     "q": null                     // q では終了しない
    //   }}
    // }},
    // {{
    //   "context": "Insert",
    //   "bindings": {{ "j k": "vim::NormalMode" }}   // jk で Normal モードへ
    // }}
  ],
  // Vim 形式のマッピング (.vimrc と同じ書き方)。1 行に 1 コマンド
  "vimrc": [
    // 例: 先頭の // を外すと有効になります
    // "imap jj <Esc>",                  // Insert モードで jj → Normal モードへ
    // "let mapleader = ' '",            // <Leader> を Space にする
    // "nnoremap <Leader>w :w<CR>",      // Space → w で保存
    // "nnoremap <Leader>s <C-r>",       // Space → s で送信
    // "set timeoutlen=500"              // マッピングの続きのキーを 500 ms 待つ
  ]
}}

{reference}
"#,
        lang = i18n::current().code(),
        reference = reference()
    )
}

/// コンテキスト・コマンド・既定のキーバインドの一覧 (すべてコメント)。
pub fn reference() -> String {
    let mut lines = vec![marker(REFERENCE_BEGIN).to_string()];
    let mut line = |s: &str| {
        lines.push(if s.is_empty() {
            "//".to_string()
        } else {
            format!("// {s}")
        })
    };
    line(t!(
        "Everything from here to the \"End of reference\" line is rewritten with the latest",
        "ここから「リファレンスここまで」の行までは、:config で開くたびに最新の内容に"
    ));
    line(t!(
        "content every time you open :config. Put your own settings in \"keymap\" above.",
        "書き直されます。自分の設定は上の \"keymap\" に書いてください。"
    ));
    line("");
    line(t!("■ Key syntax", "■ キーの書き方"));
    line(t!(
        "  Modifiers go first        : \"ctrl-r\" \"alt-x\" \"shift-g\" \"ctrl-shift-up\"",
        "  修飾キーは前に付けます : \"ctrl-r\" \"alt-x\" \"shift-g\" \"ctrl-shift-up\""
    ));
    line(t!(
        "  Key sequences use spaces  : \"g g\" \"ctrl-w h\" \"space w\"",
        "  続けて押すキーは空白で : \"g g\" \"ctrl-w h\" \"space w\""
    ));
    line(t!(
        "  Symbols are written as is : \"?\" \"$\" \":\" (no shift-)",
        "  記号はそのまま書きます : \"?\" \"$\" \":\" (shift- は付けません)"
    ));
    line(t!(
        "  Special keys              : enter escape tab backspace delete space insert",
        "  特殊キー               : enter escape tab backspace delete space insert"
    ));
    line(t!(
        "                              up down left right home end pageup pagedown f1-f12",
        "                           up down left right home end pageup pagedown f1〜f12"
    ));
    line("");
    line(t!(
        "■ Contexts (names usable in \"context\"; combine them with && || ! ( ))",
        "■ コンテキスト (\"context\" に書ける名前。&& || ! ( ) で組み合わせられます)"
    ));
    for ctx in Ctx::ALL {
        line(&format!("  {:<11}{}", ctx.name(), ctx.description()));
    }
    line(t!(
        "  e.g. \"Normal && Collection\"  \"Insert && !Url\"  \"Normal && (Url || Request)\"",
        "  例: \"Normal && Collection\"  \"Insert && !Url\"  \"Normal && (Url || Request)\""
    ));
    line("");
    line(t!(
        "■ Default key bindings (\"key\": \"command name\", // what it does)",
        "■ 既定のキーバインド (\"キー\": \"コマンド名\", // 動作)"
    ));
    line(t!(
        "  After d / c / y (operators), keys bound to (motion) commands can be used.",
        "  d / c / y (オペレータ) の後は、(モーション) のコマンドに割り当てたキーが使えます。"
    ));
    line("[");
    for (context, bindings) in DEFAULTS {
        line("  {");
        if context.is_empty() {
            line(t!(
                "    // no \"context\": active everywhere",
                "    // \"context\" なし: どこでも有効"
            ));
        } else {
            line(&format!("    \"context\": \"{context}\","));
        }
        line("    \"bindings\": {");
        for (key, command) in *bindings {
            let key = format!("\"{}\":", json_escape(key));
            let name = format!("\"{}\",", command.name());
            line(&format!(
                "      {key:<17}{name:<34}// {}",
                command.description()
            ));
        }
        line("    }");
        line("  },");
    }
    line("]");
    let unbound: Vec<Command> = Command::ALL
        .iter()
        .copied()
        .filter(|c| {
            !DEFAULTS
                .iter()
                .any(|(_, bindings)| bindings.iter().any(|(_, b)| b == c))
        })
        .collect();
    if !unbound.is_empty() {
        line("");
        line(t!(
            "■ Commands with no default key binding",
            "■ 既定ではキーが割り当てられていないコマンド"
        ));
        for command in unbound {
            line(&format!(
                "  {:<24}{}",
                format!("\"{}\"", command.name()),
                command.description()
            ));
        }
    }
    line("");
    for l in vimrc_reference() {
        line(l);
    }
    line("");
    line(t!("■ : commands", "■ : コマンド"));
    line(t!(
        "  :w save  :q quit  :wq save and quit  :send send  :help help  :config open this file",
        "  :w 保存  :q 終了  :wq 保存して終了  :send 送信  :help ヘルプ  :config この設定を開く"
    ));
    lines.push(marker(REFERENCE_END).to_string());
    lines.join("\n")
}

/// リファレンスの "vimrc" の説明。
fn vimrc_reference() -> Vec<&'static str> {
    let en = [
        "■ \"vimrc\" (Vim-style mappings: replace keys with other keys)",
        "  \"vimrc\" is an array of lines (or one string with newlines). Lines starting with \" are comments.",
        "  The right-hand side is the keys to press instead, so anything you can type works,",
        "  including : commands: \"nnoremap <Leader>w :w<CR>\"",
        "  imap / inoremap      Insert mode",
        "  nmap / nnoremap      Normal mode",
        "  vmap / xmap / vnoremap / xnoremap   Visual mode",
        "  omap / onoremap      after an operator (d / c / y)",
        "  cmap / cnoremap      : command line and the rename prompt",
        "  map / noremap        Normal + Visual + operator      map! / noremap!   Insert + command line",
        "  *unmap {lhs}         remove a mapping written earlier (nunmap, iunmap, unmap!, ...)",
        "  {rhs} = <Nop>        make the key do nothing (e.g. \"nnoremap q <Nop>\")",
        "  <silent> <buffer> <unique> are accepted and ignored. <nowait> does not wait for longer mappings",
        "  Keys: <Esc> <CR> <Tab> <S-Tab> <BS> <Del> <Space> <Up> <F5> <C-x> <M-x> (<A-x>) <lt> <Bar> <Bslash>",
        "        <Leader> <LocalLeader> (default \\; change with let mapleader = ' ' before the mappings)",
        "  set timeoutlen=1000  how long (ms) to wait for the rest of a mapping. set notimeout waits forever",
    ];
    let ja = [
        "■ \"vimrc\" (Vim 形式のマッピング: キーを別のキー列に置き換える)",
        "  \"vimrc\" は行の配列 (または改行を含む 1 つの文字列) です。\" で始まる行はコメントです。",
        "  右辺は代わりに押すキー列なので、: コマンドも含め、手で打てるものは何でも書けます:",
        "  \"nnoremap <Leader>w :w<CR>\"",
        "  imap / inoremap      Insert モード",
        "  nmap / nnoremap      Normal モード",
        "  vmap / xmap / vnoremap / xnoremap   Visual モード",
        "  omap / onoremap      オペレータ (d / c / y) の後",
        "  cmap / cnoremap      : コマンドラインとリクエスト名の入力欄",
        "  map / noremap        Normal + Visual + オペレータの後   map! / noremap!   Insert + コマンドライン",
        "  *unmap {lhs}         前に書いたマッピングを取り消す (nunmap, iunmap, unmap! など)",
        "  {rhs} = <Nop>        そのキーで何もしない (例: \"nnoremap q <Nop>\")",
        "  <silent> <buffer> <unique> は書けますが無視します。<nowait> は長いマッピングの続きを待ちません",
        "  キー: <Esc> <CR> <Tab> <S-Tab> <BS> <Del> <Space> <Up> <F5> <C-x> <M-x> (<A-x>) <lt> <Bar> <Bslash>",
        "        <Leader> <LocalLeader> (既定は \\。マッピングより前に let mapleader = ' ' で変更)",
        "  set timeoutlen=1000  マッピングの続きのキーを待つ時間 (ミリ秒)。set notimeout で待ち続けます",
    ];
    match crate::i18n::current() {
        crate::i18n::Lang::En => en.to_vec(),
        crate::i18n::Lang::Ja => ja.to_vec(),
    }
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// 設定ファイルを開くエディタ (`$VISUAL` → `$EDITOR` → vi / notepad) と、その引数。
pub fn editor_command() -> (String, Vec<String>) {
    let configured = ["VISUAL", "EDITOR"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|v| !v.trim().is_empty());
    let fallback = if cfg!(windows) { "notepad" } else { "vi" };
    let command = configured.unwrap_or_else(|| fallback.to_string());
    let mut parts = command.split_whitespace().map(str::to_string);
    let program = parts.next().unwrap_or_else(|| fallback.to_string());
    (program, parts.collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::{ContextSet, Lookup, parse_keys};

    #[test]
    fn strips_comments_and_trailing_commas() {
        let text = r#"{
  // line "comment"
  "a": "x // not a comment", /* block
  comment */ "b": [1, 2, ],
  "c": "quote \" , ]",
}"#;
        let json = strip_jsonc(text);
        assert_eq!(json.lines().count(), text.lines().count());
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["a"], "x // not a comment");
        assert_eq!(v["b"], serde_json::json!([1, 2]));
        assert_eq!(v["c"], "quote \" , ]");
    }

    #[test]
    fn template_is_valid_and_changes_nothing() {
        let text = template();
        let km = parse(&text).unwrap().keymap;
        assert_eq!(km.user_count(), 0);
        // 全コマンドと全コンテキストが載っている
        for command in Command::ALL {
            assert!(text.contains(command.name()), "{}", command.name());
        }
        for ctx in Ctx::ALL {
            assert!(text.contains(ctx.name()));
        }
    }

    #[test]
    fn reference_defaults_can_be_pasted_into_keymap() {
        // リファレンスの「既定のキーバインド」はコメントを外せばそのまま使える
        let reference = reference();
        let start = reference.find("// [\n").unwrap();
        let end = reference.find("// ]\n").unwrap() + 5;
        let body: String = reference[start..end]
            .lines()
            .map(|l| l.strip_prefix("// ").unwrap_or("").to_string() + "\n")
            .collect();
        let km = parse(&format!("{{ \"keymap\": {body} }}")).unwrap().keymap;
        let count: usize = DEFAULTS.iter().map(|(_, b)| b.len()).sum();
        assert_eq!(km.user_count(), count);
    }

    #[test]
    fn parses_user_keymap() {
        let km = parse(
            r#"{
  "keymap": [
    { "context": "Normal", "bindings": { "ctrl-g": "app::Send", "q": null, } },
    { "bindings": { "f2": "app::OpenConfig" } },
  ],
}"#,
        )
        .unwrap()
        .keymap;
        let normal = ContextSet::default().with(Ctx::Normal);
        assert_eq!(km.lookup(normal, &parse_keys("q").unwrap()), Lookup::None);
        assert!(matches!(
            km.lookup(normal, &parse_keys("ctrl-g").unwrap()),
            Lookup::Found {
                command: Command::AppSend,
                ..
            }
        ));
        assert!(matches!(
            km.lookup(ContextSet::default(), &parse_keys("f2").unwrap()),
            Lookup::Found { global: true, .. }
        ));
    }

    #[test]
    fn parses_vimrc() {
        let config = parse(
            r#"{
  "vimrc": [
    "\" comment",
    "imap jj <Esc>",
    "let mapleader = ' '",
    "nnoremap <Leader>w :w<CR>",
  ],
}"#,
        )
        .unwrap();
        assert_eq!(config.vimrc.maps.len(), 2);
        assert_eq!(config.user_count(), 2);
        // 改行を含む 1 つの文字列でもよい
        let text = parse(r#"{ "vimrc": "imap jj <Esc>\nset tm=200\n" }"#).unwrap();
        assert_eq!(text.vimrc.maps, config.vimrc.maps[..1]);
        let err = parse(r#"{ "vimrc": ["imap jj <Esc>", "nmap"] }"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("\"vimrc\" line 2"), "{err}");
        // 雛形の例はコメントを外せばそのまま使える
        let uncommented = template().replace("    // \"", "    \"");
        assert_eq!(parse(&uncommented).unwrap().vimrc.maps.len(), 3);
    }

    #[test]
    fn parses_lang() {
        assert_eq!(parse("{}").unwrap().lang, Lang::En);
        assert_eq!(parse(r#"{ "lang": "en" }"#).unwrap().lang, Lang::En);
        assert_eq!(parse(r#"{ "lang": "ja", }"#).unwrap().lang, Lang::Ja);
        let err = parse(r#"{ "lang": "fr" }"#).unwrap_err().to_string();
        assert!(err.contains("\"fr\"") && err.contains("\"ja\""), "{err}");
        assert!(parse(r#"{ "lang": 1 }"#).is_err());
    }

    #[test]
    fn peeks_lang_even_when_the_rest_is_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.jsonc");
        assert_eq!(peek_lang(&path), None);
        std::fs::write(
            &path,
            r#"{ "lang": "ja", "keymap": [ { "bindings": { "x": "nope" } } ] } // c"#,
        )
        .unwrap();
        assert!(load(&path).is_err());
        assert_eq!(peek_lang(&path), Some(Lang::Ja));
        std::fs::write(&path, r#"{ "lang": "fr" }"#).unwrap();
        assert_eq!(peek_lang(&path), None);
        std::fs::write(&path, "{ \"lang\": \"ja\"").unwrap();
        assert_eq!(peek_lang(&path), None);
    }

    #[test]
    fn errors_are_reported() {
        assert!(parse("{ \"keymap\": [ { \"bindings\": { \"x\": \"nope\" } } ] }").is_err());
        assert!(parse("{ \"keymap\": [ { \"context\": \"Nope\" } ] }").is_err());
        assert!(parse("{ \"keymaps\": [] }").is_err());
        assert!(parse("{ \"keymap\": [").is_err());
        // 空 / コメントだけのファイルは既定のまま
        assert_eq!(parse("").unwrap().keymap.user_count(), 0);
        assert_eq!(parse("// nothing\n").unwrap().keymap.user_count(), 0);
    }

    #[test]
    fn prepare_creates_template_and_refreshes_reference() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/config.jsonc");
        assert_eq!(load(&path).unwrap().keymap.user_count(), 0);
        prepare(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), template());

        // 古いリファレンスは書き直し、ユーザーの設定はそのまま残す
        let user = "{ \"keymap\": [ { \"bindings\": { \"f2\": \"app::Save\" } } ] }\n";
        // 目印はどちらの言語のものでも見つける
        for i in 0..2 {
            let (begin, end) = (REFERENCE_BEGIN[i], REFERENCE_END[i]);
            let stale = format!("{user}{begin}\n// old\n{end}\n// mine\n");
            assert_eq!(
                refresh_reference(&stale),
                format!("{user}{}\n// mine\n", reference())
            );
        }
        let (begin, end) = (REFERENCE_BEGIN[1], REFERENCE_END[0]);
        let stale = format!("{user}{begin}\n// old\n{end}\n// mine\n");
        std::fs::write(&path, &stale).unwrap();
        prepare(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with(user));
        assert!(text.ends_with("\n// mine\n"));
        assert!(!text.contains("// old"));
        assert!(text.contains("app::Quit"));
        assert_eq!(load(&path).unwrap().keymap.user_count(), 1);

        // 目印が無いファイルには触らない
        std::fs::write(&path, user).unwrap();
        prepare(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), user);
    }
}
