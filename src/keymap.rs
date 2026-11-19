//! キーバインド: コマンド名 / コンテキスト / 既定のキーマップ。
//!
//! Zed のキーマップと同じ考え方で、「どの環境 (コンテキスト) で」「どのキーを押すと」
//! 「どのコマンドが動くか」を表にしている。既定の表 ([`DEFAULTS`]) の後ろに
//! 設定ファイルの内容を足し、後に書かれたものを優先する。

use std::fmt;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{t, tf};

macro_rules! commands {
    ($($variant:ident => $name:literal, $en:literal, $ja:literal;)*) => {
        /// キーに割り当てられる操作。
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Command { $($variant,)* }

        impl Command {
            pub const ALL: &'static [Command] = &[$(Command::$variant,)*];

            /// 設定ファイルに書く名前。
            pub fn name(self) -> &'static str {
                match self { $(Command::$variant => $name,)* }
            }

            /// 現在の表示言語での説明。
            pub fn description(self) -> &'static str {
                match self { $(Command::$variant => $crate::t!($en, $ja),)* }
            }

            pub fn from_name(name: &str) -> Option<Self> {
                match name { $($name => Some(Command::$variant),)* _ => None }
            }
        }
    };
}

commands! {
    AppQuit => "app::Quit", "Quit", "終了";
    AppSend => "app::Send", "Send the request", "リクエストを送信";
    AppSave => "app::Save", "Save the collection", "コレクションを保存";
    AppSaveAndQuit => "app::SaveAndQuit", "Save and quit", "保存して終了";
    AppShowHelp => "app::ShowHelp", "Show help", "ヘルプを表示";
    AppOpenCommandLine => "app::OpenCommandLine", "Open the : command line", ": コマンドラインを開く";
    AppOpenConfig => "app::OpenConfig", "Open this config file in your editor (same as :config)", "この設定ファイルをエディタで開く (:config と同じ)";
    AppOpenVimrc => "app::OpenVimrc", "Open the vimrc file (Vim-style mappings) in your editor (same as :vimrc)", "vimrc ファイル (Vim 形式のマッピング) をエディタで開く (:vimrc と同じ)";

    PaneFocusLeft => "pane::FocusLeft", "Focus the pane to the left (Collection)", "左のペイン (Collection) へ";
    PaneFocusDown => "pane::FocusDown", "Focus the pane below", "下のペインへ";
    PaneFocusUp => "pane::FocusUp", "Focus the pane above", "上のペインへ";
    PaneFocusRight => "pane::FocusRight", "Focus the pane to the right (the last used of URL / Request / Response)", "右のペイン (直前にいた URL / Request / Response) へ";
    PaneFocusNext => "pane::FocusNext", "Focus the next pane", "次のペインへ";
    PaneFocusPrev => "pane::FocusPrev", "Focus the previous pane", "前のペインへ";
    PaneFocusCollection => "pane::FocusCollection", "Focus the Collection pane", "Collection ペインへ";
    PaneFocusUrl => "pane::FocusUrl", "Focus the URL pane", "URL ペインへ";
    PaneFocusRequest => "pane::FocusRequest", "Focus the Request pane", "Request ペインへ";
    PaneFocusResponse => "pane::FocusResponse", "Focus the Response pane", "Response ペインへ";
    PaneToggleZoom => "pane::ToggleZoom", "Zoom the focused pane to full screen / restore", "選択中のペインを全画面表示 / 元に戻す";

    RequestNextMethod => "request::NextMethod", "Next HTTP method", "HTTP メソッドを次へ";
    RequestPrevMethod => "request::PrevMethod", "Previous HTTP method", "HTTP メソッドを前へ";

    TabNext => "tab::Next", "Next tab", "次のタブ";
    TabPrev => "tab::Prev", "Previous tab", "前のタブ";
    TabSelect1 => "tab::Select1", "1st tab", "1 番目のタブ";
    TabSelect2 => "tab::Select2", "2nd tab", "2 番目のタブ";
    TabSelect3 => "tab::Select3", "3rd tab", "3 番目のタブ";
    TabSelect4 => "tab::Select4", "4th tab", "4 番目のタブ";

    CollectionSelectNext => "collection::SelectNext", "Select the request below", "下のリクエストを選択";
    CollectionSelectPrev => "collection::SelectPrev", "Select the request above", "上のリクエストを選択";
    CollectionSelectFirst => "collection::SelectFirst", "Select the first request", "先頭のリクエストを選択";
    CollectionSelectLast => "collection::SelectLast", "Select the last request", "末尾のリクエストを選択";
    CollectionNewBelow => "collection::NewBelow", "Create a new request below", "下に新しいリクエストを作成";
    CollectionNewAbove => "collection::NewAbove", "Create a new request above", "上に新しいリクエストを作成";
    CollectionYank => "collection::Yank", "Yank (copy) the request", "リクエストをヤンク (コピー)";
    CollectionDelete => "collection::Delete", "Delete the request (also yanks it)", "リクエストを削除 (ヤンクもする)";
    CollectionPasteAfter => "collection::PasteAfter", "Paste the yanked request below", "ヤンクしたリクエストを下に貼り付け";
    CollectionPasteBefore => "collection::PasteBefore", "Paste the yanked request above", "ヤンクしたリクエストを上に貼り付け";
    CollectionRename => "collection::Rename", "Rename the request", "リクエストの名前を変更";

    ResponseScrollDown => "response::ScrollDown", "Scroll down one line", "1 行下へスクロール";
    ResponseScrollUp => "response::ScrollUp", "Scroll up one line", "1 行上へスクロール";
    ResponseHalfPageDown => "response::HalfPageDown", "Half a page down", "半ページ下へ";
    ResponseHalfPageUp => "response::HalfPageUp", "Half a page up", "半ページ上へ";
    ResponsePageDown => "response::PageDown", "One page down", "1 ページ下へ";
    ResponsePageUp => "response::PageUp", "One page up", "1 ページ上へ";
    ResponseScrollToTop => "response::ScrollToTop", "Go to the top", "先頭へ";
    ResponseScrollToBottom => "response::ScrollToBottom", "Go to the bottom", "末尾へ";

    PreviewZoomIn => "preview::ZoomIn", "Zoom in the HTML preview", "HTML プレビューを拡大";
    PreviewZoomOut => "preview::ZoomOut", "Zoom out the HTML preview", "HTML プレビューを縮小";
    PreviewZoomReset => "preview::ZoomReset", "Reset the HTML preview zoom to 100%", "HTML プレビューの倍率を 100% に戻す";

    VimMoveLeft => "vim::MoveLeft", "(motion) Left", "(モーション) 左へ";
    VimMoveRight => "vim::MoveRight", "(motion) Right", "(モーション) 右へ";
    VimMoveUp => "vim::MoveUp", "(motion) Up", "(モーション) 上へ";
    VimMoveDown => "vim::MoveDown", "(motion) Down", "(モーション) 下へ";
    VimMoveWordForward => "vim::MoveWordForward", "(motion) Start of the next word", "(モーション) 次の単語の先頭へ";
    VimMoveWordBackward => "vim::MoveWordBackward", "(motion) Start of the previous word", "(モーション) 前の単語の先頭へ";
    VimMoveWordEnd => "vim::MoveWordEnd", "(motion) End of the word", "(モーション) 単語の末尾へ";
    VimMoveToLineStart => "vim::MoveToLineStart", "(motion) Start of the line", "(モーション) 行頭へ";
    VimMoveToFirstNonBlank => "vim::MoveToFirstNonBlank", "(motion) First non-blank character of the line", "(モーション) 行頭の空白でない文字へ";
    VimMoveToLineEnd => "vim::MoveToLineEnd", "(motion) End of the line", "(モーション) 行末へ";
    VimMoveToTop => "vim::MoveToTop", "(motion) First line", "(モーション) 先頭行へ";
    VimMoveToBottom => "vim::MoveToBottom", "(motion) Last line", "(モーション) 最終行へ";
    VimInsert => "vim::Insert", "Enter Insert mode at the cursor", "カーソル位置で Insert モードへ";
    VimAppend => "vim::Append", "Enter Insert mode after the cursor", "カーソルの後ろで Insert モードへ";
    VimInsertAtLineStart => "vim::InsertAtLineStart", "Enter Insert mode at the start of the line", "行頭で Insert モードへ";
    VimAppendAtLineEnd => "vim::AppendAtLineEnd", "Enter Insert mode at the end of the line", "行末で Insert モードへ";
    VimOpenLineBelow => "vim::OpenLineBelow", "Open a line below and enter Insert mode", "下に行を追加して Insert モードへ";
    VimOpenLineAbove => "vim::OpenLineAbove", "Open a line above and enter Insert mode", "上に行を追加して Insert モードへ";
    VimDeleteChar => "vim::DeleteChar", "Delete the character under the cursor", "カーソル位置の 1 文字を削除";
    VimDeleteCharBefore => "vim::DeleteCharBefore", "Delete the character before the cursor", "カーソルの前の 1 文字を削除";
    VimDeleteToLineEnd => "vim::DeleteToLineEnd", "Delete to the end of the line", "行末まで削除";
    VimChangeToLineEnd => "vim::ChangeToLineEnd", "Delete to the end of the line and enter Insert mode", "行末まで削除して Insert モードへ";
    VimSubstituteChar => "vim::SubstituteChar", "Delete one character and enter Insert mode", "1 文字削除して Insert モードへ";
    VimSubstituteLine => "vim::SubstituteLine", "Clear the line and enter Insert mode", "行を消して Insert モードへ";
    VimYankLine => "vim::YankLine", "Yank the line", "行をヤンク";
    VimPasteAfter => "vim::PasteAfter", "Paste the yanked text after the cursor", "ヤンクした内容を後ろに貼り付け";
    VimPasteBefore => "vim::PasteBefore", "Paste the yanked text before the cursor", "ヤンクした内容を前に貼り付け";
    VimUndo => "vim::Undo", "Undo", "元に戻す";
    VimReplaceChar => "vim::ReplaceChar", "Replace one character with the next key pressed", "次に押した文字で 1 文字置換";
    VimDelete => "vim::Delete", "(operator) Delete. Follow with a motion, or the same key for the whole line", "(オペレータ) 削除。続けてモーション、同じキーで行ごと";
    VimChange => "vim::Change", "(operator) Delete and enter Insert mode", "(オペレータ) 削除して Insert モードへ";
    VimYank => "vim::Yank", "(operator) Yank and copy to the clipboard. In Visual mode, applies to the selection", "(オペレータ) ヤンクしてクリップボードにコピー。Visual モードでは選択範囲が対象";
    VimVisual => "vim::Visual", "Start / end Visual mode (character-wise selection)", "Visual モード (文字単位の選択) を開始 / 終了";
    VimVisualLine => "vim::VisualLine", "Start / end Visual mode (line-wise selection)", "Visual モード (行単位の選択) を開始 / 終了";
    VimVisualSwapEnds => "vim::VisualSwapEnds", "(Visual) Move the cursor to the other end of the selection", "(Visual) 選択範囲の反対側の端へカーソルを移す";
    VimNormalMode => "vim::NormalMode", "Leave Insert / Visual mode and return to Normal mode", "Insert / Visual モードを抜けて Normal モードへ";

    EditorBackspace => "editor::Backspace", "Delete the previous character", "前の 1 文字を削除";
    EditorDelete => "editor::Delete", "Delete the character under the cursor", "カーソル位置の 1 文字を削除";
    EditorDeleteWordBefore => "editor::DeleteWordBefore", "Delete the previous word", "直前の単語を削除";
    EditorMoveLeft => "editor::MoveLeft", "Left", "左へ";
    EditorMoveRight => "editor::MoveRight", "Right", "右へ";
    EditorMoveUp => "editor::MoveUp", "Up", "上へ";
    EditorMoveDown => "editor::MoveDown", "Down", "下へ";
    EditorMoveToLineStart => "editor::MoveToLineStart", "Start of the line", "行頭へ";
    EditorMoveToLineEnd => "editor::MoveToLineEnd", "End of the line", "行末へ";
    EditorKillToLineEnd => "editor::KillToLineEnd", "Delete to the end of the line", "行末まで削除";
    EditorClear => "editor::Clear", "Clear all input", "入力内容をすべて消す";
    EditorNewline => "editor::Newline", "Insert a newline", "改行";
    EditorOpenExternal => "editor::OpenExternal", "Open the focused text field (URL / Params / Headers / Body) in $EDITOR and apply it when the editor closes (same as :edit)", "選択中のテキスト欄 (URL / Params / Headers / Body) を $EDITOR で開き、閉じたら反映する (:edit と同じ)";

    CommandCancel => "command::Cancel", "Close the command line", "コマンドラインを閉じる";
    CommandExecute => "command::Execute", "Run the command", "コマンドを実行";
    RenameCancel => "rename::Cancel", "Cancel renaming", "名前の変更をやめる";
    RenameConfirm => "rename::Confirm", "Confirm the name", "名前を確定";
}

