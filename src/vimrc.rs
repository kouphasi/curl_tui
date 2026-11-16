//! vimrc ファイル (`~/.config/curl_tui/vimrc`): Vim の `imap jj <Esc>` のような
//! map 系コマンドで、押したキー列を別のキー列に置き換える。
//!
//! キーマップ ([`crate::keymap`]) が「キー → コマンド」なのに対し、こちらは Vim と同じく
//! 「キー → キー」の置き換え。置き換えた後のキーは普通に押したのと同じように処理される。

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, anyhow};
use crossterm::event::{KeyCode, KeyModifiers};

use crate::keymap::KeyStroke;
use crate::{t, tf};

/// マッピングが有効になるモード (Vim の map モード)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapMode {
    /// NORMAL モード
    Normal,
    /// VISUAL モード
    Visual,
    /// d / c / y の後のモーション待ち
    OperatorPending,
    /// INSERT モード
    Insert,
    /// `:` コマンドライン (と、リクエスト名の入力欄)
    CmdLine,
}

impl MapMode {
    fn bit(self) -> u8 {
        match self {
            MapMode::Normal => 1,
            MapMode::Visual => 2,
            MapMode::OperatorPending => 4,
            MapMode::Insert => 8,
            MapMode::CmdLine => 16,
        }
    }
}

const N: u8 = 1;
const V: u8 = 2;
const O: u8 = 4;
const I: u8 = 8;
const C: u8 = 16;
const NVO: u8 = N | V | O;
const IC: u8 = I | C;

/// (コマンド名, モード, noremap か)。省略形も受け付ける。
const MAP_COMMANDS: &[(&[&str], u8, bool)] = &[
    (&["map"], NVO, false),
    (&["noremap", "no", "nor"], NVO, true),
    (&["nmap", "nm"], N, false),
    (&["nnoremap", "nn", "nno", "nnor"], N, true),
    (&["vmap", "vm", "xmap", "xm"], V, false),
    (
        &[
            "vnoremap", "vn", "vno", "vnor", "xnoremap", "xn", "xno", "xnor",
        ],
        V,
        true,
    ),
    (&["omap", "om"], O, false),
    (&["onoremap", "ono", "onor"], O, true),
    (&["imap", "im"], I, false),
    (&["inoremap", "ino", "inor"], I, true),
    (&["cmap", "cm"], C, false),
    (&["cnoremap", "cno", "cnor"], C, true),
    (&["map!"], IC, false),
    (&["noremap!", "no!", "nor!"], IC, true),
];

const UNMAP_COMMANDS: &[(&[&str], u8)] = &[
    (&["unmap", "unm"], NVO),
    (&["nunmap", "nun"], N),
    (&["vunmap", "vu", "xunmap", "xu"], V),
    (&["ounmap", "ou"], O),
    (&["iunmap", "iu"], I),
    (&["cunmap", "cu"], C),
    (&["unmap!", "unm!"], IC),
];

/// 1 つのマッピング。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapping {
    modes: u8,
    pub lhs: Vec<KeyStroke>,
    /// 空なら `<Nop>` (キーを押しても何もしない)
    pub rhs: Vec<KeyStroke>,
    /// true なら置き換えた後のキーには、もうマッピングを適用しない
    pub noremap: bool,
    /// `<nowait>`: もっと長いマッピングがあっても続きのキーを待たない
    pub nowait: bool,
}

impl Mapping {
    pub fn applies_to(&self, mode: MapMode) -> bool {
        self.modes & mode.bit() != 0
    }
}

/// vimrc ファイルの場所: `$CURL_TUI_VIMRC` → 設定ファイル (config.jsonc) と同じディレクトリの `vimrc`。
pub fn default_path() -> PathBuf {
    match std::env::var_os("CURL_TUI_VIMRC").filter(|v| !v.is_empty()) {
        Some(path) => PathBuf::from(path),
        None => crate::config::default_path().with_file_name("vimrc"),
    }
}

/// vimrc ファイルを読む。ファイルが無ければマッピングなし。
pub fn load(path: &Path) -> anyhow::Result<Vimrc> {
    if !path.exists() {
        return Ok(Vimrc::default());
    }
    let text = std::fs::read_to_string(path).with_context(|| {
        tf!(
            "cannot read {p}",
            "{p} を読み込めません",
            p = path.display()
        )
    })?;
    let lines: Vec<&str> = text.lines().collect();
    parse(&lines).map_err(|e| anyhow!("{}: {e}", path.display()))
}

