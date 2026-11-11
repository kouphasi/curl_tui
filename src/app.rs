//! アプリケーション状態とキー入力処理。
//!
//! I/O を持たない純粋な状態機械にしておき、テストしやすくしている。
//! 実際の送信や保存は [`Action`] として呼び出し側 (main) に返す。

use std::cell::Cell;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;

use crate::editor::TextEditor;
use crate::model::{self, HttpMethod, Request, Response};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Collection,
    Url,
    Request,
    Response,
}

impl Focus {
    const ORDER: [Focus; 4] = [
        Focus::Collection,
        Focus::Url,
        Focus::Request,
        Focus::Response,
    ];

    fn next(self) -> Self {
        let i = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0);
        Self::ORDER[(i + 1) % Self::ORDER.len()]
    }

    fn prev(self) -> Self {
        let i = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0);
        Self::ORDER[(i + Self::ORDER.len() - 1) % Self::ORDER.len()]
    }

    /// 右側の列 (URL / Request / Response) で 1 つ下のペイン。
    fn below(self) -> Option<Self> {
        match self {
            Focus::Url => Some(Focus::Request),
            Focus::Request => Some(Focus::Response),
            _ => None,
        }
    }

    fn above(self) -> Option<Self> {
        match self {
            Focus::Request => Some(Focus::Url),
            Focus::Response => Some(Focus::Request),
            _ => None,
        }
    }
}

/// Vim のオペレータ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Delete,
    Change,
    Yank,
}

impl Operator {
    fn from_char(c: char) -> Option<Self> {
        match c {
            'd' => Some(Operator::Delete),
            'c' => Some(Operator::Change),
            'y' => Some(Operator::Yank),
            _ => None,
        }
    }

    fn as_char(self) -> char {
        match self {
            Operator::Delete => 'd',
            Operator::Change => 'c',
            Operator::Yank => 'y',
        }
    }
}

/// 2 打鍵目を待っているキー。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pending {
    #[default]
    None,
    /// `g` (gg / gt / gT)
    G,
    /// `Ctrl+W` (ウィンドウ移動)
    Window,
    /// `d` / `c` / `y` の後のモーション待ち
    Op(Operator),
    /// `dg` など、オペレータの後の `g`
    OpG(Operator),
    /// `r` の後の置換文字待ち
    Replace,
}

/// カーソル移動。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordForward,
    WordBackward,
    WordEnd,
    LineStart,
    FirstNonBlank,
    LineEnd,
    Top,
    Bottom,
}

impl Motion {
    fn from_key(code: KeyCode) -> Option<Self> {
        Some(match code {
            KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => Motion::Left,
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Char(' ') => Motion::Right,
            KeyCode::Char('k') | KeyCode::Up => Motion::Up,
            KeyCode::Char('j') | KeyCode::Down => Motion::Down,
            KeyCode::Char('w') => Motion::WordForward,
            KeyCode::Char('b') => Motion::WordBackward,
            KeyCode::Char('e') => Motion::WordEnd,
            KeyCode::Char('0') | KeyCode::Home => Motion::LineStart,
            KeyCode::Char('^') => Motion::FirstNonBlank,
            KeyCode::Char('$') | KeyCode::End => Motion::LineEnd,
            KeyCode::Char('G') => Motion::Bottom,
            _ => return None,
        })
    }

    fn linewise(self) -> bool {
        matches!(
            self,
            Motion::Up | Motion::Down | Motion::Top | Motion::Bottom
        )
    }

    /// 移動先の文字も範囲に含むか (e, $)。
    fn inclusive(self) -> bool {
        matches!(self, Motion::WordEnd | Motion::LineEnd)
    }
}

/// ヤンクしたテキスト。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Register {
    pub text: String,
    pub linewise: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestTab {
    Params,
    Headers,
    Body,
    Curl,
}

impl RequestTab {
    pub const ALL: [RequestTab; 4] = [
        RequestTab::Params,
        RequestTab::Headers,
        RequestTab::Body,
        RequestTab::Curl,
    ];

    pub fn title(self) -> &'static str {
        match self {
            RequestTab::Params => "Params",
            RequestTab::Headers => "Headers",
            RequestTab::Body => "Body",
            RequestTab::Curl => "cURL",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    fn cycle(self, delta: isize) -> Self {
        let len = Self::ALL.len() as isize;
        Self::ALL[((self.index() as isize + delta).rem_euclid(len)) as usize]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseTab {
    Body,
    Headers,
    /// HTML をブラウザで描画した画像 (HTML のレスポンスのときだけ選べる)
    Preview,
}

impl ResponseTab {
    pub fn index(self) -> usize {
        match self {
            ResponseTab::Body => 0,
            ResponseTab::Headers => 1,
            ResponseTab::Preview => 2,
        }
    }
}

/// HTML プレビューの描画状態。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewState {
    /// まだ描画していない (Preview タブを開くと描画を始める)
    Idle,
    Rendering,
    /// 描画済み。`cols` は描画した幅 (セル数)、`rows` は画像の高さ (セル数)
    Ready {
        cols: u16,
        rows: u16,
    },
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    /// フォーカス中のテキスト欄を編集中 (Vim の Insert モード)
    Editing,
    /// `:` コマンドライン入力中
    Command,
    /// リクエスト名の変更中
    Rename,
    /// ヘルプ表示中
    Help,
}

/// main ループに実行してほしい副作用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    Send(Request),
    Save(Vec<Request>),
    /// `:wq` 保存してから終了
    SaveAndQuit(Vec<Request>),
}

/// 直近の送信結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseState {
    Empty,
    Loading,
    Done(Response),
    Failed(String),
}

pub const SPINNER: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];

pub struct App {
    pub collection: Vec<Request>,
    pub selected: usize,
    pub method: HttpMethod,
    pub url: TextEditor,
    pub params: TextEditor,
    pub headers: TextEditor,
    pub body: TextEditor,
    pub rename: TextEditor,
    pub focus: Focus,
    pub mode: Mode,
    pub request_tab: RequestTab,
    pub response_tab: ResponseTab,
    pub response: ResponseState,
    pub response_scroll: u16,
    /// 直近の描画でのレスポンス表示領域の高さ (Ctrl+D などのページ送りに使う)
    pub response_height: Cell<u16>,
    pub preview: PreviewState,
    /// レスポンスごとに増える番号。古いレスポンスの描画結果を捨てるのに使う
    pub preview_generation: u64,
    /// プレビューが使えない理由 (端末やブラウザが未対応)。main が起動時に設定する
    pub preview_unavailable: Option<String>,
    /// 直近の描画で Preview タブを表示した領域 (ポップアップ表示中などは None)
    pub preview_area: Cell<Option<Rect>>,
    pub spinner: usize,
    pub status: String,
    pub pending: Pending,
    pub command: TextEditor,
    /// テキスト欄でヤンクした内容
    pub register: Register,
    /// Collection でヤンク (yy / dd) したリクエスト
    pub request_register: Option<Request>,
    /// Collection から Ctrl+L で戻る先
    last_right_focus: Focus,
}