/// キーバインドが有効になる環境。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ctx {
    Normal,
    Insert,
    Visual,
    Command,
    Rename,
    Help,
    TextInput,
    Collection,
    Url,
    Request,
    Response,
    Preview,
    Editor,
    TextView,
    Operator,
}

impl Ctx {
    pub const ALL: [Ctx; 15] = [
        Ctx::Normal,
        Ctx::Insert,
        Ctx::Visual,
        Ctx::Command,
        Ctx::Rename,
        Ctx::Help,
        Ctx::TextInput,
        Ctx::Collection,
        Ctx::Url,
        Ctx::Request,
        Ctx::Response,
        Ctx::Preview,
        Ctx::Editor,
        Ctx::TextView,
        Ctx::Operator,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Ctx::Normal => "Normal",
            Ctx::Insert => "Insert",
            Ctx::Visual => "Visual",
            Ctx::Command => "Command",
            Ctx::Rename => "Rename",
            Ctx::Help => "Help",
            Ctx::TextInput => "TextInput",
            Ctx::Collection => "Collection",
            Ctx::Url => "Url",
            Ctx::Request => "Request",
            Ctx::Response => "Response",
            Ctx::Preview => "Preview",
            Ctx::Editor => "Editor",
            Ctx::TextView => "TextView",
            Ctx::Operator => "Operator",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Ctx::Normal => t!("NORMAL mode", "NORMAL モード"),
            Ctx::Insert => t!(
                "INSERT mode (editing a text field)",
                "INSERT モード (テキスト欄を編集中)"
            ),
            Ctx::Visual => t!(
                "VISUAL mode (selecting text; the text cannot be changed)",
                "VISUAL モード (文字を選択中。文字列は変更できない)"
            ),
            Ctx::Command => t!("Typing in the : command line", ": コマンドラインに入力中"),
            Ctx::Rename => t!("Renaming a request", "リクエスト名を変更中"),
            Ctx::Help => t!(
                "Help is shown (unbound keys close it)",
                "ヘルプを表示中 (割り当てのないキーはヘルプを閉じる)"
            ),
            Ctx::TextInput => t!(
                "Typing text (any of Insert / Command / Rename)",
                "文字を入力中 (Insert / Command / Rename のどれか)"
            ),
            Ctx::Collection => t!(
                "Collection pane is focused (in Normal / Insert / Visual)",
                "Collection ペインを選択中 (Normal / Insert / Visual のとき)"
            ),
            Ctx::Url => t!(
                "URL pane is focused (same as above)",
                "URL ペインを選択中 (同上)"
            ),
            Ctx::Request => t!(
                "Request pane is focused (same as above)",
                "Request ペインを選択中 (同上)"
            ),
            Ctx::Response => t!(
                "Response pane is focused (same as above)",
                "Response ペインを選択中 (同上)"
            ),
            Ctx::Preview => t!(
                "Preview tab (HTML preview) is shown in the Response pane",
                "Response ペインで Preview タブ (HTML プレビュー) を表示中"
            ),
            Ctx::Editor => t!(
                "An editable text field (URL / Params / Headers / Body) is focused",
                "編集できるテキスト欄 (URL / Params / Headers / Body) を選択中"
            ),
            Ctx::TextView => t!(
                "Read-only text (cURL tab / Body and Headers tabs of Response) is focused",
                "読み取り専用のテキスト (cURL タブ / Response の Body・Headers タブ) を選択中"
            ),
            Ctx::Operator => t!(
                "Waiting for a motion after an operator such as d / c / y",
                "d / c / y などオペレータの後のモーション待ち"
            ),
        }
    }

    fn bit(self) -> u16 {
        1 << Self::ALL.iter().position(|c| *c == self).unwrap_or(0)
    }
}