/// エディタで開く前の準備。ファイルが無い (空の) ときだけ雛形を作る。
pub fn prepare(path: &Path) -> anyhow::Result<()> {
    match std::fs::read_to_string(path) {
        Ok(text) if !text.trim().is_empty() => return Ok(()),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(e).with_context(|| {
                tf!(
                    "cannot read {p}",
                    "{p} を読み込めません",
                    p = path.display()
                )
            });
        }
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
    std::fs::write(path, template()).with_context(|| {
        tf!(
            "cannot write to {p}",
            "{p} に書き込めません",
            p = path.display()
        )
    })
}

/// 新しく作る vimrc の内容 (現在の表示言語で書く)。すべてコメントなので何も変えない。
pub fn template() -> String {
    let lines: &[&str] = match crate::i18n::current() {
        crate::i18n::Lang::En => &[
            "\" curl_tui vimrc: Vim-style key mappings",
            "\" Changes take effect as soon as you save and quit the editor (open it with :vimrc).",
            "\" Lines starting with \" are comments. Remove the leading \" of an example to enable it.",
            "\"",
            "\" Examples:",
            "\"imap jj <Esc>",
            "\"let mapleader = ' '",
            "\"nnoremap <Leader>w :w<CR>",
            "\"nnoremap <Leader>s <C-r>",
            "\"nnoremap q <Nop>",
            "\"set timeoutlen=500",
            "\"",
            "\" ==== Reference ====",
            "\" The right-hand side is the keys to press instead, so anything you can type works,",
            "\" including : commands (:w<CR>) and keys bound in \"keymap\" of config.jsonc.",
            "\"   imap / inoremap      Insert mode",
            "\"   nmap / nnoremap      Normal mode",
            "\"   vmap / xmap / vnoremap / xnoremap   Visual mode",
            "\"   omap / onoremap      after an operator (d / c / y)",
            "\"   cmap / cnoremap      : command line and the rename prompt",
            "\"   map / noremap        Normal + Visual + operator",
            "\"   map! / noremap!      Insert + command line",
            "\"   *unmap {lhs}         remove a mapping written earlier (nunmap, iunmap, unmap!, ...)",
            "\"   {rhs} = <Nop>        make the key do nothing",
            "\" map applies mappings again to the result; noremap does not.",
            "\" <silent> <buffer> <unique> are accepted and ignored. <nowait> does not wait for longer mappings.",
            "\" Keys: <Esc> <CR> <Tab> <S-Tab> <BS> <Del> <Space> <Up> <F5> <C-x> <M-x> (<A-x>) <lt> <Bar> <Bslash>",
            "\"       <Leader> <LocalLeader> (default \\; set let mapleader = ' ' before the mappings)",
            "\" set timeoutlen=1000  how long (ms) to wait for the rest of a mapping. set notimeout waits forever",
            "\" Other commands (set number, autocmd, ...) are not supported and are reported as errors.",
        ],
        crate::i18n::Lang::Ja => &[
            "\" curl_tui の vimrc: Vim 形式のキーマッピング",
            "\" 保存してエディタを終了すると、すぐに反映されます (:vimrc で開けます)。",
            "\" \" で始まる行はコメントです。例の先頭の \" を外すと有効になります。",
            "\"",
            "\" 例:",
            "\"imap jj <Esc>",
            "\"let mapleader = ' '",
            "\"nnoremap <Leader>w :w<CR>",
            "\"nnoremap <Leader>s <C-r>",
            "\"nnoremap q <Nop>",
            "\"set timeoutlen=500",
            "\"",
            "\" ==== リファレンス ====",
            "\" 右辺は代わりに押すキー列なので、: コマンド (:w<CR>) や config.jsonc の \"keymap\" で",
            "\" 割り当てたキーも含め、手で打てるものは何でも書けます。",
            "\"   imap / inoremap      Insert モード",
            "\"   nmap / nnoremap      Normal モード",
            "\"   vmap / xmap / vnoremap / xnoremap   Visual モード",
            "\"   omap / onoremap      オペレータ (d / c / y) の後",
            "\"   cmap / cnoremap      : コマンドラインとリクエスト名の入力欄",
            "\"   map / noremap        Normal + Visual + オペレータの後",
            "\"   map! / noremap!      Insert + コマンドライン",
            "\"   *unmap {lhs}         前に書いたマッピングを取り消す (nunmap, iunmap, unmap! など)",
            "\"   {rhs} = <Nop>        そのキーで何もしない",
            "\" map は展開後のキーにもマッピングを適用し、noremap は適用しません。",
            "\" <silent> <buffer> <unique> は書けますが無視します。<nowait> は長いマッピングの続きを待ちません。",
            "\" キー: <Esc> <CR> <Tab> <S-Tab> <BS> <Del> <Space> <Up> <F5> <C-x> <M-x> (<A-x>) <lt> <Bar> <Bslash>",
            "\"       <Leader> <LocalLeader> (既定は \\。マッピングより前に let mapleader = ' ' で変更)",
            "\" set timeoutlen=1000  マッピングの続きのキーを待つ時間 (ミリ秒)。set notimeout で待ち続けます",
            "\" それ以外のコマンド (set number / autocmd など) には対応しておらず、エラーになります。",
        ],
    };
    lines.join("\n") + "\n"
}