impl App {
    pub fn new(mut collection: Vec<Request>) -> Self {
        if collection.is_empty() {
            collection.push(Request::default());
        }
        let mut app = Self {
            collection,
            selected: 0,
            method: HttpMethod::Get,
            url: TextEditor::single_line(""),
            params: TextEditor::multi_line(""),
            headers: TextEditor::multi_line(""),
            body: TextEditor::multi_line(""),
            rename: TextEditor::single_line(""),
            focus: Focus::Url,
            mode: Mode::Normal,
            request_tab: RequestTab::Params,
            response_tab: ResponseTab::Body,
            response: ResponseState::Empty,
            response_scroll: 0,
            response_height: Cell::new(0),
            preview: PreviewState::Idle,
            preview_generation: 0,
            preview_unavailable: None,
            preview_area: Cell::new(None),
            spinner: 0,
            status: "? でヘルプ / Ctrl+R で送信 / Ctrl+H/J/K/L でペイン移動".to_string(),
            pending: Pending::None,
            command: TextEditor::single_line(""),
            register: Register::default(),
            request_register: None,
            last_right_focus: Focus::Url,
        };
        app.load_selected();
        app
    }

    /// エディタの内容から現在のリクエストを組み立てる。
    pub fn current_request(&self) -> Request {
        Request {
            name: self.collection[self.selected].name.clone(),
            method: self.method,
            url: self.url.text().trim().to_string(),
            params: model::parse_params(&self.params.text()),
            headers: model::parse_headers(&self.headers.text()),
            body: self.body.text(),
        }
    }

    /// エディタの内容をコレクションに書き戻す。
    pub fn commit(&mut self) {
        let req = self.current_request();
        self.collection[self.selected] = req;
    }

    fn load_selected(&mut self) {
        let req = self.collection[self.selected].clone();
        self.method = req.method;
        self.url.set_text(&req.url);
        self.params.set_text(&model::params_to_text(&req.params));
        self.headers.set_text(&model::headers_to_text(&req.headers));
        self.body.set_text(&req.body);
        for ed in [
            &mut self.url,
            &mut self.params,
            &mut self.headers,
            &mut self.body,
        ] {
            ed.clamp_normal();
        }
    }

    fn select(&mut self, index: usize) {
        if index == self.selected || index >= self.collection.len() {
            return;
        }
        self.commit();
        self.selected = index;
        self.load_selected();
        if self.response != ResponseState::Loading {
            self.response = ResponseState::Empty;
            self.reset_preview();
        }
        self.response_scroll = 0;
    }

    /// 編集対象のエディタ (編集不可なら None)。
    pub fn active_editor(&mut self) -> Option<&mut TextEditor> {
        match self.focus {
            Focus::Url => Some(&mut self.url),
            Focus::Request => match self.request_tab {
                RequestTab::Params => Some(&mut self.params),
                RequestTab::Headers => Some(&mut self.headers),
                RequestTab::Body => Some(&mut self.body),
                RequestTab::Curl => None,
            },
            _ => None,
        }
    }

    pub fn is_loading(&self) -> bool {
        self.response == ResponseState::Loading
    }

    /// スピナーを回す必要があるか。
    pub fn is_busy(&self) -> bool {
        self.is_loading() || self.preview == PreviewState::Rendering
    }

    /// 表示中のレスポンスが HTML か。
    pub fn response_is_html(&self) -> bool {
        matches!(&self.response, ResponseState::Done(r) if r.is_html())
    }