/// いま有効なコンテキストの集合。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContextSet(u16);

impl ContextSet {
    pub fn with(mut self, ctx: Ctx) -> Self {
        self.0 |= ctx.bit();
        self
    }

    pub fn contains(self, ctx: Ctx) -> bool {
        self.0 & ctx.bit() != 0
    }
}

/// `"Normal && !Collection"` のようなコンテキストの条件式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextExpr {
    /// 条件なし (どこでも有効)
    Always,
    Is(Ctx),
    Not(Box<ContextExpr>),
    And(Box<ContextExpr>, Box<ContextExpr>),
    Or(Box<ContextExpr>, Box<ContextExpr>),
}

impl ContextExpr {
    pub fn parse(s: &str) -> Result<Self, String> {
        if s.trim().is_empty() {
            return Ok(ContextExpr::Always);
        }
        let tokens = tokenize(s)?;
        let mut pos = 0;
        let expr = parse_or(&tokens, &mut pos)?;
        if pos != tokens.len() {
            return Err(tf!(
                "Invalid context expression \"{s}\"",
                "コンテキスト \"{s}\" の書き方が正しくありません"
            ));
        }
        Ok(expr)
    }

    pub fn matches(&self, set: ContextSet) -> bool {
        match self {
            ContextExpr::Always => true,
            ContextExpr::Is(c) => set.contains(*c),
            ContextExpr::Not(e) => !e.matches(set),
            ContextExpr::And(a, b) => a.matches(set) && b.matches(set),
            ContextExpr::Or(a, b) => a.matches(set) || b.matches(set),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Ident(String),
    And,
    Or,
    Not,
    Open,
    Close,
}

fn tokenize(s: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = s.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let pair = chars.get(i + 1).copied();
        match c {
            _ if c.is_whitespace() => i += 1,
            '&' if pair == Some('&') => {
                tokens.push(Token::And);
                i += 2;
            }
            '|' if pair == Some('|') => {
                tokens.push(Token::Or);
                i += 2;
            }
            '!' => {
                tokens.push(Token::Not);
                i += 1;
            }
            '(' => {
                tokens.push(Token::Open);
                i += 1;
            }
            ')' => {
                tokens.push(Token::Close);
                i += 1;
            }
            _ if c.is_alphanumeric() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                tokens.push(Token::Ident(chars[start..i].iter().collect()));
            }
            _ => {
                return Err(tf!(
                    "Context \"{s}\" contains an invalid character '{c}'",
                    "コンテキスト \"{s}\" に使えない文字 '{c}' があります"
                ));
            }
        }
    }
    Ok(tokens)
}