/// vimrc の内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vimrc {
    pub maps: Vec<Mapping>,
    /// 続きのキーを待つ時間 (`timeoutlen`)。None なら待ち続ける (`set notimeout`)
    pub timeout: Option<Duration>,
}

impl Default for Vimrc {
    fn default() -> Self {
        Self {
            maps: Vec::new(),
            timeout: Some(DEFAULT_TIMEOUT),
        }
    }
}

const DEFAULT_TIMEOUT: Duration = Duration::from_millis(1000);

/// [`Vimrc::find`] の結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found<'a> {
    /// 与えたキー列より長く、それで始まるマッピングがあるか (続きを待つべきか)
    pub longer: bool,
    /// 与えたキー列の先頭部分に一致する、いちばん長いマッピング
    pub exact: Option<&'a Mapping>,
}

impl Vimrc {
    pub fn is_empty(&self) -> bool {
        self.maps.is_empty()
    }

    /// `mode` で `keys` に当てはまるマッピングを探す。
    pub fn find(&self, mode: MapMode, keys: &[KeyStroke]) -> Found<'_> {
        let mut found = Found {
            longer: false,
            exact: None,
        };
        for m in self.maps.iter().filter(|m| m.applies_to(mode)) {
            if m.lhs.len() > keys.len() && m.lhs.starts_with(keys) {
                found.longer = true;
            } else if keys.starts_with(&m.lhs)
                && found.exact.is_none_or(|e| e.lhs.len() < m.lhs.len())
            {
                found.exact = Some(m);
            }
        }
        found
    }

    /// `modes` のうち `lhs` のマッピングを取り除く。取り除いたら true。
    fn remove(&mut self, modes: u8, lhs: &[KeyStroke]) -> bool {
        let mut removed = false;
        for m in self.maps.iter_mut().filter(|m| m.lhs == lhs) {
            if m.modes & modes != 0 {
                m.modes &= !modes;
                removed = true;
            }
        }
        self.maps.retain(|m| m.modes != 0);
        removed
    }
}

/// `<Leader>` などの解釈に使う状態。
struct Leaders {
    leader: Vec<KeyStroke>,
    local: Vec<KeyStroke>,
}

/// vimrc の各行を解釈する。
pub fn parse<S: AsRef<str>>(lines: &[S]) -> Result<Vimrc, String> {
    let mut vimrc = Vimrc::default();
    let backslash = vec![char_key('\\')];
    let mut leaders = Leaders {
        leader: backslash.clone(),
        local: backslash,
    };
    for (i, line) in lines.iter().enumerate() {
        let n = i + 1;
        parse_line(line.as_ref(), &mut vimrc, &mut leaders).map_err(|e| {
            tf!(
                "line {n} ({line}): {e}",
                "{n} 行目 ({line}): {e}",
                line = line.as_ref().trim()
            )
        })?;
    }
    Ok(vimrc)
}