    /// 選択できるレスポンスタブ (Preview は HTML のときだけ)。
    pub fn response_tabs(&self) -> &'static [ResponseTab] {
        if self.response_is_html() {
            &[
                ResponseTab::Body,
                ResponseTab::Headers,
                ResponseTab::Preview,
            ]
        } else {
            &[ResponseTab::Body, ResponseTab::Headers]
        }
    }

    /// Preview タブを見ている間は Response ペインを全画面にする。
    /// 別のタブやペインに移ると元のレイアウトに戻る。
    pub fn preview_fullscreen(&self) -> bool {
        self.focus == Focus::Response
            && self.response_tab == ResponseTab::Preview
            && self.response_is_html()
            && self.preview_unavailable.is_none()
    }

    fn set_response_tab(&mut self, tab: ResponseTab) {
        if self.response_tabs().contains(&tab) {
            self.response_tab = tab;
            self.response_scroll = 0;
        }
    }

    pub fn tick(&mut self) {
        self.spinner = (self.spinner + 1) % SPINNER.len();
    }

    /// 送信結果を反映する。
    pub fn set_response(&mut self, result: Result<Response, String>) {
        self.response_scroll = 0;
        self.reset_preview();
        match result {
            Ok(r) => {
                self.status = format!("{} {} ({} ms)", r.status, r.status_text, r.elapsed_ms);
                self.response = ResponseState::Done(r);
            }
            Err(e) => {
                self.status = "送信に失敗しました".to_string();
                self.response = ResponseState::Failed(e);
            }
        }
        // HTML 以外では Preview タブは選べない
        if !self.response_tabs().contains(&self.response_tab) {
            self.response_tab = ResponseTab::Body;
        }
    }

    fn reset_preview(&mut self) {
        self.preview = PreviewState::Idle;
        self.preview_generation += 1;
    }

    pub fn handle_event(&mut self, event: Event) -> Action {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.handle_key(key),
            Event::Paste(text) => {
                match self.mode {
                    Mode::Editing => {
                        if let Some(ed) = self.active_editor() {
                            ed.insert_str(&text);
                        }
                    }
                    Mode::Rename => self.rename.insert_str(&text),
                    Mode::Command => self.command.insert_str(&text),
                    _ => {}
                }
                Action::None
            }
            _ => Action::None,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        // どのモードでも有効なショートカット
        if ctrl {
            match key.code {
                KeyCode::Char('c') => return Action::Quit,
                KeyCode::Char('r') => return self.send(),
                KeyCode::Char('s') => return self.save(),
                _ => {}
            }
        }
        if key.code == KeyCode::F(5) {
            return self.send();
        }

        match self.mode {
            Mode::Help => {
                self.mode = Mode::Normal;
                Action::None
            }
            Mode::Rename => {
                self.handle_rename_key(key);
                Action::None
            }
            Mode::Editing => {
                self.handle_editing_key(key);
                Action::None
            }
            Mode::Command => self.handle_command_key(key),
            Mode::Normal => self.handle_normal_key(key),
        }
    }

    fn send(&mut self) -> Action {
        if self.is_loading() {
            self.status = "送信中です…".to_string();
            return Action::None;
        }
        self.commit();
        let req = self.current_request();
        if let Err(e) = req.full_url() {
            self.status = e.to_string();
            return Action::None;
        }
        self.response = ResponseState::Loading;
        self.response_scroll = 0;
        self.status = format!("{} {} を送信中…", req.method, req.url);
        Action::Send(req)
    }

    fn save(&mut self) -> Action {
        self.commit();
        Action::Save(self.collection.clone())
    }

    fn handle_rename_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter => {
                let name = self.rename.text().trim().to_string();
                if !name.is_empty() {
                    self.collection[self.selected].name = name;
                }
                self.mode = Mode::Normal;
            }
            _ => edit_key(&mut self.rename, key),
        }
    }

    fn handle_editing_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.leave_insert(),
            KeyCode::Tab => {
                self.leave_insert();
                self.set_focus(self.focus.next());
            }
            KeyCode::Enter if self.focus == Focus::Url => self.leave_insert(),
            _ => {
                if let Some(ed) = self.active_editor() {
                    edit_key(ed, key);
                } else {
                    self.mode = Mode::Normal;
                }
            }
        }
    }

    /// Insert → Normal。Vim と同じくカーソルを 1 つ左に戻す。
    fn leave_insert(&mut self) {
        self.mode = Mode::Normal;
        if let Some(ed) = self.active_editor() {
            ed.move_left_in_line();
            ed.clamp_normal();
        }
        self.commit();
    }

    /// Insert モードに入る。`checkpoint` が true なら undo 地点を作る。
    fn enter_insert(&mut self, checkpoint: bool) {
        if let Some(ed) = self.active_editor() {
            if checkpoint {
                ed.checkpoint();
            }
            self.mode = Mode::Editing;
        }
    }

    fn set_focus(&mut self, focus: Focus) {
        self.focus = focus;
        if focus != Focus::Collection {
            self.last_right_focus = focus;
        }
    }

    /// Vim のウィンドウ移動 (Ctrl+H/J/K/L, Ctrl+W h/j/k/l)。
    fn move_focus(&mut self, dir: char) {
        let target = match dir {
            'h' => (self.focus != Focus::Collection).then_some(Focus::Collection),
            'l' => (self.focus == Focus::Collection).then_some(self.last_right_focus),
            'j' => self.focus.below(),
            'k' => self.focus.above(),
            _ => None,
        };
        if let Some(f) = target {
            self.set_focus(f);
        }
    }

    fn handle_window_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c @ ('h' | 'j' | 'k' | 'l')) => self.move_focus(c),
            KeyCode::Left => self.move_focus('h'),
            KeyCode::Down => self.move_focus('j'),
            KeyCode::Up => self.move_focus('k'),
            KeyCode::Right => self.move_focus('l'),
            KeyCode::Char('w') => self.set_focus(self.focus.next()),
            KeyCode::Char('W') => self.set_focus(self.focus.prev()),
            KeyCode::Char('t') => self.set_focus(Focus::Collection),
            KeyCode::Char('b') => self.set_focus(Focus::Response),
            _ => {}
        }
    }

    fn handle_command_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter => {
                self.mode = Mode::Normal;
                let cmd = self.command.text();
                return self.run_command(cmd.trim());
            }
            KeyCode::Backspace if self.command.is_empty() => self.mode = Mode::Normal,
            _ => edit_key(&mut self.command, key),
        }
        Action::None
    }

    /// `:` コマンドを実行する。
    fn run_command(&mut self, cmd: &str) -> Action {
        match cmd {
            "" => Action::None,
            "w" | "write" => self.save(),
            "q" | "q!" | "quit" | "qa" | "qa!" => Action::Quit,
            "wq" | "x" | "xa" | "wqa" => {
                self.commit();
                Action::SaveAndQuit(self.collection.clone())
            }
            "send" | "s" => self.send(),
            "h" | "help" => {
                self.mode = Mode::Help;
                Action::None
            }
            _ => {
                self.status = format!("不明なコマンド: :{cmd}");
                Action::None
            }
        }
    }

    /// 1 ページ (half なら半ページ) 分の行数。
    fn page_size(&self, half: bool) -> u16 {
        let h = self.response_height.get().max(2);
        if half { h / 2 } else { h }
    }

    fn cycle_tab(&mut self, delta: isize) {
        match self.focus {
            Focus::Request => self.request_tab = self.request_tab.cycle(delta),
            Focus::Response => {
                let tabs = self.response_tabs();
                let len = tabs.len() as isize;
                let cur = tabs
                    .iter()
                    .position(|t| *t == self.response_tab)
                    .unwrap_or(0) as isize;
                self.set_response_tab(tabs[(cur + delta).rem_euclid(len) as usize]);
            }
            _ => {}
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let pending = std::mem::take(&mut self.pending);
        if key.code == KeyCode::Esc {
            return Action::None;
        }
        match pending {
            Pending::None => {}
            Pending::Window => {
                // Ctrl+W Ctrl+W / Ctrl+W Ctrl+H なども受け付ける
                match key.code {
                    KeyCode::Char('w') if ctrl => self.set_focus(self.focus.next()),
                    _ => self.handle_window_key(key),
                }
                return Action::None;
            }
            Pending::G => {
                match key.code {
                    KeyCode::Char('t') => self.cycle_tab(1),
                    KeyCode::Char('T') => self.cycle_tab(-1),
                    KeyCode::Char('g') => self.goto_top(),
                    _ => {}
                }
                return Action::None;
            }
            Pending::Op(_) | Pending::OpG(_) | Pending::Replace => {
                self.handle_pending_edit(pending, key);
                return Action::None;
            }
        }

        if ctrl {
            match key.code {
                KeyCode::Char(c @ ('h' | 'j' | 'k' | 'l')) => self.move_focus(c),
                KeyCode::Char('w') => self.pending = Pending::Window,
                KeyCode::Char('d') => self.scroll_response(self.page_size(true) as i32),
                KeyCode::Char('u') => self.scroll_response(-(self.page_size(true) as i32)),
                KeyCode::Char('f') => self.scroll_response(self.page_size(false) as i32),
                KeyCode::Char('b') => self.scroll_response(-(self.page_size(false) as i32)),
                KeyCode::Char('e') => self.scroll_response(1),
                KeyCode::Char('y') => self.scroll_response(-1),
                _ => {}
            }
            return Action::None;
        }

        match key.code {
            KeyCode::Char('q') => return Action::Quit,
            KeyCode::Char('?') => self.mode = Mode::Help,
            KeyCode::Char(':') => {
                self.command = TextEditor::single_line("");
                self.mode = Mode::Command;
            }
            KeyCode::Tab => self.set_focus(self.focus.next()),
            KeyCode::BackTab => self.set_focus(self.focus.prev()),
            KeyCode::Char('m') => self.method = self.method.next(),
            KeyCode::Char('M') => self.method = self.method.prev(),
            KeyCode::Char('H') => self.cycle_tab(-1),
            KeyCode::Char('L') => self.cycle_tab(1),
            KeyCode::Char('g') => self.pending = Pending::G,
            KeyCode::Char(c @ '1'..='4') if self.focus == Focus::Request => {
                self.request_tab = RequestTab::ALL[(c as u8 - b'1') as usize];
            }
            KeyCode::Char('1') if self.focus == Focus::Response => {
                self.set_response_tab(ResponseTab::Body)
            }
            KeyCode::Char('2') if self.focus == Focus::Response => {
                self.set_response_tab(ResponseTab::Headers)
            }
            KeyCode::Char('3') if self.focus == Focus::Response => {
                self.set_response_tab(ResponseTab::Preview)
            }
            _ => match self.focus {
                Focus::Collection => self.handle_collection_key(key),
                Focus::Response => self.handle_response_key(key),
                Focus::Url | Focus::Request => self.handle_editor_normal_key(key),
            },
        }
        Action::None
    }

    /// `gg`
    fn goto_top(&mut self) {
        match self.focus {
            Focus::Collection => self.select(0),
            Focus::Response => self.response_scroll = 0,
            _ => {
                if let Some(ed) = self.active_editor() {
                    ed.set_cursor(0, 0);
                    let col = ed.first_non_blank_col(0);
                    ed.set_cursor(0, col);
                    ed.clamp_normal();
                }
            }
        }
    }

    fn scroll_response(&mut self, delta: i32) {
        if self.focus != Focus::Response {
            return;
        }
        let next = (self.response_scroll as i32 + delta).clamp(0, self.max_scroll() as i32);
        self.response_scroll = next as u16;
    }

    fn handle_collection_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.select(self.selected - 1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => self.select(self.selected + 1),
            KeyCode::Char('G') => self.select(self.collection.len() - 1),
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
                self.set_focus(self.last_right_focus)
            }
            KeyCode::Char('n') | KeyCode::Char('o') => {
                self.commit();
                self.collection
                    .insert(self.selected + 1, Request::default());
                self.select(self.selected + 1);
                self.status = "新しいリクエストを追加しました".to_string();
            }
            KeyCode::Char('O') => {
                self.commit();
                self.collection.insert(self.selected, Request::default());
                // select() は同じ index だと何もしないので直接読み込む
                self.load_selected();
                self.status = "新しいリクエストを追加しました".to_string();
            }
            KeyCode::Char('y') => self.pending = Pending::Op(Operator::Yank),
            KeyCode::Char('d') => self.pending = Pending::Op(Operator::Delete),
            KeyCode::Char('p') | KeyCode::Char('P') => {
                let Some(mut req) = self.request_register.clone() else {
                    self.status = "ヤンクしたリクエストがありません (yy でヤンク)".to_string();
                    return;
                };
                self.commit();
                let at = if key.code == KeyCode::Char('p') {
                    self.selected + 1
                } else {
                    self.selected
                };
                if self.collection.iter().any(|r| r.name == req.name) {
                    req.name = format!("{} (copy)", req.name);
                }
                self.collection.insert(at, req);
                self.selected = at;
                self.load_selected();
                self.status = "リクエストを貼り付けました".to_string();
            }
            KeyCode::Char('r') => {
                self.rename = TextEditor::single_line(&self.collection[self.selected].name);
                self.mode = Mode::Rename;
            }
            _ => {}
        }
    }

    /// Collection での dd / yy。
    fn collection_operator(&mut self, op: Operator, key: KeyEvent) {
        if key.code != KeyCode::Char(op.as_char()) {
            return;
        }
        self.commit();
        match op {
            Operator::Yank => {
                self.request_register = Some(self.collection[self.selected].clone());
                self.status = format!(
                    "「{}」をヤンクしました (p で貼り付け)",
                    self.collection[self.selected].name
                );
            }
            Operator::Delete => {
                let removed = self.collection.remove(self.selected);
                if self.collection.is_empty() {
                    self.collection.push(Request::default());
                }
                self.selected = self.selected.min(self.collection.len() - 1);
                self.load_selected();
                self.status = format!("「{}」を削除しました (p で貼り付け)", removed.name);
                self.request_register = Some(removed);
            }
            Operator::Change => {}
        }
    }

    /// テキスト欄 (URL / Params / Headers / Body) での Normal モード操作。
    fn handle_editor_normal_key(&mut self, key: KeyEvent) {
        if self.focus == Focus::Request && self.request_tab == RequestTab::Curl {
            return;
        }
        if let Some(motion) = Motion::from_key(key.code) {
            if let Some(ed) = self.active_editor() {
                apply_motion(ed, motion);
            }
            return;
        }
        let reg = self.register.clone();
        let Some(ed) = self.active_editor() else {
            return;
        };
        match key.code {
            KeyCode::Char('i') => {}
            KeyCode::Char('a') => {
                if ed.line_len(ed.cursor().0) > 0 {
                    ed.move_right();
                }
            }
            KeyCode::Char('I') => {
                let row = ed.cursor().0;
                let col = ed.first_non_blank_col(row);
                ed.set_cursor(row, col);
            }
            KeyCode::Char('A') | KeyCode::Enter => ed.move_end(),
            KeyCode::Char('o') | KeyCode::Char('O') => {
                ed.checkpoint();
                if ed.is_multiline() {
                    let row = ed.cursor().0;
                    ed.insert_lines(row, key.code == KeyCode::Char('O'), "");
                } else {
                    ed.move_end();
                }
                self.enter_insert(false);
                return;
            }
            KeyCode::Char('x') | KeyCode::Delete => {
                let (row, col) = ed.cursor();
                if col < ed.line_len(row) {
                    ed.checkpoint();
                    let text = ed.delete_range((row, col), (row, col + 1));
                    ed.clamp_normal();
                    self.register = Register {
                        text,
                        linewise: false,
                    };
                    self.commit();
                }
                return;
            }
            KeyCode::Char('X') => {
                let (row, col) = ed.cursor();
                if col > 0 {
                    ed.checkpoint();
                    let text = ed.delete_range((row, col - 1), (row, col));
                    self.register = Register {
                        text,
                        linewise: false,
                    };
                    self.commit();
                }
                return;
            }
            KeyCode::Char('D') => {
                self.operate(Operator::Delete, Motion::LineEnd);
                return;
            }
            KeyCode::Char('C') => {
                self.operate(Operator::Change, Motion::LineEnd);
                return;
            }
            KeyCode::Char('s') => {
                let (row, col) = ed.cursor();
                ed.checkpoint();
                if col < ed.line_len(row) {
                    let text = ed.delete_range((row, col), (row, col + 1));
                    self.register = Register {
                        text,
                        linewise: false,
                    };
                }
                self.enter_insert(false);
                return;
            }
            KeyCode::Char('S') => {
                ed.checkpoint();
                let text = ed.clear_line();
                self.register = Register {
                    text,
                    linewise: true,
                };
                self.enter_insert(false);
                return;
            }
            KeyCode::Char('Y') => {
                let row = ed.cursor().0;
                self.register = Register {
                    text: ed.lines_text(row, row),
                    linewise: true,
                };
                return;
            }
            KeyCode::Char('p') | KeyCode::Char('P') => {
                let before = key.code == KeyCode::Char('P');
                if reg.text.is_empty() {
                    return;
                }
                ed.checkpoint();
                if reg.linewise {
                    let row = ed.cursor().0;
                    ed.insert_lines(row, before, &reg.text);
                } else {
                    if !before && ed.line_len(ed.cursor().0) > 0 {
                        ed.move_right();
                    }
                    ed.insert_str(&reg.text);
                    ed.move_left();
                }
                ed.clamp_normal();
                self.commit();
                return;
            }
            KeyCode::Char('u') => {
                if !ed.undo() {
                    self.status = "これ以上元に戻せません".to_string();
                }
                self.commit();
                return;
            }
            KeyCode::Char('r') => {
                self.pending = Pending::Replace;
                return;
            }
            KeyCode::Char(c @ ('d' | 'c' | 'y')) => {
                self.pending = Pending::Op(Operator::from_char(c).unwrap());
                return;
            }
            _ => return,
        }
        // ここに来るのは Insert モードに入るキー (i / a / I / A / Enter)
        self.enter_insert(true);
    }

    /// オペレータ / r の 2 打鍵目。
    fn handle_pending_edit(&mut self, pending: Pending, key: KeyEvent) {
        match pending {
            Pending::Op(op) if self.focus == Focus::Collection => self.collection_operator(op, key),
            Pending::Op(op) => match key.code {
                KeyCode::Char(c) if c == op.as_char() => self.operate_lines(op, None),
                KeyCode::Char('g') => self.pending = Pending::OpG(op),
                code => {
                    if let Some(m) = Motion::from_key(code) {
                        self.operate(op, m);
                    }
                }
            },
            Pending::OpG(op) => {
                if key.code == KeyCode::Char('g') {
                    self.operate(op, Motion::Top);
                }
            }
            Pending::Replace => {
                if let KeyCode::Char(c) = key.code
                    && let Some(ed) = self.active_editor()
                {
                    let (row, col) = ed.cursor();
                    if col < ed.line_len(row) {
                        ed.checkpoint();
                        ed.delete_range((row, col), (row, col + 1));
                        ed.insert_char(c);
                        ed.set_cursor(row, col);
                        self.commit();
                    }
                }
            }
            _ => {}
        }
    }

    /// 行単位の操作 (dd / cc / yy、または j / k / gg / G と組み合わせたとき)。
    /// `to` が None なら現在行のみ。
    fn operate_lines(&mut self, op: Operator, to: Option<usize>) {
        let Some(ed) = self.active_editor() else {
            return;
        };
        let row = ed.cursor().0;
        let to = to.unwrap_or(row);
        let (first, last) = (row.min(to), row.max(to));
        let register = match op {
            Operator::Yank => {
                let text = ed.lines_text(first, last);
                ed.set_cursor(first, ed.cursor().1);
                ed.clamp_normal();
                text
            }
            Operator::Delete => {
                ed.checkpoint();
                ed.delete_lines(first, last)
            }
            Operator::Change => {
                ed.checkpoint();
                let text = ed.lines_text(first, last);
                if last > first {
                    ed.delete_lines(first + 1, last);
                }
                ed.set_cursor(first, 0);
                ed.clear_line();
                text
            }
        };
        self.register = Register {
            text: register,
            linewise: true,
        };
        if op == Operator::Change {
            self.enter_insert(false);
        } else {
            self.commit();
        }
    }

    /// オペレータ + モーション (dw / c$ / ye など)。
    fn operate(&mut self, op: Operator, motion: Motion) {
        let Some(ed) = self.active_editor() else {
            return;
        };
        if motion.linewise() {
            let target = motion_target(ed, motion).0;
            self.operate_lines(op, Some(target));
            return;
        }
        let cur = ed.cursor();
        // cw は ce と同じ (Vim の仕様)
        let motion = if op == Operator::Change
            && motion == Motion::WordForward
            && ed.line_len(cur.0) > cur.1
            && !ed.lines()[cur.0]
                .chars()
                .nth(cur.1)
                .unwrap()
                .is_whitespace()
        {
            Motion::WordEnd
        } else {
            motion
        };
        let mut target = motion_target(ed, motion);
        if motion == Motion::WordForward && target.0 > cur.0 {
            // dw は行をまたがない
            target = (cur.0, ed.line_len(cur.0));
        }
        if motion.inclusive() && target >= cur {
            target.1 = (target.1 + 1).min(ed.line_len(target.0));
        }
        let (start, end) = if target < cur {
            (target, cur)
        } else {
            (cur, target)
        };
        if start == end && op != Operator::Change {
            return;
        }
        let text = match op {
            Operator::Yank => {
                let t = ed.text_range(start, end);
                ed.set_cursor(start.0, start.1);
                t
            }
            Operator::Delete | Operator::Change => {
                ed.checkpoint();
                ed.delete_range(start, end)
            }
        };
        self.register = Register {
            text,
            linewise: false,
        };
        match op {
            Operator::Change => self.enter_insert(false),
            _ => {
                if let Some(ed) = self.active_editor() {
                    ed.clamp_normal();
                }
                self.commit();
            }
        }
    }

    /// ステータス行に表示する入力途中のキー。
    pub fn pending_keys(&self) -> String {
        match self.pending {
            Pending::None => String::new(),
            Pending::G => "g".into(),
            Pending::Window => "^W".into(),
            Pending::Op(op) => op.as_char().to_string(),
            Pending::OpG(op) => format!("{}g", op.as_char()),
            Pending::Replace => "r".into(),
        }
    }

    fn handle_response_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Enter => self.scroll_response(1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_response(-1),
            KeyCode::PageDown => self.scroll_response(self.page_size(false) as i32),
            KeyCode::PageUp => self.scroll_response(-(self.page_size(false) as i32)),
            KeyCode::Home => self.response_scroll = 0,
            KeyCode::Char('G') | KeyCode::End => self.response_scroll = self.max_scroll(),
            _ => {}
        }
    }

    /// 現在のレスポンスタブで表示する行数 - 1。
    fn max_scroll(&self) -> u16 {
        let lines = match &self.response {
            ResponseState::Done(r) => match self.response_tab {
                ResponseTab::Body => r.body.lines().count(),
                ResponseTab::Headers => r.headers.len(),
                // 画像は最後まで表示したところで止める
                ResponseTab::Preview => match self.preview {
                    PreviewState::Ready { rows, .. } => {
                        return rows.saturating_sub(self.response_height.get());
                    }
                    _ => 0,
                },
            },
            _ => 0,
        };
        lines.saturating_sub(1).min(u16::MAX as usize) as u16
    }
}