fn parse_or(tokens: &[Token], pos: &mut usize) -> Result<ContextExpr, String> {
    let mut left = parse_and(tokens, pos)?;
    while tokens.get(*pos) == Some(&Token::Or) {
        *pos += 1;
        let right = parse_and(tokens, pos)?;
        left = ContextExpr::Or(Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn parse_and(tokens: &[Token], pos: &mut usize) -> Result<ContextExpr, String> {
    let mut left = parse_unary(tokens, pos)?;
    while tokens.get(*pos) == Some(&Token::And) {
        *pos += 1;
        let right = parse_unary(tokens, pos)?;
        left = ContextExpr::And(Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn parse_unary(tokens: &[Token], pos: &mut usize) -> Result<ContextExpr, String> {
    let token = tokens.get(*pos);
    *pos += 1;
    match token {
        Some(Token::Not) => Ok(ContextExpr::Not(Box::new(parse_unary(tokens, pos)?))),
        Some(Token::Open) => {
            let inner = parse_or(tokens, pos)?;
            if tokens.get(*pos) != Some(&Token::Close) {
                return Err(t!(
                    "Unclosed ( in the context expression",
                    "コンテキストの ( が閉じられていません"
                )
                .to_string());
            }
            *pos += 1;
            Ok(inner)
        }
        Some(Token::Ident(name)) => Ctx::ALL
            .iter()
            .find(|c| c.name() == name)
            .map(|c| ContextExpr::Is(*c))
            .ok_or_else(|| {
                let names: Vec<&str> = Ctx::ALL.iter().map(|c| c.name()).collect();
                let names = names.join(" / ");
                tf!(
                    "Unknown context \"{name}\" (available: {names})",
                    "不明なコンテキスト \"{name}\" (使える名前: {names})"
                )
            }),
        _ => Err(t!(
            "Invalid context expression",
            "コンテキストの書き方が正しくありません"
        )
        .to_string()),
    }
}

/// 1 回のキー入力。文字キーの Shift は文字そのもの (大文字 / 記号) で表す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyStroke {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

const NAMED_KEYS: &[(&str, KeyCode)] = &[
    ("enter", KeyCode::Enter),
    ("escape", KeyCode::Esc),
    ("tab", KeyCode::Tab),
    ("backspace", KeyCode::Backspace),
    ("delete", KeyCode::Delete),
    ("space", KeyCode::Char(' ')),
    ("up", KeyCode::Up),
    ("down", KeyCode::Down),
    ("left", KeyCode::Left),
    ("right", KeyCode::Right),
    ("home", KeyCode::Home),
    ("end", KeyCode::End),
    ("pageup", KeyCode::PageUp),
    ("pagedown", KeyCode::PageDown),
    ("insert", KeyCode::Insert),
];

const MODIFIERS: &[(&str, KeyModifiers)] = &[
    ("ctrl-", KeyModifiers::CONTROL),
    ("alt-", KeyModifiers::ALT),
    ("shift-", KeyModifiers::SHIFT),
    ("cmd-", KeyModifiers::SUPER),
];

impl KeyStroke {
    pub fn new(code: KeyCode, mods: KeyModifiers) -> Self {
        Self { code, mods }
    }

    pub fn from_event(key: &KeyEvent) -> Self {
        let mut mods = key.modifiers
            & (KeyModifiers::CONTROL
                | KeyModifiers::ALT
                | KeyModifiers::SHIFT
                | KeyModifiers::SUPER);
        let code = match key.code {
            KeyCode::BackTab => {
                mods |= KeyModifiers::SHIFT;
                KeyCode::Tab
            }
            KeyCode::Char(c) => {
                let c = if mods.contains(KeyModifiers::SHIFT) {
                    upper(c)
                } else {
                    c
                };
                mods -= KeyModifiers::SHIFT;
                KeyCode::Char(c)
            }
            code => code,
        };
        Self { code, mods }
    }

    /// `"ctrl-w"` / `"shift-g"` / `"escape"` のような 1 打鍵分の表記を読む。
    pub fn parse(s: &str) -> Result<Self, String> {
        let mut mods = KeyModifiers::NONE;
        let mut rest = s;
        while let Some((m, r)) = MODIFIERS.iter().find_map(|(prefix, m)| {
            rest.strip_prefix(prefix)
                .filter(|r| !r.is_empty())
                .map(|r| (*m, r))
        }) {
            mods |= m;
            rest = r;
        }
        let mut chars = rest.chars();
        let code = if let Some(code) = named_key(rest) {
            code
        } else if let (Some(c), None) = (chars.next(), chars.next()) {
            KeyCode::Char(c)
        } else {
            return Err(tf!(
                "Cannot parse key \"{s}\"",
                "キー \"{s}\" を解釈できません"
            ));
        };
        let code = match code {
            KeyCode::Char(c) if mods.contains(KeyModifiers::SHIFT) => {
                if !c.is_alphabetic() {
                    return Err(tf!(
                        "Key \"{s}\": shift- only works with letters and special keys (write symbols as they are, like \"?\")",
                        "キー \"{s}\": shift- は英字と特殊キーにだけ使えます (記号は \"?\" のようにそのまま書いてください)"
                    ));
                }
                mods -= KeyModifiers::SHIFT;
                KeyCode::Char(upper(c))
            }
            code => code,
        };
        Ok(Self { code, mods })
    }

    /// 修飾キーを無視した形。割り当てが見つからなかったときの再検索に使う
    /// (Shift+Enter を Enter として扱うなど)。変わらないときは None。
    fn relaxed(self) -> Option<Self> {
        let mods = match self.code {
            KeyCode::Char(_) => self.mods - (KeyModifiers::ALT | KeyModifiers::SUPER),
            // Shift+Tab は Tab とは別のキーとして扱う
            KeyCode::Tab => return None,
            _ => KeyModifiers::NONE,
        };
        (mods != self.mods).then_some(Self { mods, ..self })
    }

    /// ヘッダ行に出す短い表記 (`^W` / `d` / `<enter>`)。
    pub fn short(self) -> String {
        match self.code {
            KeyCode::Char(c) if self.mods == KeyModifiers::CONTROL => {
                format!("^{}", upper(c))
            }
            KeyCode::Char(c) if self.mods.is_empty() && c != ' ' => c.to_string(),
            _ => format!("<{self}>"),
        }
    }
}

/// 名前の付いたキー (`"enter"` / `"pageup"` / `"f5"`)。小文字で渡す。
pub(crate) fn named_key(name: &str) -> Option<KeyCode> {
    if let Some((_, code)) = NAMED_KEYS.iter().find(|(n, _)| *n == name) {
        return Some(*code);
    }
    let n = name.strip_prefix('f')?.parse::<u8>().ok()?;
    (1..=12).contains(&n).then_some(KeyCode::F(n))
}

pub(crate) fn upper(c: char) -> char {
    let mut it = c.to_uppercase();
    match (it.next(), it.next()) {
        (Some(u), None) => u,
        _ => c,
    }
}

/// 設定ファイルと同じ表記で表示する。
impl fmt::Display for KeyStroke {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut mods = self.mods;
        let mut key = match self.code {
            KeyCode::Char(c) if c.is_uppercase() => {
                mods |= KeyModifiers::SHIFT;
                c.to_lowercase().collect::<String>()
            }
            KeyCode::Char(c) => c.to_string(),
            KeyCode::F(n) => format!("f{n}"),
            _ => String::new(),
        };
        if let Some((name, _)) = NAMED_KEYS.iter().find(|(_, code)| *code == self.code) {
            key = name.to_string();
        }
        for (prefix, m) in MODIFIERS {
            if mods.contains(*m) {
                f.write_str(prefix)?;
            }
        }
        f.write_str(&key)
    }
}

/// `"g g"` / `"ctrl-w h"` のように空白で区切ったキー列を読む。
pub fn parse_keys(s: &str) -> Result<Vec<KeyStroke>, String> {
    let keys = s
        .split_whitespace()
        .map(KeyStroke::parse)
        .collect::<Result<Vec<_>, _>>()?;
    if keys.is_empty() {
        return Err(t!("The key is empty", "キーが空です").to_string());
    }
    Ok(keys)
}

/// 既定のキーバインド。(コンテキスト, [(キー, コマンド)])。後ろの組ほど優先される。
pub const DEFAULTS: &[(&str, &[(&str, Command)])] = &[
    (
        "",
        &[
            ("ctrl-c", Command::AppQuit),
            ("ctrl-r", Command::AppSend),
            ("f5", Command::AppSend),
            ("ctrl-s", Command::AppSave),
            ("shift-escape", Command::PaneToggleZoom),
        ],
    ),
    (
        "Normal",
        &[
            ("q", Command::AppQuit),
            ("?", Command::AppShowHelp),
            (":", Command::AppOpenCommandLine),
            ("tab", Command::PaneFocusNext),
            ("shift-tab", Command::PaneFocusPrev),
            ("ctrl-h", Command::PaneFocusLeft),
            ("ctrl-j", Command::PaneFocusDown),
            ("ctrl-k", Command::PaneFocusUp),
            ("ctrl-l", Command::PaneFocusRight),
            ("ctrl-w h", Command::PaneFocusLeft),
            ("ctrl-w j", Command::PaneFocusDown),
            ("ctrl-w k", Command::PaneFocusUp),
            ("ctrl-w l", Command::PaneFocusRight),
            ("ctrl-w ctrl-h", Command::PaneFocusLeft),
            ("ctrl-w ctrl-j", Command::PaneFocusDown),
            ("ctrl-w ctrl-k", Command::PaneFocusUp),
            ("ctrl-w ctrl-l", Command::PaneFocusRight),
            ("ctrl-w left", Command::PaneFocusLeft),
            ("ctrl-w down", Command::PaneFocusDown),
            ("ctrl-w up", Command::PaneFocusUp),
            ("ctrl-w right", Command::PaneFocusRight),
            ("ctrl-w w", Command::PaneFocusNext),
            ("ctrl-w ctrl-w", Command::PaneFocusNext),
            ("ctrl-w shift-w", Command::PaneFocusPrev),
            ("ctrl-w t", Command::PaneFocusCollection),
            ("ctrl-w b", Command::PaneFocusResponse),
            ("ctrl-w z", Command::PaneToggleZoom),
            ("m", Command::RequestNextMethod),
            ("shift-m", Command::RequestPrevMethod),
            ("shift-l", Command::TabNext),
            ("shift-h", Command::TabPrev),
            ("g t", Command::TabNext),
            ("g shift-t", Command::TabPrev),
            ("1", Command::TabSelect1),
            ("2", Command::TabSelect2),
            ("3", Command::TabSelect3),
            ("4", Command::TabSelect4),
        ],
    ),
    (
        "Normal && Collection",
        &[
            ("j", Command::CollectionSelectNext),
            ("down", Command::CollectionSelectNext),
            ("k", Command::CollectionSelectPrev),
            ("up", Command::CollectionSelectPrev),
            ("g g", Command::CollectionSelectFirst),
            ("shift-g", Command::CollectionSelectLast),
            ("enter", Command::PaneFocusRight),
            ("l", Command::PaneFocusRight),
            ("right", Command::PaneFocusRight),
            ("o", Command::CollectionNewBelow),
            ("n", Command::CollectionNewBelow),
            ("shift-o", Command::CollectionNewAbove),
            ("y y", Command::CollectionYank),
            ("d d", Command::CollectionDelete),
            ("p", Command::CollectionPasteAfter),
            ("shift-p", Command::CollectionPasteBefore),
            ("r", Command::CollectionRename),
        ],
    ),
    (
        "Normal && Response",
        &[
            ("j", Command::ResponseScrollDown),
            ("down", Command::ResponseScrollDown),
            ("enter", Command::ResponseScrollDown),
            ("ctrl-e", Command::ResponseScrollDown),
            ("k", Command::ResponseScrollUp),
            ("up", Command::ResponseScrollUp),
            ("ctrl-y", Command::ResponseScrollUp),
            ("ctrl-d", Command::ResponseHalfPageDown),
            ("ctrl-u", Command::ResponseHalfPageUp),
            ("ctrl-f", Command::ResponsePageDown),
            ("pagedown", Command::ResponsePageDown),
            ("ctrl-b", Command::ResponsePageUp),
            ("pageup", Command::ResponsePageUp),
            ("g g", Command::ResponseScrollToTop),
            ("home", Command::ResponseScrollToTop),
            ("shift-g", Command::ResponseScrollToBottom),
            ("end", Command::ResponseScrollToBottom),
        ],
    ),
    (
        "Normal && Preview",
        &[
            ("+", Command::PreviewZoomIn),
            ("=", Command::PreviewZoomIn),
            ("-", Command::PreviewZoomOut),
            ("0", Command::PreviewZoomReset),
        ],
    ),
    (
        "Normal && Editor",
        &[
            ("h", Command::VimMoveLeft),
            ("left", Command::VimMoveLeft),
            ("backspace", Command::VimMoveLeft),
            ("l", Command::VimMoveRight),
            ("right", Command::VimMoveRight),
            ("space", Command::VimMoveRight),
            ("k", Command::VimMoveUp),
            ("up", Command::VimMoveUp),
            ("j", Command::VimMoveDown),
            ("down", Command::VimMoveDown),
            ("w", Command::VimMoveWordForward),
            ("b", Command::VimMoveWordBackward),
            ("e", Command::VimMoveWordEnd),
            ("0", Command::VimMoveToLineStart),
            ("home", Command::VimMoveToLineStart),
            ("^", Command::VimMoveToFirstNonBlank),
            ("$", Command::VimMoveToLineEnd),
            ("end", Command::VimMoveToLineEnd),
            ("g g", Command::VimMoveToTop),
            ("shift-g", Command::VimMoveToBottom),
            ("i", Command::VimInsert),
            ("a", Command::VimAppend),
            ("shift-i", Command::VimInsertAtLineStart),
            ("shift-a", Command::VimAppendAtLineEnd),
            ("enter", Command::VimAppendAtLineEnd),
            ("o", Command::VimOpenLineBelow),
            ("shift-o", Command::VimOpenLineAbove),
            ("x", Command::VimDeleteChar),
            ("delete", Command::VimDeleteChar),
            ("shift-x", Command::VimDeleteCharBefore),
            ("shift-d", Command::VimDeleteToLineEnd),
            ("shift-c", Command::VimChangeToLineEnd),
            ("s", Command::VimSubstituteChar),
            ("shift-s", Command::VimSubstituteLine),
            ("shift-y", Command::VimYankLine),
            ("p", Command::VimPasteAfter),
            ("shift-p", Command::VimPasteBefore),
            ("u", Command::VimUndo),
            ("r", Command::VimReplaceChar),
            ("d", Command::VimDelete),
            ("c", Command::VimChange),
            ("y", Command::VimYank),
            ("v", Command::VimVisual),
            ("shift-v", Command::VimVisualLine),
            ("ctrl-e", Command::EditorOpenExternal),
        ],
    ),
    // 読み取り専用のテキスト: 移動と選択・ヤンクだけ (Response のスクロールより優先する)
    (
        "Normal && TextView",
        &[
            ("h", Command::VimMoveLeft),
            ("left", Command::VimMoveLeft),
            ("backspace", Command::VimMoveLeft),
            ("l", Command::VimMoveRight),
            ("right", Command::VimMoveRight),
            ("space", Command::VimMoveRight),
            ("k", Command::VimMoveUp),
            ("up", Command::VimMoveUp),
            ("j", Command::VimMoveDown),
            ("down", Command::VimMoveDown),
            ("w", Command::VimMoveWordForward),
            ("b", Command::VimMoveWordBackward),
            ("e", Command::VimMoveWordEnd),
            ("0", Command::VimMoveToLineStart),
            ("home", Command::VimMoveToLineStart),
            ("^", Command::VimMoveToFirstNonBlank),
            ("$", Command::VimMoveToLineEnd),
            ("end", Command::VimMoveToLineEnd),
            ("g g", Command::VimMoveToTop),
            ("shift-g", Command::VimMoveToBottom),
            ("enter", Command::VimMoveDown),
            ("shift-y", Command::VimYankLine),
            ("y", Command::VimYank),
            ("v", Command::VimVisual),
            ("shift-v", Command::VimVisualLine),
        ],
    ),
    (
        "Visual",
        &[
            ("h", Command::VimMoveLeft),
            ("left", Command::VimMoveLeft),
            ("backspace", Command::VimMoveLeft),
            ("l", Command::VimMoveRight),
            ("right", Command::VimMoveRight),
            ("space", Command::VimMoveRight),
            ("k", Command::VimMoveUp),
            ("up", Command::VimMoveUp),
            ("j", Command::VimMoveDown),
            ("down", Command::VimMoveDown),
            ("w", Command::VimMoveWordForward),
            ("b", Command::VimMoveWordBackward),
            ("e", Command::VimMoveWordEnd),
            ("0", Command::VimMoveToLineStart),
            ("home", Command::VimMoveToLineStart),
            ("^", Command::VimMoveToFirstNonBlank),
            ("$", Command::VimMoveToLineEnd),
            ("end", Command::VimMoveToLineEnd),
            ("g g", Command::VimMoveToTop),
            ("shift-g", Command::VimMoveToBottom),
            ("v", Command::VimVisual),
            ("shift-v", Command::VimVisualLine),
            ("o", Command::VimVisualSwapEnds),
            ("y", Command::VimYank),
            ("escape", Command::VimNormalMode),
        ],
    ),
    (
        "Visual && Response",
        &[
            ("ctrl-e", Command::ResponseScrollDown),
            ("ctrl-y", Command::ResponseScrollUp),
            ("ctrl-d", Command::ResponseHalfPageDown),
            ("ctrl-u", Command::ResponseHalfPageUp),
            ("ctrl-f", Command::ResponsePageDown),
            ("pagedown", Command::ResponsePageDown),
            ("ctrl-b", Command::ResponsePageUp),
            ("pageup", Command::ResponsePageUp),
        ],
    ),
    (
        "TextInput",
        &[
            ("backspace", Command::EditorBackspace),
            ("ctrl-h", Command::EditorBackspace),
            ("delete", Command::EditorDelete),
            ("ctrl-w", Command::EditorDeleteWordBefore),
            ("left", Command::EditorMoveLeft),
            ("right", Command::EditorMoveRight),
            ("up", Command::EditorMoveUp),
            ("down", Command::EditorMoveDown),
            ("ctrl-a", Command::EditorMoveToLineStart),
            ("home", Command::EditorMoveToLineStart),
            ("ctrl-e", Command::EditorMoveToLineEnd),
            ("end", Command::EditorMoveToLineEnd),
            ("ctrl-k", Command::EditorKillToLineEnd),
            ("ctrl-u", Command::EditorClear),
            ("enter", Command::EditorNewline),
        ],
    ),
    (
        "Insert",
        &[
            ("escape", Command::VimNormalMode),
            ("tab", Command::PaneFocusNext),
        ],
    ),
    ("Insert && Url", &[("enter", Command::VimNormalMode)]),
    (
        "Command",
        &[
            ("escape", Command::CommandCancel),
            ("enter", Command::CommandExecute),
        ],
    ),
    (
        "Rename",
        &[
            ("escape", Command::RenameCancel),
            ("enter", Command::RenameConfirm),
        ],
    ),
];

#[derive(Debug, Clone)]
struct Binding {
    context: ContextExpr,
    keys: Vec<KeyStroke>,
    /// None は「このキーには何も割り当てない」(設定ファイルの null)
    command: Option<Command>,
}

/// キー列を調べた結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup {
    /// `global` はコンテキストなし (どこでも有効) のバインドか
    Found {
        command: Command,
        global: bool,
    },
    /// 続きのキーを待つ
    Pending,
    None,
}

/// 設定ファイルの `"keymap"` の 1 要素。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Section {
    pub context: String,
    /// (キー, コマンド名)。コマンド名が None ならそのキーを無効にする
    pub bindings: Vec<(String, Option<String>)>,
}

#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: Vec<Binding>,
    user_count: usize,
}

impl Default for Keymap {
    fn default() -> Self {
        let mut bindings = Vec::new();
        for (context, keys) in DEFAULTS {
            let context = ContextExpr::parse(context).expect("既定のコンテキスト");
            for (key, command) in *keys {
                bindings.push(Binding {
                    context: context.clone(),
                    keys: parse_keys(key).expect("既定のキー"),
                    command: Some(*command),
                });
            }
        }
        Self {
            bindings,
            user_count: 0,
        }
    }
}

impl Keymap {
    /// 既定のキーマップに設定ファイルの内容を重ねる。
    pub fn with_user(sections: &[Section]) -> Result<Self, String> {
        let mut keymap = Self::default();
        for (i, section) in sections.iter().enumerate() {
            let n = i + 1;
            let at = tf!("\"keymap\" entry {n}", "\"keymap\" の {n} 番目");
            let context = ContextExpr::parse(&section.context).map_err(|e| format!("{at}: {e}"))?;
            for (key, name) in &section.bindings {
                let keys = parse_keys(key).map_err(|e| format!("{at}: {e}"))?;
                let command = match name {
                    None => None,
                    Some(name) => Some(Command::from_name(name).ok_or_else(|| {
                        tf!(
                            "{at} (\"{key}\"): unknown command \"{name}\"",
                            "{at} (\"{key}\"): 不明なコマンド \"{name}\""
                        )
                    })?),
                };
                keymap.bindings.push(Binding {
                    context: context.clone(),
                    keys,
                    command,
                });
                keymap.user_count += 1;
            }
        }
        Ok(keymap)
    }

    /// 設定ファイルで追加されたバインドの数。
    pub fn user_count(&self) -> usize {
        self.user_count
    }

    /// `ctx` で有効なバインドから `keys` に一致するものを探す。後に定義されたものが勝つ。
    pub fn lookup(&self, ctx: ContextSet, keys: &[KeyStroke]) -> Lookup {
        match self.lookup_exact(ctx, keys) {
            Lookup::None => {}
            found => return found,
        }
        // 修飾キーだけが違う場合は、修飾キーなしの割り当てを使う
        if let Some((last, head)) = keys.split_last()
            && let Some(relaxed) = last.relaxed()
        {
            let mut keys = head.to_vec();
            keys.push(relaxed);
            return self.lookup_exact(ctx, &keys);
        }
        Lookup::None
    }

    fn lookup_exact(&self, ctx: ContextSet, keys: &[KeyStroke]) -> Lookup {
        for b in self.bindings.iter().rev() {
            if !b.keys.starts_with(keys) || !b.context.matches(ctx) {
                continue;
            }
            return match b.command {
                // より長いキー列の途中
                Some(_) if b.keys.len() > keys.len() => Lookup::Pending,
                Some(command) => Lookup::Found {
                    command,
                    global: b.context == ContextExpr::Always,
                },
                None if b.keys.len() > keys.len() => continue,
                None => Lookup::None,
            };
        }
        Lookup::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(s: &str) -> Vec<KeyStroke> {
        parse_keys(s).unwrap()
    }

    fn found(command: Command, global: bool) -> Lookup {
        Lookup::Found { command, global }
    }

    #[test]
    fn command_names_are_unique_and_round_trip() {
        for (i, c) in Command::ALL.iter().enumerate() {
            assert_eq!(Command::from_name(c.name()), Some(*c));
            assert!(c.name().contains("::"));
            assert!(!c.description().is_empty());
            assert!(Command::ALL[..i].iter().all(|o| o.name() != c.name()));
        }
        assert_eq!(Command::from_name("app::Nope"), None);
    }

    #[test]
    fn key_notation_round_trips() {
        for (_, bindings) in DEFAULTS {
            for (key, _) in *bindings {
                let shown: Vec<String> = keys(key).iter().map(|k| k.to_string()).collect();
                assert_eq!(shown.join(" "), *key);
            }
        }
        assert_eq!(KeyStroke::parse("G"), KeyStroke::parse("shift-g"));
        assert_eq!(
            KeyStroke::parse("ctrl--").unwrap(),
            KeyStroke::new(KeyCode::Char('-'), KeyModifiers::CONTROL)
        );
        assert_eq!(KeyStroke::parse("f5").unwrap().code, KeyCode::F(5));
        assert!(KeyStroke::parse("shift-4").is_err());
        assert!(KeyStroke::parse("enterr").is_err());
        assert!(parse_keys("  ").is_err());
    }

    #[test]
    fn events_are_normalized() {
        let ev = |code, mods| KeyStroke::from_event(&KeyEvent::new(code, mods));
        assert_eq!(
            ev(KeyCode::Char('G'), KeyModifiers::SHIFT),
            KeyStroke::parse("shift-g").unwrap()
        );
        assert_eq!(
            ev(KeyCode::Char('?'), KeyModifiers::SHIFT),
            KeyStroke::parse("?").unwrap()
        );
        assert_eq!(
            ev(KeyCode::BackTab, KeyModifiers::NONE),
            KeyStroke::parse("shift-tab").unwrap()
        );
        assert_eq!(
            ev(KeyCode::Esc, KeyModifiers::SHIFT),
            KeyStroke::parse("shift-escape").unwrap()
        );
        assert_eq!(KeyStroke::parse("ctrl-w").unwrap().short(), "^W");
        assert_eq!(KeyStroke::parse("escape").unwrap().short(), "<escape>");
    }

    #[test]
    fn context_expressions() {
        let set = ContextSet::default().with(Ctx::Normal).with(Ctx::Url);
        let m = |s: &str| ContextExpr::parse(s).unwrap().matches(set);
        assert!(m(""));
        assert!(m("Normal"));
        assert!(m("Normal && Url"));
        assert!(!m("Normal && Collection"));
        assert!(m("Insert || Url"));
        assert!(m("Normal && !(Collection || Response)"));
        assert!(!m("!Normal"));
        assert!(ContextExpr::parse("Nomal").unwrap_err().contains("Nomal"));
        assert!(ContextExpr::parse("Normal &&").is_err());
        assert!(ContextExpr::parse("(Normal").is_err());
        assert!(ContextExpr::parse("Normal Url").is_err());
    }

    #[test]
    fn lookup_defaults() {
        let km = Keymap::default();
        let normal = ContextSet::default().with(Ctx::Normal);
        let editor = normal.with(Ctx::Url).with(Ctx::Editor);
        assert_eq!(
            km.lookup(normal, &keys("q")),
            found(Command::AppQuit, false)
        );
        assert_eq!(
            km.lookup(normal, &keys("ctrl-c")),
            found(Command::AppQuit, true)
        );
        assert_eq!(km.lookup(normal, &keys("g")), Lookup::Pending);
        assert_eq!(km.lookup(normal, &keys("g g")), Lookup::None);
        assert_eq!(
            km.lookup(editor, &keys("g g")),
            found(Command::VimMoveToTop, false)
        );
        assert_eq!(km.lookup(normal, &keys("x")), Lookup::None);
        // 修飾キーだけ違う特殊キーは、修飾キーなしとして扱う
        let insert = ContextSet::default().with(Ctx::Insert).with(Ctx::TextInput);
        assert_eq!(
            km.lookup(insert, &keys("shift-enter")),
            found(Command::EditorNewline, false)
        );
        assert_eq!(km.lookup(insert, &keys("shift-tab")), Lookup::None);
    }

    #[test]
    fn user_bindings_override_defaults() {
        let section = |context: &str, bindings: &[(&str, Option<&str>)]| Section {
            context: context.to_string(),
            bindings: bindings
                .iter()
                .map(|(k, c)| (k.to_string(), c.map(str::to_string)))
                .collect(),
        };
        let km = Keymap::with_user(&[
            section("Normal", &[("q", None), ("space w", Some("app::Save"))]),
            section("", &[("ctrl-g", Some("app::Send"))]),
        ])
        .unwrap();
        assert_eq!(km.user_count(), 3);
        let normal = ContextSet::default().with(Ctx::Normal);
        assert_eq!(km.lookup(normal, &keys("q")), Lookup::None);
        assert_eq!(km.lookup(normal, &keys("space")), Lookup::Pending);
        assert_eq!(
            km.lookup(normal, &keys("space w")),
            found(Command::AppSave, false)
        );
        assert_eq!(
            km.lookup(normal, &keys("ctrl-g")),
            found(Command::AppSend, true)
        );

        let err = Keymap::with_user(&[section("Normal", &[("x", Some("app::Nope"))])]);
        assert!(err.unwrap_err().contains("app::Nope"));
        assert!(Keymap::with_user(&[section("Nope", &[])]).is_err());
        assert!(Keymap::with_user(&[section("", &[("ctrl-", Some("app::Quit"))])]).is_err());
    }
}