fn parse_line(line: &str, vimrc: &mut Vimrc, leaders: &mut Leaders) -> Result<(), String> {
    let line = line.trim_start().trim_start_matches(':').trim_start();
    if line.is_empty() || line.starts_with('"') {
        return Ok(());
    }
    let (command, rest) = split_word(line);
    if command == "let" {
        return parse_let(rest, leaders);
    }
    if command == "set" || command == "se" {
        return parse_set(rest, vimrc);
    }
    if let Some((_, modes, noremap)) = MAP_COMMANDS.iter().find(|(n, ..)| n.contains(&command)) {
        return parse_map(rest, *modes, *noremap, vimrc, leaders);
    }
    if let Some((_, modes)) = UNMAP_COMMANDS.iter().find(|(n, _)| n.contains(&command)) {
        let (rest, _) = map_arguments(rest)?;
        let (lhs, extra) = split_word(rest);
        if lhs.is_empty() || !extra.is_empty() {
            return Err(t!(
                "write it as `unmap {lhs}`",
                "`unmap {lhs}` の形で書いてください"
            )
            .to_string());
        }
        let lhs = parse_notation(lhs, leaders)?;
        if !vimrc.remove(*modes, &lhs) {
            return Err(t!(
                "no such mapping (to disable a built-in key, map it to <Nop>, e.g. `nnoremap q <Nop>`)",
                "そのマッピングはありません (既定のキーを無効にするには `nnoremap q <Nop>` のように <Nop> に割り当ててください)"
            )
            .to_string());
        }
        return Ok(());
    }
    Err(tf!(
        "unsupported command \"{command}\" (available: map / noremap / nmap / imap / vmap / xmap / omap / cmap / map! and their noremap and unmap versions, let mapleader, set timeoutlen)",
        "対応していないコマンド \"{command}\" (使えるもの: map / noremap / nmap / imap / vmap / xmap / omap / cmap / map! と、それぞれの noremap 版・unmap 版、let mapleader、set timeoutlen)"
    ))
}

/// 先頭の空白区切りの単語と、残り (先頭の空白は除く)。
fn split_word(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    let end = s.find(char::is_whitespace).unwrap_or(s.len());
    (&s[..end], s[end..].trim_start())
}

/// `<silent>` などの引数を読み飛ばす。戻り値は (残り, <nowait> があったか)。
fn map_arguments(mut rest: &str) -> Result<(&str, bool), String> {
    const IGNORED: &[&str] = &["<silent>", "<buffer>", "<special>", "<script>", "<unique>"];
    let mut nowait = false;
    loop {
        rest = rest.trim_start();
        if !rest.starts_with('<') {
            break;
        }
        let Some(arg) = rest.get(..rest.find('>').map_or(0, |i| i + 1)) else {
            break;
        };
        let lower = arg.to_ascii_lowercase();
        if lower == "<nowait>" {
            nowait = true;
        } else if lower == "<expr>" {
            return Err(t!("<expr> is not supported", "<expr> には対応していません").to_string());
        } else if !IGNORED.contains(&lower.as_str()) {
            break;
        }
        rest = &rest[arg.len()..];
    }
    Ok((rest, nowait))
}

fn parse_map(
    rest: &str,
    modes: u8,
    noremap: bool,
    vimrc: &mut Vimrc,
    leaders: &Leaders,
) -> Result<(), String> {
    let (rest, nowait) = map_arguments(rest)?;
    let (lhs, rhs) = split_word(rest);
    let rhs = rhs.trim_end();
    if lhs.is_empty() || rhs.is_empty() {
        return Err(t!(
            "write it as `map {lhs} {rhs}` (use <Nop> to make a key do nothing)",
            "`map {lhs} {rhs}` の形で書いてください (何もしないキーにするには <Nop>)"
        )
        .to_string());
    }
    let lhs = parse_notation(lhs, leaders)?;
    let rhs = if rhs.eq_ignore_ascii_case("<nop>") {
        Vec::new()
    } else {
        parse_notation(rhs, leaders)?
    };
    // 同じモード・同じキーの古いマッピングは置き換える
    vimrc.remove(modes, &lhs);
    vimrc.maps.push(Mapping {
        modes,
        lhs,
        rhs,
        noremap,
        nowait,
    });
    Ok(())
}

fn parse_let(rest: &str, leaders: &mut Leaders) -> Result<(), String> {
    let (name, value) = rest.split_once('=').unwrap_or((rest, ""));
    let target = match name.trim().trim_start_matches("g:") {
        "mapleader" => &mut leaders.leader,
        "maplocalleader" => &mut leaders.local,
        other => {
            return Err(tf!(
                "unsupported variable \"{other}\" (only mapleader / maplocalleader)",
                "対応していない変数 \"{other}\" (mapleader / maplocalleader だけです)"
            ));
        }
    };
    let keys = parse_string(value.trim())?;
    if keys.is_empty() {
        return Err(t!("the leader is empty", "リーダーキーが空です").to_string());
    }
    *target = keys;
    Ok(())
}