/// モーションの移動先。
fn motion_target(ed: &TextEditor, motion: Motion) -> (usize, usize) {
    let (row, col) = ed.cursor();
    let last_row = ed.line_count() - 1;
    match motion {
        Motion::Left => (row, col.saturating_sub(1)),
        Motion::Right => (row, (col + 1).min(ed.line_len(row))),
        Motion::Up => (row.saturating_sub(1), col),
        Motion::Down => ((row + 1).min(last_row), col),
        Motion::WordForward => ed.word_forward_pos(),
        Motion::WordBackward => ed.word_backward_pos(),
        Motion::WordEnd => ed.word_end_pos(),
        Motion::LineStart => (row, 0),
        Motion::FirstNonBlank => (row, ed.first_non_blank_col(row)),
        Motion::LineEnd => (row, ed.line_len(row).saturating_sub(1)),
        Motion::Top => (0, ed.first_non_blank_col(0)),
        Motion::Bottom => (last_row, ed.first_non_blank_col(last_row)),
    }
}

/// Normal モードでのカーソル移動。
fn apply_motion(ed: &mut TextEditor, motion: Motion) {
    match motion {
        Motion::Left => ed.move_left_in_line(),
        Motion::Right => ed.move_right_normal(),
        _ => {
            let (row, col) = motion_target(ed, motion);
            ed.set_cursor(row, col);
        }
    }
    ed.clamp_normal();
}