/// Vim の文字列リテラル (`' '` / `","` / `"\<Space>"`) をキー列にする。
fn parse_string(s: &str) -> Result<Vec<KeyStroke>, String> {
    let invalid = || {
        tf!(
            "write the value as a quoted string, e.g. ' ' or \",\" (got `{s}`)",
            "値は ' ' や \",\" のように引用符で囲んでください (`{s}`)"
        )
    };
    if let Some(body) = s.strip_prefix('\'').and_then(|b| b.strip_suffix('\'')) {
        return Ok(body.replace("''", "'").chars().map(char_key).collect());
    }
    let body = s
        .strip_prefix('"')
        .and_then(|b| b.strip_suffix('"'))
        .ok_or_else(invalid)?;
    let chars: Vec<char> = body.chars().collect();
    let mut keys = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            if chars[i + 1] == '<'
                && let Some((special, len)) = special_at(&chars[i + 1..], None)?
            {
                keys.extend(special);
                i += 1 + len;
                continue;
            }
            keys.push(char_key(chars[i + 1]));
            i += 2;
        } else {
            keys.push(char_key(chars[i]));
            i += 1;
        }
    }
    Ok(keys)
}

fn char_key(c: char) -> KeyStroke {
    KeyStroke::new(KeyCode::Char(c), KeyModifiers::NONE)
}

/// `jj` / `<Esc>` / `<C-w>h` / `<Leader>w` のような Vim のキー表記をキー列にする。
fn parse_notation(s: &str, leaders: &Leaders) -> Result<Vec<KeyStroke>, String> {
    let chars: Vec<char> = s.chars().collect();
    let mut keys = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<'
            && let Some((special, len)) = special_at(&chars[i..], Some(leaders))?
        {
            keys.extend(special);
            i += len;
            continue;
        }
        keys.push(char_key(chars[i]));
        i += 1;
    }
    Ok(keys)
}

/// `chars` の先頭 (`<`) から始まる `<...>` を読む。特殊キーでなければ None
/// (そのときは `<` をそのままの文字として扱う)。戻り値は (キー列, 読んだ文字数)。
fn special_at(
    chars: &[char],
    leaders: Option<&Leaders>,
) -> Result<Option<(Vec<KeyStroke>, usize)>, String> {
    let Some(mut close) = chars.iter().skip(2).position(|c| *c == '>').map(|p| p + 2) else {
        return Ok(None);
    };
    // `<C->>` のように > 自身に修飾キーを付けたもの
    if chars.get(close + 1) == Some(&'>')
        && special_key(&chars[1..close].iter().collect::<String>()).is_none()
    {
        close += 1;
    }
    let name: String = chars[1..close].iter().collect();
    let lower = name.to_ascii_lowercase();
    match (lower.as_str(), leaders) {
        ("leader", Some(l)) => return Ok(Some((l.leader.clone(), close + 1))),
        ("localleader", Some(l)) => return Ok(Some((l.local.clone(), close + 1))),
        ("nop", _) => {
            return Err(t!(
                "<Nop> must be the whole right-hand side",
                "<Nop> は右辺にそれだけを書いてください"
            )
            .to_string());
        }
        _ => {}
    }
    Ok(special_key(&name).map(|k| (vec![k], close + 1)))
}

/// `<...>` の中身 (`Esc` / `C-w` / `S-Tab` / `M-x` / `F5`) を 1 打鍵にする。
fn special_key(name: &str) -> Option<KeyStroke> {
    let mut mods = KeyModifiers::NONE;
    let mut rest = name;
    // 修飾キー: C- (Ctrl) S- (Shift) M- / A- (Alt) D- (Cmd)
    loop {
        let mut it = rest.chars();
        let (Some(m), Some('-')) = (it.next(), it.next()) else {
            break;
        };
        if it.as_str().is_empty() {
            break;
        }
        mods |= match m.to_ascii_lowercase() {
            'c' => KeyModifiers::CONTROL,
            's' => KeyModifiers::SHIFT,
            'm' | 'a' => KeyModifiers::ALT,
            'd' => KeyModifiers::SUPER,
            _ => break,
        };
        rest = &rest[2..];
    }
    let lower = rest.to_ascii_lowercase();
    let code = match lower.as_str() {
        "esc" => KeyCode::Esc,
        "cr" | "enter" | "return" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "bs" | "backspace" => KeyCode::Backspace,
        "del" | "delete" => KeyCode::Delete,
        "space" => KeyCode::Char(' '),
        "bar" => KeyCode::Char('|'),
        "bslash" => KeyCode::Char('\\'),
        "lt" => KeyCode::Char('<'),
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "insert" | "ins" => KeyCode::Insert,
        _ => {
            if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok())
                && (1..=12).contains(&n)
            {
                KeyCode::F(n)
            } else {
                let mut it = rest.chars();
                match (it.next(), it.next()) {
                    // `<a>` は特殊キーではない (修飾キーがあるときだけ 1 文字を受け付ける)
                    (Some(c), None) if !mods.is_empty() => KeyCode::Char(c),
                    _ => return None,
                }
            }
        }
    };
    // 文字キーの Shift は大文字で表す (KeyStroke::from_event と同じ)。Ctrl+文字は大小を区別しない
    let code = match code {
        KeyCode::Char(c) => {
            let shift = mods.contains(KeyModifiers::SHIFT);
            mods -= KeyModifiers::SHIFT;
            if mods.contains(KeyModifiers::CONTROL) {
                KeyCode::Char(c.to_ascii_lowercase())
            } else if shift {
                KeyCode::Char(c.to_ascii_uppercase())
            } else {
                KeyCode::Char(c)
            }
        }
        code => code,
    };
    Some(KeyStroke::new(code, mods))
}