/// エディタ共通のキー操作 (Insert モード / コマンドライン / 名前変更)。
fn edit_key(ed: &mut TextEditor, key: KeyEvent) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('h') if ctrl => ed.backspace(),
        KeyCode::Char('w') if ctrl => ed.delete_word_before(),
        KeyCode::Char('a') if ctrl => ed.move_home(),
        KeyCode::Char('e') if ctrl => ed.move_end(),
        KeyCode::Char('k') if ctrl => ed.kill_to_end(),
        KeyCode::Char('u') if ctrl => ed.clear(),
        KeyCode::Char(c) if !ctrl => ed.insert_char(c),
        KeyCode::Enter => ed.insert_newline(),
        KeyCode::Backspace => ed.backspace(),
        KeyCode::Delete => ed.delete(),
        KeyCode::Left => ed.move_left(),
        KeyCode::Right => ed.move_right(),
        KeyCode::Up => ed.move_up(),
        KeyCode::Down => ed.move_down(),
        KeyCode::Home => ed.move_home(),
        KeyCode::End => ed.move_end(),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::KeyValue;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn type_str(app: &mut App, s: &str) {
        for c in s.chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
    }

    fn sample_response(body: &str) -> Response {
        Response {
            status: 200,
            status_text: "OK".into(),
            elapsed_ms: 5,
            headers: vec![("content-type".into(), "application/json".into())],
            body: body.into(),
            size: body.len(),
            ..Default::default()
        }
    }

    #[test]
    fn new_app_loads_first_request() {
        let app = App::new(vec![Request {
            url: "http://a".into(),
            headers: vec![KeyValue::new("X", "1")],
            ..Default::default()
        }]);
        assert_eq!(app.url.text(), "http://a");
        assert_eq!(app.headers.text(), "X: 1");
        assert_eq!(app.focus, Focus::Url);
    }

    #[test]
    fn empty_collection_gets_default() {
        let app = App::new(vec![]);
        assert_eq!(app.collection.len(), 1);
    }

    #[test]
    fn tab_cycles_focus() {
        let mut app = App::new(vec![]);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Request);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Response);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Collection);
        app.handle_key(key(KeyCode::BackTab));
        assert_eq!(app.focus, Focus::Response);
    }

    #[test]
    fn q_quits_in_normal_but_types_in_editing() {
        let mut app = App::new(vec![]);
        app.handle_key(key(KeyCode::Char('A')));
        assert_eq!(app.mode, Mode::Editing);
        assert_eq!(app.handle_key(key(KeyCode::Char('q'))), Action::None);
        assert!(app.url.text().ends_with('q'));
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.handle_key(key(KeyCode::Char('q'))), Action::Quit);
        assert_eq!(app.handle_key(ctrl('c')), Action::Quit);
    }

    #[test]
    fn edit_url_and_send_produces_request() {
        let mut app = App::new(vec![]);
        app.handle_key(key(KeyCode::Enter));
        app.handle_key(ctrl('u'));
        type_str(&mut app, "localhost:8080/items");
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.mode, Mode::Normal);
        app.handle_key(key(KeyCode::Char('m')));

        let action = app.handle_key(ctrl('r'));
        let Action::Send(req) = action else {
            panic!("expected Send, got {action:?}");
        };
        assert_eq!(req.method, HttpMethod::Post);
        assert_eq!(req.url, "localhost:8080/items");
        assert!(app.is_loading());
        // 送信中の二重送信は無視される
        assert_eq!(app.handle_key(key(KeyCode::F(5))), Action::None);
    }

    #[test]
    fn send_with_empty_url_is_rejected() {
        let mut app = App::new(vec![Request {
            url: String::new(),
            ..Default::default()
        }]);
        assert_eq!(app.handle_key(ctrl('r')), Action::None);
        assert!(app.status.contains("URL"));
        assert_eq!(app.response, ResponseState::Empty);
    }

    #[test]
    fn editing_headers_and_body_via_tabs() {
        let mut app = App::new(vec![]);
        app.handle_key(key(KeyCode::Tab)); // Request
        app.handle_key(key(KeyCode::Char('2'))); // Headers
        assert_eq!(app.request_tab, RequestTab::Headers);
        app.handle_key(key(KeyCode::Char('i')));
        type_str(&mut app, "Content-Type: application/json");
        app.handle_key(key(KeyCode::Enter));
        type_str(&mut app, "# X-Off: 1");
        app.handle_key(key(KeyCode::Esc));

        app.handle_key(key(KeyCode::Char('L'))); // Body
        assert_eq!(app.request_tab, RequestTab::Body);
        app.handle_key(key(KeyCode::Enter));
        type_str(&mut app, "{\"a\":1}");
        app.handle_key(key(KeyCode::Esc));

        let req = app.current_request();
        assert_eq!(
            req.headers,
            vec![
                KeyValue::new("Content-Type", "application/json"),
                KeyValue::disabled("X-Off", "1")
            ]
        );
        assert_eq!(req.body, "{\"a\":1}");
        assert_eq!(app.collection[0], req);
    }

    #[test]
    fn curl_tab_is_not_editable() {
        let mut app = App::new(vec![]);
        app.focus = Focus::Request;
        app.handle_key(key(KeyCode::Char('4')));
        assert_eq!(app.request_tab, RequestTab::Curl);
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn request_tabs_wrap_around() {
        let mut app = App::new(vec![]);
        app.focus = Focus::Request;
        app.handle_key(key(KeyCode::Char('H')));
        assert_eq!(app.request_tab, RequestTab::Curl);
        app.handle_key(key(KeyCode::Char('L')));
        assert_eq!(app.request_tab, RequestTab::Params);
        app.handle_key(key(KeyCode::Char('g')));
        app.handle_key(key(KeyCode::Char('t')));
        assert_eq!(app.request_tab, RequestTab::Headers);
        app.handle_key(key(KeyCode::Char('g')));
        app.handle_key(key(KeyCode::Char('T')));
        assert_eq!(app.request_tab, RequestTab::Params);
    }

    #[test]
    fn collection_switch_preserves_edits() {
        let mut app = App::new(vec![
            Request {
                name: "one".into(),
                url: "http://one".into(),
                ..Default::default()
            },
            Request {
                name: "two".into(),
                url: "http://two".into(),
                ..Default::default()
            },
        ]);
        app.handle_key(key(KeyCode::Enter));
        type_str(&mut app, "/edited");
        app.handle_key(key(KeyCode::Esc));

        app.focus = Focus::Collection;
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.selected, 1);
        assert_eq!(app.url.text(), "http://two");
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.url.text(), "http://one/edited");
        // 範囲外には移動しない
        app.handle_key(key(KeyCode::Up));
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn collection_new_duplicate_delete() {
        let mut app = App::new(vec![]);
        app.focus = Focus::Collection;
        app.handle_key(key(KeyCode::Char('o')));
        assert_eq!(app.collection.len(), 2);
        assert_eq!(app.selected, 1);

        // yy でヤンク、p で直後に貼り付け (= 複製)
        type_str(&mut app, "yyp");
        assert_eq!(app.collection.len(), 3);
        assert_eq!(app.collection[2].name, "New Request (copy)");
        assert_eq!(app.selected, 2);

        // d 1 回では消えない
        app.handle_key(key(KeyCode::Char('d')));
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.collection.len(), 3);

        type_str(&mut app, "dddddd");
        // 最後の 1 件を消しても空にはならない
        assert_eq!(app.collection.len(), 1);
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn rename_request() {
        let mut app = App::new(vec![]);
        app.focus = Focus::Collection;
        app.handle_key(key(KeyCode::Char('r')));
        assert_eq!(app.mode, Mode::Rename);
        app.handle_key(ctrl('u'));
        type_str(&mut app, "Get users");
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.collection[0].name, "Get users");

        // 空文字やEscでは変わらない
        app.handle_key(key(KeyCode::Char('r')));
        app.handle_key(ctrl('u'));
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.collection[0].name, "Get users");
        app.handle_key(key(KeyCode::Char('r')));
        type_str(&mut app, "zzz");
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.collection[0].name, "Get users");
    }

    #[test]
    fn save_returns_committed_collection() {
        let mut app = App::new(vec![]);
        app.handle_key(key(KeyCode::Char('m')));
        let Action::Save(reqs) = app.handle_key(ctrl('s')) else {
            panic!("expected Save");
        };
        assert_eq!(reqs[0].method, HttpMethod::Post);
    }

    fn html_response() -> Response {
        Response {
            status: 200,
            headers: vec![("Content-Type".into(), "text/html; charset=utf-8".into())],
            body: "<html><body>hi</body></html>".into(),
            ..Default::default()
        }
    }

    #[test]
    fn preview_tab_only_for_html() {
        let mut app = App::new(vec![]);
        app.focus = Focus::Response;
        // JSON では Preview タブは選べない
        app.set_response(Ok(sample_response("{}")));
        app.handle_key(key(KeyCode::Char('3')));
        assert_eq!(app.response_tab, ResponseTab::Body);
        app.handle_key(key(KeyCode::Char('L')));
        app.handle_key(key(KeyCode::Char('L')));
        assert_eq!(app.response_tab, ResponseTab::Body);

        // HTML なら選べる
        app.set_response(Ok(html_response()));
        app.handle_key(key(KeyCode::Char('3')));
        assert_eq!(app.response_tab, ResponseTab::Preview);
        app.handle_key(key(KeyCode::Char('L')));
        assert_eq!(app.response_tab, ResponseTab::Body);
        app.handle_key(key(KeyCode::Char('H')));
        assert_eq!(app.response_tab, ResponseTab::Preview);

        // 次のレスポンスが JSON なら Body に戻る
        let generation = app.preview_generation;
        app.set_response(Ok(sample_response("{}")));
        assert_eq!(app.response_tab, ResponseTab::Body);
        assert_eq!(app.preview, PreviewState::Idle);
        assert!(app.preview_generation > generation);
    }

    #[test]
    fn preview_scroll_is_limited_to_image_height() {
        let mut app = App::new(vec![]);
        app.focus = Focus::Response;
        app.set_response(Ok(html_response()));
        app.handle_key(key(KeyCode::Char('3')));
        app.response_height.set(10);
        app.preview = PreviewState::Ready { cols: 80, rows: 25 };
        app.handle_key(key(KeyCode::Char('G')));
        assert_eq!(app.response_scroll, 15);
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.response_scroll, 15);
    }

    #[test]
    fn response_scroll_is_clamped() {
        let mut app = App::new(vec![]);
        app.set_response(Ok(sample_response("a\nb\nc")));
        app.focus = Focus::Response;
        for _ in 0..10 {
            app.handle_key(key(KeyCode::Char('j')));
        }
        assert_eq!(app.response_scroll, 2);
        app.handle_key(key(KeyCode::Char('k')));
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.response_scroll, 2);
        app.handle_key(key(KeyCode::Char('G')));
        assert_eq!(app.response_scroll, 2);
        app.handle_key(key(KeyCode::Char('g')));
        app.handle_key(key(KeyCode::Char('g')));
        assert_eq!(app.response_scroll, 0);
        app.handle_key(key(KeyCode::Char('G')));
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.response_scroll, 1);
        app.handle_key(key(KeyCode::Char('L')));
        assert_eq!(app.response_tab, ResponseTab::Headers);
        assert_eq!(app.response_scroll, 0);
    }

    #[test]
    fn set_response_updates_state_and_status() {
        let mut app = App::new(vec![]);
        app.set_response(Ok(sample_response("{}")));
        assert!(matches!(app.response, ResponseState::Done(_)));
        assert!(app.status.starts_with("200 OK"));
        app.set_response(Err("boom".into()));
        assert_eq!(app.response, ResponseState::Failed("boom".into()));
    }

    #[test]
    fn paste_goes_to_active_editor_only_when_editing() {
        let mut app = App::new(vec![]);
        app.focus = Focus::Request;
        app.request_tab = RequestTab::Body;
        app.handle_event(Event::Paste("ignored".into()));
        assert!(app.body.is_empty());
        app.handle_key(key(KeyCode::Enter));
        app.handle_event(Event::Paste("line1\nline2".into()));
        assert_eq!(app.body.text(), "line1\nline2");
    }

    #[test]
    fn help_closes_on_any_key() {
        let mut app = App::new(vec![]);
        app.handle_key(key(KeyCode::Char('?')));
        assert_eq!(app.mode, Mode::Help);
        assert_eq!(app.handle_key(key(KeyCode::Char('q'))), Action::None);
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn method_cycles_from_url_focus() {
        let mut app = App::new(vec![]);
        app.handle_key(key(KeyCode::Char('m')));
        assert_eq!(app.method, HttpMethod::Post);
        app.handle_key(key(KeyCode::Char('M')));
        app.handle_key(key(KeyCode::Char('M')));
        assert_eq!(app.method, HttpMethod::Options);
    }

    #[test]
    fn ctrl_hjkl_moves_between_panes() {
        let mut app = App::new(vec![]);
        assert_eq!(app.focus, Focus::Url);
        app.handle_key(ctrl('j'));
        assert_eq!(app.focus, Focus::Request);
        app.handle_key(ctrl('j'));
        assert_eq!(app.focus, Focus::Response);
        // 一番下より下には行かない
        app.handle_key(ctrl('j'));
        assert_eq!(app.focus, Focus::Response);
        app.handle_key(ctrl('h'));
        assert_eq!(app.focus, Focus::Collection);
        // Collection から右へ戻ると直前のペインに戻る
        app.handle_key(ctrl('l'));
        assert_eq!(app.focus, Focus::Response);
        app.handle_key(ctrl('k'));
        assert_eq!(app.focus, Focus::Request);
        // Ctrl+W 系
        app.handle_key(ctrl('w'));
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.focus, Focus::Url);
        app.handle_key(ctrl('w'));
        app.handle_key(key(KeyCode::Char('h')));
        assert_eq!(app.focus, Focus::Collection);
        app.handle_key(ctrl('w'));
        app.handle_key(ctrl('w'));
        assert_eq!(app.focus, Focus::Url);
    }

    #[test]
    fn insert_and_normal_mode_in_url() {
        let mut app = App::new(vec![Request {
            url: "http://example.com/users".into(),
            ..Default::default()
        }]);
        // Normal: カーソルは最後の文字
        assert_eq!(app.url.cursor(), (0, 23));
        type_str(&mut app, "0");
        assert_eq!(app.url.cursor(), (0, 0));
        // w で単語移動、cw で単語を書き換え
        type_str(&mut app, "w");
        assert_eq!(app.url.cursor(), (0, 4)); // "://"
        type_str(&mut app, "w");
        assert_eq!(app.url.cursor(), (0, 7)); // "example"
        type_str(&mut app, "cwlocalhost");
        assert_eq!(app.mode, Mode::Editing);
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.url.text(), "http://localhost.com/users");
        // Esc で 1 つ左に戻る
        assert_eq!(app.url.cursor(), (0, 15));
        // x / u
        type_str(&mut app, "x");
        assert_eq!(app.url.text(), "http://localhos.com/users");
        type_str(&mut app, "u");
        assert_eq!(app.url.text(), "http://localhost.com/users");
        // d$ / D
        type_str(&mut app, "lD");
        assert_eq!(app.url.text(), "http://localhost");
        assert_eq!(app.collection[0].url, "http://localhost");
        // I / A
        type_str(&mut app, "A:8080");
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.url.text(), "http://localhost:8080");
        // r で 1 文字置換
        type_str(&mut app, "0rH");
        assert_eq!(app.url.text(), "Http://localhost:8080");
        // undo は Insert 1 回分をまとめて戻す
        type_str(&mut app, "uu");
        assert_eq!(app.url.text(), "http://localhost");
    }

    #[test]
    fn line_operations_in_body() {
        let mut app = App::new(vec![]);
        app.focus = Focus::Request;
        app.request_tab = RequestTab::Body;
        type_str(&mut app, "i{");
        app.handle_key(key(KeyCode::Esc));
        type_str(&mut app, "o  \"a\": 1");
        app.handle_key(key(KeyCode::Esc));
        type_str(&mut app, "o}");
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.body.text(), "{\n  \"a\": 1\n}");

        // yy + P で行を複製
        type_str(&mut app, "kyyP");
        assert_eq!(app.body.text(), "{\n  \"a\": 1\n  \"a\": 1\n}");
        assert_eq!(app.body.cursor(), (1, 2));
        // dd で削除 → p で下に貼り付け
        type_str(&mut app, "ddp");
        assert_eq!(app.body.text(), "{\n  \"a\": 1\n  \"a\": 1\n}");
        // dj で 2 行削除
        type_str(&mut app, "ggjdj");
        assert_eq!(app.body.text(), "{\n}");
        // dgg で先頭まで削除
        type_str(&mut app, "Gdgg");
        assert!(app.body.is_empty());
        type_str(&mut app, "u");
        assert_eq!(app.body.text(), "{\n}");
        // cc で行を書き換え
        type_str(&mut app, "ccX");
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.body.text(), "{\nX");
        assert_eq!(app.collection[0].body, "{\nX");
    }

    #[test]
    fn colon_commands() {
        let mut app = App::new(vec![]);
        type_str(&mut app, ":w");
        assert_eq!(app.mode, Mode::Command);
        assert!(matches!(
            app.handle_key(key(KeyCode::Enter)),
            Action::Save(_)
        ));
        assert_eq!(app.mode, Mode::Normal);

        type_str(&mut app, ":wq");
        assert!(matches!(
            app.handle_key(key(KeyCode::Enter)),
            Action::SaveAndQuit(_)
        ));
        type_str(&mut app, ":q");
        assert_eq!(app.handle_key(key(KeyCode::Enter)), Action::Quit);

        type_str(&mut app, ":foo");
        assert_eq!(app.handle_key(key(KeyCode::Enter)), Action::None);
        assert!(app.status.contains("不明なコマンド"));

        // Esc / 空での Backspace でキャンセル
        type_str(&mut app, ":q");
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Normal);
        type_str(&mut app, ":");
        app.handle_key(key(KeyCode::Backspace));
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn response_page_scroll() {
        let mut app = App::new(vec![]);
        let body: Vec<String> = (0..100).map(|i| i.to_string()).collect();
        app.set_response(Ok(sample_response(&body.join("\n"))));
        app.focus = Focus::Response;
        app.response_height.set(20);
        app.handle_key(ctrl('d'));
        assert_eq!(app.response_scroll, 10);
        app.handle_key(ctrl('f'));
        assert_eq!(app.response_scroll, 30);
        app.handle_key(ctrl('u'));
        assert_eq!(app.response_scroll, 20);
        app.handle_key(ctrl('b'));
        assert_eq!(app.response_scroll, 0);
        app.handle_key(ctrl('b'));
        assert_eq!(app.response_scroll, 0);
    }

    #[test]
    fn pending_key_is_cancelled_by_esc() {
        let mut app = App::new(vec![]);
        type_str(&mut app, "d");
        assert_eq!(app.pending_keys(), "d");
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.pending_keys(), "");
        // オペレータ待ちの q は終了しない
        type_str(&mut app, "d");
        assert_eq!(app.handle_key(key(KeyCode::Char('q'))), Action::None);
        assert_eq!(app.url.text(), "https://httpbin.org/get");
    }
}