fn parse_set(rest: &str, vimrc: &mut Vimrc) -> Result<(), String> {
    for option in rest.split_whitespace() {
        let (name, value) = option.split_once('=').unwrap_or((option, ""));
        match (name, value) {
            ("timeoutlen" | "tm", ms) => {
                let ms: u64 = ms.parse().map_err(|_| {
                    tf!(
                        "timeoutlen needs a number of milliseconds (got `{ms}`)",
                        "timeoutlen にはミリ秒の数値を指定してください (`{ms}`)"
                    )
                })?;
                vimrc.timeout = Some(Duration::from_millis(ms));
            }
            ("timeout" | "to", "") => {
                vimrc.timeout.get_or_insert(DEFAULT_TIMEOUT);
            }
            ("notimeout" | "noto", "") => vimrc.timeout = None,
            _ => {
                return Err(tf!(
                    "unsupported option \"{option}\" (only timeoutlen / timeout / notimeout)",
                    "対応していないオプション \"{option}\" (timeoutlen / timeout / notimeout だけです)"
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::parse_keys;

    fn keys(s: &str) -> Vec<KeyStroke> {
        parse_keys(s).unwrap()
    }

    fn one(line: &str) -> Mapping {
        let vimrc = parse(&[line]).unwrap();
        assert_eq!(vimrc.maps.len(), 1, "{line}");
        vimrc.maps[0].clone()
    }

    #[test]
    fn parses_map_commands() {
        let m = one("imap jj <Esc>");
        assert_eq!((&m.lhs, &m.rhs), (&keys("j j"), &keys("escape")));
        assert!(!m.noremap);
        assert!(m.applies_to(MapMode::Insert) && !m.applies_to(MapMode::Normal));

        let m = one(":nnoremap <silent> <C-g> :w<CR>");
        assert_eq!(m.lhs, keys("ctrl-g"));
        assert_eq!(m.rhs, keys(": w enter"));
        assert!(m.noremap && m.applies_to(MapMode::Normal));

        let m = one("map! <M-a> <S-Tab>x<lt>X<Bar>");
        assert_eq!(m.lhs, keys("alt-a"));
        assert_eq!(m.rhs, keys("shift-tab x < shift-x |"));
        assert!(m.applies_to(MapMode::Insert) && m.applies_to(MapMode::CmdLine));

        // 右辺の空白はそのままキーになる。<a> は特殊キーではないので文字のまま
        let m = one("noremap <F5> a b<a>");
        assert_eq!(m.rhs, keys("a space b < a >"));
        assert!(m.applies_to(MapMode::Visual) && m.applies_to(MapMode::OperatorPending));
        assert!(!m.applies_to(MapMode::Insert));

        assert_eq!(one("nnoremap <C-S-a> <C-W>").lhs, keys("ctrl-a"));
        assert_eq!(one("nnoremap <C-W> x").lhs, keys("ctrl-w"));
        assert_eq!(one("xn <nowait> <C->> x").lhs, keys("ctrl->"));
        assert!(one("xn <nowait> x y").nowait);
        assert!(one("nnoremap q <Nop>").rhs.is_empty());
    }

    #[test]
    fn leader_is_expanded_at_definition_time() {
        let vimrc = parse(&[
            "nmap <Leader>a x",
            "let mapleader = ' '",
            "nmap <leader>w :w<CR>",
            r#"let g:mapleader = "\<Space>,""#,
            "nmap <Leader>q :q<CR>",
            "let maplocalleader=','",
            "nmap <LocalLeader>x y",
        ])
        .unwrap();
        let lhs: Vec<_> = vimrc.maps.iter().map(|m| m.lhs.clone()).collect();
        assert_eq!(
            lhs,
            [
                keys("\\ a"),
                keys("space w"),
                keys("space , q"),
                keys(", x")
            ]
        );
    }

    #[test]
    fn later_mappings_replace_and_unmap_removes() {
        let vimrc = parse(&["map x a", "nmap x b", "vunmap x"]).unwrap();
        let normal = vimrc.find(MapMode::Normal, &keys("x")).exact.unwrap();
        assert_eq!(normal.rhs, keys("b"));
        let op = vimrc
            .find(MapMode::OperatorPending, &keys("x"))
            .exact
            .unwrap();
        assert_eq!(op.rhs, keys("a"));
        assert!(vimrc.find(MapMode::Visual, &keys("x")).exact.is_none());
        assert!(parse(&["nunmap x"]).is_err());
    }

    #[test]
    fn find_reports_longer_and_longest_prefix() {
        let vimrc = parse(&["imap j a", "imap jj <Esc>", "imap jjj b"]).unwrap();
        let f = vimrc.find(MapMode::Insert, &keys("j j"));
        assert!(f.longer);
        assert_eq!(f.exact.unwrap().rhs, keys("escape"));
        let f = vimrc.find(MapMode::Insert, &keys("j j x"));
        assert!(!f.longer);
        assert_eq!(f.exact.unwrap().lhs, keys("j j"));
        assert_eq!(vimrc.find(MapMode::Normal, &keys("j")).exact, None);
    }

    #[test]
    fn template_examples_work_when_uncommented() {
        let template = template();
        assert!(
            parse(&template.lines().collect::<Vec<_>>())
                .unwrap()
                .is_empty()
        );
        let examples: Vec<&str> = template
            .lines()
            .filter_map(|l| l.strip_prefix('"'))
            .filter(|l| !l.is_empty() && !l.starts_with(' '))
            .collect();
        assert_eq!(examples.len(), 6);
        assert_eq!(parse(&examples).unwrap().maps.len(), 4);
    }

    #[test]
    fn load_and_prepare() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/vimrc");
        assert!(load(&path).unwrap().is_empty());
        prepare(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), template());
        // 既にあるファイルには触らない
        std::fs::write(&path, "imap jj <Esc>\n").unwrap();
        prepare(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "imap jj <Esc>\n");
        assert_eq!(load(&path).unwrap().maps.len(), 1);
        std::fs::write(&path, "imap jj <Esc>\nnmap\n").unwrap();
        let err = load(&path).unwrap_err().to_string();
        assert!(err.contains("vimrc") && err.contains("line 2"), "{err}");
    }

    #[test]
    fn comments_options_and_errors() {
        let vimrc = parse(&["\" comment", "", "set timeoutlen=300", "  "]).unwrap();
        assert!(vimrc.is_empty());
        assert_eq!(vimrc.timeout, Some(Duration::from_millis(300)));
        assert_eq!(parse(&["set notimeout"]).unwrap().timeout, None);
        assert_eq!(
            parse(&["set noto to"]).unwrap().timeout,
            Some(DEFAULT_TIMEOUT)
        );

        for bad in [
            "imap jj",
            "imap",
            "nnoremap <expr> x y",
            "nmap x <Nop>y",
            "set number",
            "set tm=abc",
            "let foo = 1",
            "let mapleader = x",
            "let mapleader = ''",
            "autocmd BufRead * echo",
            "iunmap a b",
        ] {
            assert!(parse(&[bad]).is_err(), "{bad}");
        }
        let err = parse(&["nmap a b", "foo"]).unwrap_err();
        assert!(err.contains('2') && err.contains("foo"), "{err}");
    }
}
