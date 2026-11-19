//! アプリケーション状態とキー入力処理。
//!
//! I/O を持たない純粋な状態機械にしておき、テストしやすくしている。
//! 実際の送信や保存は [`Action`] として呼び出し側 (main) に返す。

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::time::Instant;

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};

use crate::curl;
use crate::editor::TextEditor;
use crate::keymap::{Command, ContextSet, Ctx, KeyStroke, Keymap, Lookup};
use crate::model::{self, HttpMethod, Request, Response};
use crate::vimrc::{MapMode, Vimrc};
use crate::{t, tf};

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

/// プレビューの表示倍率 (%) の既定値・範囲・`+` / `-` での増減幅。
pub const PREVIEW_ZOOM_DEFAULT: u16 = 100;
pub const PREVIEW_ZOOM_MIN: u16 = 25;
pub const PREVIEW_ZOOM_MAX: u16 = 400;
pub const PREVIEW_ZOOM_STEP: u16 = 25;

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
    /// 文字を選択中 (Vim の Visual モード)。`y` でクリップボードにコピーする
    Visual,
    /// `:` コマンドライン入力中
    Command,
    /// リクエスト名の変更中
    Rename,
    /// ヘルプ表示中
    Help,
}

/// 外部エディタ ($EDITOR) で編集できるテキスト欄。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextField {
    Url,
    Params,
    Headers,
    Body,
}

impl TextField {
    pub fn name(self) -> &'static str {
        match self {
            TextField::Url => "URL",
            TextField::Params => "Params",
            TextField::Headers => "Headers",
            TextField::Body => "Body",
        }
    }
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
    /// 設定ファイルをエディタで開き、閉じたら読み直す
    EditConfig,
    /// vimrc ファイルをエディタで開き、閉じたら読み直す
    EditVimrc,
    /// テキストをクリップボードにコピーする
    Copy(String),
    /// テキスト欄の内容を外部エディタ ($EDITOR) で開き、閉じたら
    /// [`App::apply_external_edit`] で書き戻す
    EditText {
        field: TextField,
        text: String,
    },
}

/// Visual モードの選択状態 (選択を始めた位置と、行単位かどうか)。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Visual {
    pub anchor: (usize, usize),
    pub linewise: bool,
}

/// 選択範囲。`start` から `end` まで (`end` の文字も含む)。位置は (行, 文字単位の列)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub start: (usize, usize),
    pub end: (usize, usize),
    pub linewise: bool,
}

impl Selection {
    pub fn contains(&self, row: usize, col: usize) -> bool {
        if row < self.start.0 || row > self.end.0 {
            return false;
        }
        self.linewise || ((row, col) >= self.start && (row, col) <= self.end)
    }
}

/// 読み取り専用のテキスト (カーソル移動と選択・コピーだけできる)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewId {
    Curl,
    ResponseBody,
    ResponseHeaders,
}

/// 直近の送信結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseState {
    Empty,
    Loading,
    Done(Response),
    Failed(String),
}

/// テキスト欄の表示領域とスクロール量 (クリック位置をカーソル位置に変換するのに使う)。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EditorRegion {
    pub area: Rect,
    pub scroll_x: u16,
    pub scroll_y: u16,
    /// 折り返して表示しているか (読み取り専用ビュー)。`scroll_y` は折り返し後の行数
    pub wrap: bool,
}

/// 直近の描画での各ペインの位置。マウスのクリック先を判定するために ui が記録する。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Regions {
    pub collection: Rect,
    /// Collection の一覧部分 (枠の内側) と、先頭に表示している項目の番号
    pub collection_list: Rect,
    pub collection_offset: usize,
    /// Method と URL を合わせた領域
    pub url: Rect,
    pub url_editor: Option<EditorRegion>,
    pub request: Rect,
    pub request_tabs: Vec<Rect>,
    pub request_editor: Option<EditorRegion>,
    pub response: Rect,
    pub response_tabs: Vec<Rect>,
    /// Response の Body / Headers タブの本文部分
    pub response_editor: Option<EditorRegion>,
}

/// マウスホイール 1 目盛りでスクロールする行数。
const WHEEL_LINES: i32 = 3;

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
    /// cURL タブの表示内容 (読み取り専用)。キー入力のたびにリクエストから作り直す
    pub curl_view: TextEditor,
    /// `curl_view` の元になった curl コマンド
    curl_text: String,
    pub curl_scroll: u16,
    /// Response の Body / Headers タブの表示内容 (読み取り専用)
    pub response_body_view: TextEditor,
    pub response_headers_view: TextEditor,
    pub focus: Focus,
    /// フォーカス中のペインを全画面表示しているか (Shift+Esc / Ctrl+W z で切り替え)
    pub zoomed: bool,
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
    /// プレビューの表示倍率 (%)。100 で端末の文字と同程度の大きさ
    pub preview_zoom: u16,
    /// プレビューが使えない理由 (端末やブラウザが未対応)。main が起動時に設定する
    pub preview_unavailable: Option<String>,
    /// 直近の描画で Preview タブを表示した領域 (ポップアップ表示中などは None)
    pub preview_area: Cell<Option<Rect>>,
    /// 直近の描画での各ペインの位置 (マウス操作用)
    pub regions: RefCell<Regions>,
    pub spinner: usize,
    pub status: String,
    pub keymap: Keymap,
    /// 入力途中のキー列 (`g` / `Ctrl+W` など、続きのキーを待っている)
    keys: Vec<KeyStroke>,
    /// モーション待ちのオペレータと、それを呼び出したキーの表示
    operator: Option<(Operator, String)>,
    /// `r` の後の置換文字待ち (呼び出したキーの表示)
    replace: Option<String>,
    pub command: TextEditor,
    /// テキスト欄でヤンクした内容
    pub register: Register,
    /// Collection でヤンク (yy / dd) したリクエスト
    pub request_register: Option<Request>,
    /// Visual モードの選択状態 (`mode` が Visual のときだけ意味を持つ)
    pub visual: Visual,
    /// 今回のキー入力でクリップボードにコピーするテキスト
    clipboard: Option<String>,
    /// Collection から Ctrl+L で戻る先
    last_right_focus: Focus,
    /// 設定ファイルの "vimrc" (キー → キーのマッピング)
    vimrc: Vimrc,
    /// まだ処理していないキー (Vim の typeahead)。マッピングの続きを待っているか、
    /// マッピングを展開した結果のキー
    typeahead: VecDeque<Queued>,
    /// マッピングの続きのキーを待ち始めた時刻 (待っていないときは None)
    waiting_since: Option<Instant>,
}

/// typeahead の 1 キー。
#[derive(Debug, Clone, Copy)]
struct Queued {
    event: KeyEvent,
    /// マッピングを適用してよいか (noremap の展開結果は false)
    remap: bool,
}

/// マッピングの展開を繰り返せる回数 (Vim の maxmapdepth)。
const MAX_MAP_DEPTH: usize = 1000;

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
            curl_view: TextEditor::multi_line(""),
            curl_text: String::new(),
            curl_scroll: 0,
            response_body_view: TextEditor::multi_line(""),
            response_headers_view: TextEditor::multi_line(""),
            focus: Focus::Url,
            zoomed: false,
            mode: Mode::Normal,
            request_tab: RequestTab::Params,
            response_tab: ResponseTab::Body,
            response: ResponseState::Empty,
            response_scroll: 0,
            response_height: Cell::new(0),
            preview: PreviewState::Idle,
            preview_generation: 0,
            preview_zoom: PREVIEW_ZOOM_DEFAULT,
            preview_unavailable: None,
            preview_area: Cell::new(None),
            regions: RefCell::new(Regions::default()),
            spinner: 0,
            status: t!(
                "? for help / Ctrl+R to send / Ctrl+H/J/K/L to move between panes",
                "? でヘルプ / Ctrl+R で送信 / Ctrl+H/J/K/L でペイン移動"
            )
            .to_string(),
            keymap: Keymap::default(),
            keys: Vec::new(),
            operator: None,
            replace: None,
            command: TextEditor::single_line(""),
            register: Register::default(),
            request_register: None,
            visual: Visual::default(),
            clipboard: None,
            last_right_focus: Focus::Url,
            vimrc: Vimrc::default(),
            typeahead: VecDeque::new(),
            waiting_since: None,
        };
        app.load_selected();
        app.sync_curl_view();
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

    /// Response で表示中の読み取り専用ビュー (レスポンスが無いときや Preview タブでは None)。
    fn response_view_id(&self) -> Option<ViewId> {
        if !matches!(self.response, ResponseState::Done(_)) {
            return None;
        }
        match self.response_tab {
            ResponseTab::Body => Some(ViewId::ResponseBody),
            ResponseTab::Headers => Some(ViewId::ResponseHeaders),
            ResponseTab::Preview => None,
        }
    }

    /// フォーカス中の読み取り専用ビュー。
    fn focused_view(&self) -> Option<ViewId> {
        match self.focus {
            Focus::Request if self.request_tab == RequestTab::Curl => Some(ViewId::Curl),
            Focus::Response => self.response_view_id(),
            _ => None,
        }
    }

    fn view(&self, id: ViewId) -> &TextEditor {
        match id {
            ViewId::Curl => &self.curl_view,
            ViewId::ResponseBody => &self.response_body_view,
            ViewId::ResponseHeaders => &self.response_headers_view,
        }
    }

    /// ビューと、そのスクロール位置 (折り返し後の行数)。
    fn view_mut(&mut self, id: ViewId) -> (&mut TextEditor, &mut u16) {
        match id {
            ViewId::Curl => (&mut self.curl_view, &mut self.curl_scroll),
            ViewId::ResponseBody => (&mut self.response_body_view, &mut self.response_scroll),
            ViewId::ResponseHeaders => (&mut self.response_headers_view, &mut self.response_scroll),
        }
    }

    /// Response で表示中のテキスト (描画用)。
    pub fn response_view(&self) -> Option<&TextEditor> {
        self.response_view_id().map(|id| self.view(id))
    }

    /// カーソルを置けるテキスト (編集できる欄と読み取り専用ビュー)。
    fn active_text(&mut self) -> Option<&mut TextEditor> {
        match self.focused_view() {
            Some(id) => Some(self.view_mut(id).0),
            None => self.active_editor(),
        }
    }

    fn active_text_ref(&self) -> Option<&TextEditor> {
        if let Some(id) = self.focused_view() {
            return Some(self.view(id));
        }
        match self.focus {
            Focus::Url => Some(&self.url),
            Focus::Request => match self.request_tab {
                RequestTab::Params => Some(&self.params),
                RequestTab::Headers => Some(&self.headers),
                RequestTab::Body => Some(&self.body),
                RequestTab::Curl => None,
            },
            _ => None,
        }
    }

    /// Visual モードで選択中の範囲。
    pub fn selection(&self) -> Option<Selection> {
        if self.mode != Mode::Visual {
            return None;
        }
        let text = self.active_text_ref()?;
        let cursor = text.cursor();
        // タブの切り替えなどで別のテキストになっていても範囲外を指さないようにする
        let row = self.visual.anchor.0.min(text.line_count() - 1);
        let anchor = (row, self.visual.anchor.1.min(text.line_len(row)));
        Some(Selection {
            start: anchor.min(cursor),
            end: anchor.max(cursor),
            linewise: self.visual.linewise,
        })
    }

    /// cURL タブの内容を現在のリクエストに合わせる。
    fn sync_curl_view(&mut self) {
        let text = curl::to_curl(&self.current_request());
        if text == self.curl_text {
            return;
        }
        self.curl_view.set_text(&text);
        self.curl_view.set_cursor(0, 0);
        self.curl_text = text;
        self.curl_scroll = 0;
        if self.mode == Mode::Visual && self.focused_view() == Some(ViewId::Curl) {
            self.mode = Mode::Normal;
        }
    }

    /// 直近の描画でのビューの (幅, 高さ)。まだ描画していなければ幅 0 (折り返さない)。
    fn view_size(&self, id: ViewId) -> (usize, usize) {
        let regions = self.regions.borrow();
        let region = match id {
            ViewId::Curl => regions.request_editor,
            ViewId::ResponseBody | ViewId::ResponseHeaders => regions.response_editor,
        };
        let (width, height) = region
            .filter(|r| r.wrap)
            .map(|r| (r.area.width as usize, r.area.height as usize))
            .unwrap_or((0, 0));
        let height = if height == 0 && id != ViewId::Curl {
            self.response_height.get() as usize
        } else {
            height
        };
        (width, height.max(1))
    }

    /// フォーカス中のビューを、カーソルが見える位置までスクロールする。
    fn follow_cursor(&mut self) {
        let Some(id) = self.focused_view() else {
            return;
        };
        let (width, height) = self.view_size(id);
        let (ed, scroll) = self.view_mut(id);
        let row = ed.cursor_visual_row(width);
        let top = *scroll as usize;
        let top = if row < top {
            row
        } else if row >= top + height {
            row + 1 - height
        } else {
            top
        };
        *scroll = top.min(u16::MAX as usize) as u16;
    }

    /// ビューをスクロールする。カーソルが画面外に出るなら画面内に引き戻す (Vim と同じ)。
    fn scroll_view(&mut self, id: ViewId, delta: i32) {
        let (width, height) = self.view_size(id);
        let (ed, scroll) = self.view_mut(id);
        let max = ed
            .visual_rows(width)
            .saturating_sub(1)
            .min(u16::MAX as usize) as i32;
        let top = (*scroll as i32 + delta).clamp(0, max) as usize;
        *scroll = top as u16;
        let row = ed.cursor_visual_row(width);
        let target = if row < top {
            top
        } else if row >= top + height {
            top + height - 1
        } else {
            return;
        };
        let (row, col) = ed.pos_at_visual(target, ed.cursor_visual_col(width), width);
        ed.set_cursor(row, col);
        ed.clamp_normal();
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

    fn set_response_tab(&mut self, tab: ResponseTab) {
        if self.response_tabs().contains(&tab) {
            self.response_tab = tab;
            self.response_scroll = 0;
            self.follow_cursor();
        }
    }

    pub fn tick(&mut self) {
        self.spinner = (self.spinner + 1) % SPINNER.len();
    }

    /// 送信結果を反映する。
    pub fn set_response(&mut self, result: Result<Response, String>) {
        // 選択中のテキストが置き換わるので Visual モードは抜ける
        if self.mode == Mode::Visual && self.focus == Focus::Response {
            self.mode = Mode::Normal;
        }
        self.response_scroll = 0;
        self.reset_preview();
        let (body, headers) = match &result {
            Ok(r) => (
                r.body.as_str(),
                r.headers
                    .iter()
                    .map(|(k, v)| format!("{k}: {v}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            Err(_) => ("", String::new()),
        };
        self.response_body_view.set_text(body);
        self.response_body_view.set_cursor(0, 0);
        self.response_headers_view.set_text(&headers);
        self.response_headers_view.set_cursor(0, 0);
        match result {
            Ok(r) => {
                self.status = format!("{} {} ({} ms)", r.status, r.status_text, r.elapsed_ms);
                self.response = ResponseState::Done(r);
            }
            Err(e) => {
                self.status = t!("Request failed", "送信に失敗しました").to_string();
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

    /// プレビューの表示倍率を変えて描き直す。
    pub fn set_preview_zoom(&mut self, percent: i32) {
        let zoom = percent.clamp(PREVIEW_ZOOM_MIN as i32, PREVIEW_ZOOM_MAX as i32) as u16;
        self.status = tf!("Preview zoom {zoom}%", "プレビュー倍率 {zoom}%");
        if zoom != self.preview_zoom {
            self.preview_zoom = zoom;
            self.response_scroll = 0;
            self.reset_preview();
        }
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
                self.sync_curl_view();
                Action::None
            }
            Event::Mouse(mouse) => {
                self.handle_mouse(mouse);
                Action::None
            }
            _ => Action::None,
        }
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => self.click(mouse.column, mouse.row),
            MouseEventKind::ScrollDown => self.wheel(mouse.column, mouse.row, WHEEL_LINES),
            MouseEventKind::ScrollUp => self.wheel(mouse.column, mouse.row, -WHEEL_LINES),
            _ => {}
        }
        self.sync_curl_view();
    }

    /// 左クリック: ペイン / リクエスト / タブを選び、テキスト欄ならカーソルを置く。
    fn click(&mut self, x: u16, y: u16) {
        match self.mode {
            Mode::Help => {
                self.mode = Mode::Normal;
                return;
            }
            Mode::Rename | Mode::Command => return,
            // クリックで選択を解除する
            Mode::Visual => self.mode = Mode::Normal,
            Mode::Normal | Mode::Editing => {}
        }
        let regions = self.regions.borrow().clone();
        let pos = Position::new(x, y);
        let (target, editor) = if regions.collection.contains(pos) {
            (Focus::Collection, None)
        } else if regions.url.contains(pos) {
            (Focus::Url, regions.url_editor)
        } else if regions.request.contains(pos) {
            (Focus::Request, regions.request_editor)
        } else if regions.response.contains(pos) {
            (Focus::Response, regions.response_editor)
        } else {
            return;
        };
        let editor = editor.filter(|e| e.area.contains(pos));
        self.clear_pending();
        // 編集中の欄の中をクリックしたときだけ Insert モードのままにする
        if self.mode == Mode::Editing && !(target == self.focus && editor.is_some()) {
            self.leave_insert();
        }
        self.set_focus(target);

        match target {
            Focus::Collection => {
                let list = regions.collection_list;
                if list.contains(pos) {
                    self.select(regions.collection_offset + (y - list.y) as usize);
                }
            }
            Focus::Request => {
                if let Some(i) = regions.request_tabs.iter().position(|r| r.contains(pos)) {
                    self.request_tab = RequestTab::ALL[i];
                }
            }
            Focus::Response => {
                if let Some(i) = regions.response_tabs.iter().position(|r| r.contains(pos))
                    && let Some(tab) = self.response_tabs().get(i)
                {
                    self.set_response_tab(*tab);
                }
            }
            Focus::Url => {}
        }

        if let Some(region) = editor {
            let insert = self.mode == Mode::Editing;
            if let Some(ed) = self.active_text() {
                let dx = (x - region.area.x) as usize;
                let dy = (y - region.area.y) as usize;
                let (row, col) = if region.wrap {
                    let width = region.area.width as usize;
                    ed.pos_at_visual(dy + region.scroll_y as usize, dx, width)
                } else {
                    let row = (dy + region.scroll_y as usize).min(ed.line_count() - 1);
                    (row, ed.col_at_display(row, dx + region.scroll_x as usize))
                };
                ed.set_cursor(row, col);
                if !insert {
                    ed.clamp_normal();
                }
            }
        }
    }

    /// ホイール: Response / cURL タブの上ならフォーカスに関係なくスクロールする。
    fn wheel(&mut self, x: u16, y: u16, delta: i32) {
        if !matches!(self.mode, Mode::Normal | Mode::Editing | Mode::Visual) {
            return;
        }
        let pos = Position::new(x, y);
        let (on_response, on_request) = {
            let regions = self.regions.borrow();
            (
                regions.response.contains(pos),
                regions.request.contains(pos),
            )
        };
        if on_response {
            self.scroll_response_by(delta);
        } else if on_request && self.request_tab == RequestTab::Curl {
            self.scroll_view(ViewId::Curl, delta);
        }
    }

    /// いま有効なコンテキスト (キーバインドの "context" と照らし合わせる)。
    pub fn contexts(&self) -> ContextSet {
        let set = ContextSet::default();
        let pane = match self.focus {
            Focus::Collection => Ctx::Collection,
            Focus::Url => Ctx::Url,
            Focus::Request => Ctx::Request,
            Focus::Response => Ctx::Response,
        };
        match self.mode {
            Mode::Help => set.with(Ctx::Help),
            Mode::Rename => set.with(Ctx::Rename).with(Ctx::TextInput),
            Mode::Command => set.with(Ctx::Command).with(Ctx::TextInput),
            Mode::Editing => set
                .with(Ctx::Insert)
                .with(Ctx::TextInput)
                .with(pane)
                .with(Ctx::Editor),
            Mode::Visual => {
                let set = set.with(Ctx::Visual).with(pane);
                if self.focused_view().is_some() {
                    set.with(Ctx::TextView)
                } else {
                    set
                }
            }
            Mode::Normal => {
                let mut set = set.with(Ctx::Normal).with(pane);
                if self.focused_view().is_some() {
                    set = set.with(Ctx::TextView);
                }
                if self.focus == Focus::Response && self.preview_shown() {
                    set = set.with(Ctx::Preview);
                }
                if self.editable() {
                    set = set.with(Ctx::Editor);
                }
                if self.operator.is_some() {
                    set = set.with(Ctx::Operator);
                }
                set
            }
        }
    }

    /// Preview タブを表示していて、プレビューが使えるか。
    fn preview_shown(&self) -> bool {
        self.response_tab == ResponseTab::Preview && self.preview_unavailable.is_none()
    }

    /// フォーカス中の編集できるテキスト欄。
    fn focused_field(&self) -> Option<TextField> {
        match self.focus {
            Focus::Url => Some(TextField::Url),
            Focus::Request => match self.request_tab {
                RequestTab::Params => Some(TextField::Params),
                RequestTab::Headers => Some(TextField::Headers),
                RequestTab::Body => Some(TextField::Body),
                RequestTab::Curl => None,
            },
            _ => None,
        }
    }

    fn field_mut(&mut self, field: TextField) -> &mut TextEditor {
        match field {
            TextField::Url => &mut self.url,
            TextField::Params => &mut self.params,
            TextField::Headers => &mut self.headers,
            TextField::Body => &mut self.body,
        }
    }

    /// フォーカス中のテキスト欄を外部エディタで開く (lazygit の `e` と同じ考え方)。
    fn open_external(&mut self) -> Action {
        self.back_to_normal();
        let Some(field) = self.focused_field() else {
            self.status = t!(
                "Focus URL / Params / Headers / Body to open it in the editor",
                "URL / Params / Headers / Body を選んでからエディタで開いてください"
            )
            .to_string();
            return Action::None;
        };
        let text = self.field_mut(field).text();
        Action::EditText { field, text }
    }

    /// 外部エディタで編集した内容を書き戻す。`u` で編集前に戻せる。
    pub fn apply_external_edit(&mut self, field: TextField, text: &str) {
        // エディタは末尾に改行を足すことが多いので 1 つだけ取り除く
        let text = text
            .strip_suffix("\r\n")
            .or_else(|| text.strip_suffix('\n'))
            .unwrap_or(text);
        let ed = self.field_mut(field);
        if ed.text() == text {
            self.status = tf!("{f}: no changes", "{f}: 変更なし", f = field.name());
            return;
        }
        let (row, col) = ed.cursor();
        ed.checkpoint();
        ed.replace_all(text);
        ed.set_cursor(row, col);
        ed.clamp_normal();
        self.commit();
        self.sync_curl_view();
        self.status = tf!(
            "{f}: applied the edit (u to undo)",
            "{f}: 編集内容を反映しました (u で元に戻す)",
            f = field.name()
        );
    }

    /// フォーカス中のペインが編集できるテキスト欄か。
    fn editable(&self) -> bool {
        match self.focus {
            Focus::Url => true,
            Focus::Request => self.request_tab != RequestTab::Curl,
            _ => false,
        }
    }

    pub fn set_keymap(&mut self, keymap: Keymap) {
        self.keymap = keymap;
        self.clear_pending();
    }

    /// Vim 形式のマッピング (設定ファイルの "vimrc") を設定する。
    pub fn set_vimrc(&mut self, vimrc: Vimrc) {
        self.vimrc = vimrc;
        self.clear_typeahead();
    }

    fn clear_typeahead(&mut self) {
        self.typeahead.clear();
        self.waiting_since = None;
    }

    /// マッピングの続きを待つのをやめる時刻 (待っていなければ None)。
    pub fn key_deadline(&self) -> Option<Instant> {
        Some(self.waiting_since? + self.vimrc.timeout?)
    }

    /// 続きのキーが来なかった: 待っていたキーを、当てはまる短いマッピング
    /// (なければそのままのキー) として処理する。
    pub fn key_timeout(&mut self) -> Action {
        self.run_typeahead(true)
    }

    /// 処理を中断した (Action を返した) キーが残っているか。残っていれば
    /// [`App::resume_keys`] で続きを処理する。
    pub fn has_queued_keys(&self) -> bool {
        self.waiting_since.is_none() && !self.typeahead.is_empty()
    }

    pub fn resume_keys(&mut self) -> Action {
        self.run_typeahead(false)
    }

    /// いまのモードに対応する Vim の map モード。マッピングを使わないときは None。
    fn map_mode(&self) -> Option<MapMode> {
        if self.replace.is_some() {
            // r の後の文字はマッピングしない (Vim と同じ)
            return None;
        }
        match self.mode {
            Mode::Normal if self.operator.is_some() => Some(MapMode::OperatorPending),
            Mode::Normal => Some(MapMode::Normal),
            Mode::Editing => Some(MapMode::Insert),
            Mode::Visual => Some(MapMode::Visual),
            Mode::Command | Mode::Rename => Some(MapMode::CmdLine),
            Mode::Help => None,
        }
    }

    /// typeahead のキーを、マッピングを適用しながら順に処理する。main に返す Action が
    /// 出たらそこで止める (残りは [`App::resume_keys`] で処理する)。`flush` は
    /// タイムアウト: マッピングの続きを待たない。
    fn run_typeahead(&mut self, flush: bool) -> Action {
        let mut depth = 0;
        loop {
            if self.typeahead.front().is_some_and(|q| q.remap)
                && !self.vimrc.is_empty()
                && let Some(mode) = self.map_mode()
            {
                let seq: Vec<KeyStroke> = self
                    .typeahead
                    .iter()
                    .take_while(|q| q.remap)
                    .map(|q| KeyStroke::from_event(&q.event))
                    .collect();
                let found = self.vimrc.find(mode, &seq);
                // 続きのキーが来るかもしれない (typeahead の最後まで一致している)
                let can_grow = found.longer && seq.len() == self.typeahead.len();
                let nowait = found
                    .exact
                    .is_some_and(|m| m.nowait && m.lhs.len() == seq.len());
                if can_grow && !flush && !nowait {
                    self.waiting_since = Some(Instant::now());
                    return Action::None;
                }
                if let Some(m) = found.exact {
                    depth += 1;
                    if depth > MAX_MAP_DEPTH {
                        self.clear_typeahead();
                        self.status = t!(
                            "Recursive mapping (check :vimrc; use noremap)",
                            "マッピングが再帰しています (:vimrc を確認してください。noremap を使えます)"
                        )
                        .to_string();
                        return Action::None;
                    }
                    self.typeahead.drain(..m.lhs.len());
                    // 右辺が左辺で始まるときは、その最初のキーはマッピングしない (Vim と同じ)
                    let starts_with_lhs = m.rhs.starts_with(&m.lhs);
                    for (i, k) in m.rhs.iter().enumerate().rev() {
                        self.typeahead.push_front(Queued {
                            event: KeyEvent::new(k.code, k.mods),
                            remap: !m.noremap && !(i == 0 && starts_with_lhs),
                        });
                    }
                    continue;
                }
            }
            self.waiting_since = None;
            let Some(q) = self.typeahead.pop_front() else {
                return Action::None;
            };
            let action = self.process_key(q.event);
            self.sync_curl_view();
            let action = match (action, self.clipboard.take()) {
                (Action::None, Some(text)) => Action::Copy(text),
                (action, _) => action,
            };
            if matches!(
                action,
                Action::EditConfig | Action::EditVimrc | Action::EditText { .. }
            ) {
                // エディタを開くときは残りのキーを捨てる (設定や内容が変わるため)
                self.clear_typeahead();
            }
            if action != Action::None {
                return action;
            }
        }
    }

    fn clear_pending(&mut self) {
        self.keys.clear();
        self.operator = None;
        self.replace = None;
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        self.typeahead.push_back(Queued {
            event: key,
            remap: true,
        });
        self.run_typeahead(false)
    }

    fn process_key(&mut self, key: KeyEvent) -> Action {
        let stroke = KeyStroke::from_event(&key);
        // r の後の置換文字
        if self.replace.take().is_some() {
            if let KeyCode::Char(c) = stroke.code
                && !stroke.mods.contains(KeyModifiers::CONTROL)
            {
                self.replace_char(c);
                return Action::None;
            }
            // 文字以外は置換の取り消し。どこでも有効なキーだけは実行する
            return self.run_global(stroke);
        }
        self.keys.push(stroke);
        match self.keymap.lookup(self.contexts(), &self.keys) {
            Lookup::Pending => Action::None,
            Lookup::Found { command, global } => {
                let keys = std::mem::take(&mut self.keys);
                self.dispatch(command, global, &keys)
            }
            Lookup::None => {
                let keys = std::mem::take(&mut self.keys);
                self.unbound(&keys, key)
            }
        }
    }

    /// コンテキストなし (どこでも有効) のバインドだけを対象に 1 打鍵を実行する。
    fn run_global(&mut self, stroke: KeyStroke) -> Action {
        match self.keymap.lookup(self.contexts(), &[stroke]) {
            Lookup::Found {
                command,
                global: true,
            } => self.run(command, &[stroke]),
            _ => Action::None,
        }
    }

    /// 割り当てのないキー列。
    fn unbound(&mut self, keys: &[KeyStroke], key: KeyEvent) -> Action {
        let Some((last, head)) = keys.split_last() else {
            return Action::None;
        };
        match self.mode {
            Mode::Editing | Mode::Rename | Mode::Command => {
                if head.is_empty() {
                    if let KeyCode::Char(c) = key.code
                        && !key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        self.insert_char(c);
                    }
                    return Action::None;
                }
                // 途中まで押したキーは文字として入力し、最後のキーを改めて処理する
                for k in head {
                    if let KeyCode::Char(c) = k.code
                        && !k.mods.contains(KeyModifiers::CONTROL)
                    {
                        self.insert_char(c);
                    }
                }
                self.process_key(key)
            }
            Mode::Help => {
                self.mode = Mode::Normal;
                Action::None
            }
            Mode::Normal | Mode::Visual => {
                self.operator = None;
                if head.is_empty() {
                    Action::None
                } else {
                    // `g` などの後でも Ctrl+C などは効くようにする
                    self.run_global(*last)
                }
            }
        }
    }

    fn dispatch(&mut self, command: Command, global: bool, keys: &[KeyStroke]) -> Action {
        // オペレータ (d / c / y) の後はモーションか、同じオペレータ (dd) だけを受け付ける
        if !global && let Some((op, _)) = self.operator.take() {
            if let Some(motion) = motion_of(command) {
                self.operate(op, motion);
            } else if operator_of(command) == Some(op) {
                self.operate_lines(op, None);
            }
            return Action::None;
        }
        self.run(command, keys)
    }

    /// コマンドを実行する。`keys` はそのコマンドを呼び出したキー列。
    fn run(&mut self, command: Command, keys: &[KeyStroke]) -> Action {
        use Command as C;
        match command {
            C::AppQuit => return Action::Quit,
            C::AppSend => return self.send(),
            C::AppSave => return self.save(),
            C::AppSaveAndQuit => {
                self.commit();
                return Action::SaveAndQuit(self.collection.clone());
            }
            C::AppOpenConfig => {
                self.back_to_normal();
                return Action::EditConfig;
            }
            C::AppOpenVimrc => {
                self.back_to_normal();
                return Action::EditVimrc;
            }
            C::EditorOpenExternal => return self.open_external(),
            C::AppShowHelp => {
                self.back_to_normal();
                self.mode = Mode::Help;
            }
            C::AppOpenCommandLine => {
                self.back_to_normal();
                self.command = TextEditor::single_line("");
                self.mode = Mode::Command;
            }

            C::PaneFocusLeft => self.move_focus('h'),
            C::PaneFocusDown => self.move_focus('j'),
            C::PaneFocusUp => self.move_focus('k'),
            C::PaneFocusRight => self.move_focus('l'),
            C::PaneFocusNext => self.focus_pane(self.focus.next()),
            C::PaneFocusPrev => self.focus_pane(self.focus.prev()),
            C::PaneFocusCollection => self.focus_pane(Focus::Collection),
            C::PaneFocusUrl => self.focus_pane(Focus::Url),
            C::PaneFocusRequest => self.focus_pane(Focus::Request),
            C::PaneFocusResponse => self.focus_pane(Focus::Response),
            C::PaneToggleZoom => self.toggle_zoom(),

            C::RequestNextMethod => self.method = self.method.next(),
            C::RequestPrevMethod => self.method = self.method.prev(),

            C::TabNext => self.cycle_tab(1),
            C::TabPrev => self.cycle_tab(-1),
            C::TabSelect1 => self.select_tab(0),
            C::TabSelect2 => self.select_tab(1),
            C::TabSelect3 => self.select_tab(2),
            C::TabSelect4 => self.select_tab(3),

            C::CollectionSelectNext => self.select(self.selected + 1),
            C::CollectionSelectPrev => {
                if self.selected > 0 {
                    self.select(self.selected - 1);
                }
            }
            C::CollectionSelectFirst => self.select(0),
            C::CollectionSelectLast => self.select(self.collection.len() - 1),
            C::CollectionNewBelow => {
                self.commit();
                self.collection
                    .insert(self.selected + 1, Request::default());
                self.select(self.selected + 1);
                self.status =
                    t!("Added a new request", "新しいリクエストを追加しました").to_string();
            }
            C::CollectionNewAbove => {
                self.commit();
                self.collection.insert(self.selected, Request::default());
                // select() は同じ index だと何もしないので直接読み込む
                self.load_selected();
                self.status =
                    t!("Added a new request", "新しいリクエストを追加しました").to_string();
            }
            C::CollectionYank => {
                self.commit();
                self.request_register = Some(self.collection[self.selected].clone());
                self.status = tf!(
                    "Yanked \"{name}\" (p to paste)",
                    "「{name}」をヤンクしました (p で貼り付け)",
                    name = self.collection[self.selected].name
                );
            }
            C::CollectionDelete => {
                self.commit();
                let removed = self.collection.remove(self.selected);
                if self.collection.is_empty() {
                    self.collection.push(Request::default());
                }
                self.selected = self.selected.min(self.collection.len() - 1);
                self.load_selected();
                self.status = tf!(
                    "Deleted \"{name}\" (p to paste)",
                    "「{name}」を削除しました (p で貼り付け)",
                    name = removed.name
                );
                self.request_register = Some(removed);
            }
            C::CollectionPasteAfter | C::CollectionPasteBefore => {
                let Some(mut req) = self.request_register.clone() else {
                    self.status = t!(
                        "No yanked request (yy to yank)",
                        "ヤンクしたリクエストがありません (yy でヤンク)"
                    )
                    .to_string();
                    return Action::None;
                };
                self.commit();
                let at = if command == C::CollectionPasteAfter {
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
                self.status = t!("Pasted the request", "リクエストを貼り付けました").to_string();
            }
            C::CollectionRename => {
                self.back_to_normal();
                self.rename = TextEditor::single_line(&self.collection[self.selected].name);
                self.mode = Mode::Rename;
            }

            C::ResponseScrollDown => self.scroll_response_by(1),
            C::ResponseScrollUp => self.scroll_response_by(-1),
            C::ResponseHalfPageDown => self.scroll_response_by(self.page_size(true) as i32),
            C::ResponseHalfPageUp => self.scroll_response_by(-(self.page_size(true) as i32)),
            C::ResponsePageDown => self.scroll_response_by(self.page_size(false) as i32),
            C::ResponsePageUp => self.scroll_response_by(-(self.page_size(false) as i32)),
            // Body / Headers タブではカーソルごと移動する
            C::ResponseScrollToTop | C::ResponseScrollToBottom
                if self.response_view_id().is_some() =>
            {
                if self.focus == Focus::Response && self.mode != Mode::Editing {
                    self.move_cursor(if command == C::ResponseScrollToTop {
                        Motion::Top
                    } else {
                        Motion::Bottom
                    });
                }
            }
            C::ResponseScrollToTop => self.response_scroll = 0,
            C::ResponseScrollToBottom => self.response_scroll = self.max_scroll(),

            C::PreviewZoomIn | C::PreviewZoomOut | C::PreviewZoomReset => {
                if self.preview_shown() {
                    let step = PREVIEW_ZOOM_STEP as i32;
                    self.set_preview_zoom(match command {
                        C::PreviewZoomIn => self.preview_zoom as i32 + step,
                        C::PreviewZoomOut => self.preview_zoom as i32 - step,
                        _ => PREVIEW_ZOOM_DEFAULT as i32,
                    });
                }
            }

            C::VimMoveLeft
            | C::VimMoveRight
            | C::VimMoveUp
            | C::VimMoveDown
            | C::VimMoveWordForward
            | C::VimMoveWordBackward
            | C::VimMoveWordEnd
            | C::VimMoveToLineStart
            | C::VimMoveToFirstNonBlank
            | C::VimMoveToLineEnd
            | C::VimMoveToTop
            | C::VimMoveToBottom
            | C::VimInsert
            | C::VimAppend
            | C::VimInsertAtLineStart
            | C::VimAppendAtLineEnd
            | C::VimOpenLineBelow
            | C::VimOpenLineAbove
            | C::VimDeleteChar
            | C::VimDeleteCharBefore
            | C::VimDeleteToLineEnd
            | C::VimChangeToLineEnd
            | C::VimSubstituteChar
            | C::VimSubstituteLine
            | C::VimYankLine
            | C::VimPasteAfter
            | C::VimPasteBefore
            | C::VimUndo
            | C::VimReplaceChar
            | C::VimDelete
            | C::VimChange
            | C::VimYank
            | C::VimVisual
            | C::VimVisualLine
            | C::VimVisualSwapEnds => self.run_vim(command, keys),
            C::VimNormalMode => match self.mode {
                Mode::Editing => self.leave_insert(),
                Mode::Visual => self.mode = Mode::Normal,
                _ => {}
            },

            C::EditorBackspace
            | C::EditorDelete
            | C::EditorDeleteWordBefore
            | C::EditorMoveLeft
            | C::EditorMoveRight
            | C::EditorMoveUp
            | C::EditorMoveDown
            | C::EditorMoveToLineStart
            | C::EditorMoveToLineEnd
            | C::EditorKillToLineEnd
            | C::EditorClear
            | C::EditorNewline => self.run_input(command),

            C::CommandCancel => {
                if self.mode == Mode::Command {
                    self.mode = Mode::Normal;
                }
            }
            C::CommandExecute => {
                if self.mode == Mode::Command {
                    self.mode = Mode::Normal;
                    let cmd = self.command.text();
                    return self.run_command(cmd.trim());
                }
            }
            C::RenameCancel => {
                if self.mode == Mode::Rename {
                    self.mode = Mode::Normal;
                }
            }
            C::RenameConfirm => {
                if self.mode == Mode::Rename {
                    let name = self.rename.text().trim().to_string();
                    if !name.is_empty() {
                        self.collection[self.selected].name = name;
                    }
                    self.mode = Mode::Normal;
                }
            }
        }
        Action::None
    }

    fn send(&mut self) -> Action {
        if self.is_loading() {
            self.status = t!("Already sending…", "送信中です…").to_string();
            return Action::None;
        }
        if self.mode == Mode::Visual {
            self.mode = Mode::Normal;
        }
        self.commit();
        let req = self.current_request();
        if let Err(e) = req.full_url() {
            self.status = e.to_string();
            return Action::None;
        }
        self.response = ResponseState::Loading;
        self.response_scroll = 0;
        self.status = tf!(
            "Sending {method} {url}…",
            "{method} {url} を送信中…",
            method = req.method,
            url = req.url
        );
        Action::Send(req)
    }

    fn save(&mut self) -> Action {
        self.commit();
        Action::Save(self.collection.clone())
    }

    /// 文字入力中のエディタ (Insert ならテキスト欄、Command / Rename ならその入力欄)。
    fn input_editor(&mut self) -> Option<&mut TextEditor> {
        match self.mode {
            Mode::Editing => self.active_editor(),
            Mode::Rename => Some(&mut self.rename),
            Mode::Command => Some(&mut self.command),
            Mode::Normal | Mode::Visual | Mode::Help => None,
        }
    }

    fn insert_char(&mut self, c: char) {
        match self.input_editor() {
            Some(ed) => ed.insert_char(c),
            None => self.mode = Mode::Normal,
        }
    }

    /// 文字入力中の編集操作 (Insert モード / コマンドライン / 名前変更)。
    fn run_input(&mut self, command: Command) {
        use Command as C;
        // 空のコマンドラインでの Backspace はキャンセル
        if self.mode == Mode::Command && command == C::EditorBackspace && self.command.is_empty() {
            self.mode = Mode::Normal;
            return;
        }
        let Some(ed) = self.input_editor() else {
            return;
        };
        match command {
            C::EditorBackspace => ed.backspace(),
            C::EditorDelete => ed.delete(),
            C::EditorDeleteWordBefore => ed.delete_word_before(),
            C::EditorMoveLeft => ed.move_left(),
            C::EditorMoveRight => ed.move_right(),
            C::EditorMoveUp => ed.move_up(),
            C::EditorMoveDown => ed.move_down(),
            C::EditorMoveToLineStart => ed.move_home(),
            C::EditorMoveToLineEnd => ed.move_end(),
            C::EditorKillToLineEnd => ed.kill_to_end(),
            C::EditorClear => ed.clear(),
            C::EditorNewline => ed.insert_newline(),
            _ => {}
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

    /// どのモードからでも Normal に戻す。
    fn back_to_normal(&mut self) {
        if self.mode == Mode::Editing {
            self.leave_insert();
        } else {
            self.mode = Mode::Normal;
        }
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

    fn toggle_zoom(&mut self) {
        self.zoomed = !self.zoomed;
        self.status = if self.zoomed {
            t!(
                "Zoomed (Shift+Esc / Ctrl+W z to restore)",
                "全画面表示 (Shift+Esc / Ctrl+W z で戻る)"
            )
        } else {
            t!("Zoom restored", "全画面表示を解除しました")
        }
        .to_string();
    }

    fn set_focus(&mut self, focus: Focus) {
        self.focus = focus;
        if focus != Focus::Collection {
            self.last_right_focus = focus;
        }
    }

    /// キー操作でのペイン移動。Insert 中なら Normal に戻してから移動する。
    fn focus_pane(&mut self, focus: Focus) {
        match self.mode {
            Mode::Editing => self.leave_insert(),
            Mode::Normal => {}
            Mode::Visual | Mode::Command | Mode::Rename | Mode::Help => return,
        }
        self.set_focus(focus);
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
            self.focus_pane(f);
        }
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
            "config" | "keymap" => Action::EditConfig,
            "vimrc" => Action::EditVimrc,
            "e" | "edit" => self.open_external(),
            _ => {
                self.status = tf!("Unknown command: :{cmd}", "不明なコマンド: :{cmd}");
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

    /// フォーカス中のペインの `index` 番目 (0 始まり) のタブを選ぶ。
    fn select_tab(&mut self, index: usize) {
        match self.focus {
            Focus::Request => {
                if let Some(tab) = RequestTab::ALL.get(index) {
                    self.request_tab = *tab;
                }
            }
            Focus::Response => {
                let tabs = [
                    ResponseTab::Body,
                    ResponseTab::Headers,
                    ResponseTab::Preview,
                ];
                if let Some(tab) = tabs.get(index) {
                    self.set_response_tab(*tab);
                }
            }
            _ => {}
        }
    }

    fn scroll_response_by(&mut self, delta: i32) {
        if let Some(id) = self.response_view_id() {
            self.scroll_view(id, delta);
            return;
        }
        let next = (self.response_scroll as i32 + delta).clamp(0, self.max_scroll() as i32);
        self.response_scroll = next as u16;
    }

    /// Visual モードに入る (`v` / `V`)。
    fn enter_visual(&mut self, linewise: bool) {
        if let Some(ed) = self.active_text() {
            let anchor = ed.cursor();
            self.visual = Visual { anchor, linewise };
            self.mode = Mode::Visual;
        }
    }

    /// 選択範囲をヤンクしてクリップボードにコピーし、Normal モードに戻る。
    fn yank_selection(&mut self) {
        let selection = self.selection();
        self.mode = Mode::Normal;
        let Some(sel) = selection else {
            return;
        };
        let Some(ed) = self.active_text() else {
            return;
        };
        let text = if sel.linewise {
            ed.lines_text(sel.start.0, sel.end.0)
        } else {
            let end = (sel.end.0, (sel.end.1 + 1).min(ed.line_len(sel.end.0)));
            ed.text_range(sel.start, end)
        };
        ed.set_cursor(sel.start.0, sel.start.1);
        ed.clamp_normal();
        self.register = Register {
            text,
            linewise: sel.linewise,
        };
        self.follow_cursor();
        self.copy_register();
    }

    /// ヤンクした内容をクリップボードにコピーする (実際の書き込みは main が行う)。
    fn copy_register(&mut self) {
        let text = &self.register.text;
        if text.is_empty() {
            self.status = t!("Nothing to copy", "コピーする文字がありません").to_string();
            return;
        }
        self.status = if self.register.linewise {
            let n = text.split('\n').count();
            let s = if n == 1 { "" } else { "s" };
            tf!(
                "Copied {n} line{s} to the clipboard",
                "{n} 行をクリップボードにコピーしました"
            )
        } else {
            let n = text.chars().count();
            let s = if n == 1 { "" } else { "s" };
            tf!(
                "Copied {n} character{s} to the clipboard",
                "{n} 文字をクリップボードにコピーしました"
            )
        };
        self.clipboard = Some(text.clone());
    }

    /// カーソルを動かす (読み取り専用ビューではスクロールも合わせる)。
    fn move_cursor(&mut self, motion: Motion) {
        if let Some(ed) = self.active_text() {
            apply_motion(ed, motion);
        }
        self.follow_cursor();
    }

    /// オペレータの対象。ヤンクは読み取り専用ビューでもできる。
    fn operand(&mut self, op: Operator) -> Option<&mut TextEditor> {
        match op {
            Operator::Yank => self.active_text(),
            Operator::Delete | Operator::Change => self.active_editor(),
        }
    }

    /// Visual モードでのコマンド。カーソル移動で範囲を決め、ヤンクでコピーする。
    /// 文字列を変えるコマンドは受け付けない。
    fn run_visual(&mut self, command: Command) {
        use Command as C;
        if self.active_text().is_none() {
            self.mode = Mode::Normal;
            return;
        }
        if let Some(motion) = motion_of(command) {
            self.move_cursor(motion);
            return;
        }
        match command {
            C::VimVisual if self.visual.linewise => self.visual.linewise = false,
            C::VimVisualLine if !self.visual.linewise => self.visual.linewise = true,
            C::VimVisual | C::VimVisualLine => self.mode = Mode::Normal,
            // 選択の反対側の端にカーソルを移す
            C::VimVisualSwapEnds => {
                let anchor = self.visual.anchor;
                if let Some(ed) = self.active_text() {
                    let cursor = ed.cursor();
                    ed.set_cursor(anchor.0, anchor.1);
                    ed.clamp_normal();
                    self.visual.anchor = cursor;
                }
                self.follow_cursor();
            }
            C::VimYank => self.yank_selection(),
            _ => {}
        }
    }

    /// テキスト欄 (URL / Params / Headers / Body) と読み取り専用ビューでの Normal モード操作。
    fn run_vim(&mut self, command: Command, keys: &[KeyStroke]) {
        use Command as C;
        if self.mode == Mode::Visual {
            self.run_visual(command);
            return;
        }
        if self.mode != Mode::Normal {
            return;
        }
        if let Some(motion) = motion_of(command) {
            self.move_cursor(motion);
            return;
        }
        let shown: String = keys.iter().map(|k| k.short()).collect();
        match command {
            C::VimVisual | C::VimVisualLine => {
                self.enter_visual(command == C::VimVisualLine);
                return;
            }
            C::VimYankLine => {
                self.operate_lines(Operator::Yank, None);
                return;
            }
            _ => {}
        }
        // 読み取り専用ビュー (cURL / Response) ではヤンクだけ。文字列を変えるコマンドは無視する
        if self.focused_view().is_some() {
            if command == C::VimYank {
                self.operator = Some((Operator::Yank, shown));
            }
            return;
        }
        let reg = self.register.clone();
        let Some(ed) = self.active_editor() else {
            return;
        };
        match command {
            C::VimInsert => {}
            C::VimAppend => {
                if ed.line_len(ed.cursor().0) > 0 {
                    ed.move_right();
                }
            }
            C::VimInsertAtLineStart => {
                let row = ed.cursor().0;
                let col = ed.first_non_blank_col(row);
                ed.set_cursor(row, col);
            }
            C::VimAppendAtLineEnd => ed.move_end(),
            C::VimOpenLineBelow | C::VimOpenLineAbove => {
                ed.checkpoint();
                if ed.is_multiline() {
                    let row = ed.cursor().0;
                    ed.insert_lines(row, command == C::VimOpenLineAbove, "");
                } else {
                    ed.move_end();
                }
                self.enter_insert(false);
                return;
            }
            C::VimDeleteChar => {
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
            C::VimDeleteCharBefore => {
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
            C::VimDeleteToLineEnd => {
                self.operate(Operator::Delete, Motion::LineEnd);
                return;
            }
            C::VimChangeToLineEnd => {
                self.operate(Operator::Change, Motion::LineEnd);
                return;
            }
            C::VimSubstituteChar => {
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
            C::VimSubstituteLine => {
                ed.checkpoint();
                let text = ed.clear_line();
                self.register = Register {
                    text,
                    linewise: true,
                };
                self.enter_insert(false);
                return;
            }
            C::VimPasteAfter | C::VimPasteBefore => {
                let before = command == C::VimPasteBefore;
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
            C::VimUndo => {
                if !ed.undo() {
                    self.status = t!("Nothing more to undo", "これ以上元に戻せません").to_string();
                }
                self.commit();
                return;
            }
            C::VimReplaceChar => {
                self.replace = Some(shown);
                return;
            }
            C::VimDelete | C::VimChange | C::VimYank => {
                self.operator = operator_of(command).map(|op| (op, shown));
                return;
            }
            _ => return,
        }
        // ここに来るのは Insert モードに入るコマンド (i / a / I / A)
        self.enter_insert(true);
    }

    /// `r` の後に押された文字で、カーソル位置の 1 文字を置き換える。
    fn replace_char(&mut self, c: char) {
        if let Some(ed) = self.active_editor() {
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

    /// 行単位の操作 (dd / cc / yy、または j / k / gg / G と組み合わせたとき)。
    /// `to` が None なら現在行のみ。
    fn operate_lines(&mut self, op: Operator, to: Option<usize>) {
        let Some(ed) = self.operand(op) else {
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
        match op {
            Operator::Change => self.enter_insert(false),
            Operator::Delete => self.commit(),
            Operator::Yank => {
                self.follow_cursor();
                self.copy_register();
            }
        }
    }

    /// オペレータ + モーション (dw / c$ / ye など)。
    fn operate(&mut self, op: Operator, motion: Motion) {
        let Some(ed) = self.operand(op) else {
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
            Operator::Delete => {
                if let Some(ed) = self.active_editor() {
                    ed.clamp_normal();
                }
                self.commit();
            }
            Operator::Yank => {
                if let Some(ed) = self.active_text() {
                    ed.clamp_normal();
                }
                self.follow_cursor();
                self.copy_register();
            }
        }
    }

    /// ステータス行に表示する入力途中のキー。
    pub fn pending_keys(&self) -> String {
        let mut shown = String::new();
        if let Some((_, keys)) = &self.operator {
            shown.push_str(keys);
        }
        if let Some(keys) = &self.replace {
            shown.push_str(keys);
        }
        for key in &self.keys {
            shown.push_str(&key.short());
        }
        if self.waiting_since.is_some() {
            for q in &self.typeahead {
                shown.push_str(&KeyStroke::from_event(&q.event).short());
            }
        }
        shown
    }

    /// Preview タブでスクロールできる行数 (画像は最後まで表示したところで止める)。
    fn max_scroll(&self) -> u16 {
        match (&self.response, self.response_tab, &self.preview) {
            (ResponseState::Done(_), ResponseTab::Preview, PreviewState::Ready { rows, .. }) => {
                rows.saturating_sub(self.response_height.get())
            }
            _ => 0,
        }
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

/// モーションのコマンドなら、その移動。
fn motion_of(command: Command) -> Option<Motion> {
    Some(match command {
        Command::VimMoveLeft => Motion::Left,
        Command::VimMoveRight => Motion::Right,
        Command::VimMoveUp => Motion::Up,
        Command::VimMoveDown => Motion::Down,
        Command::VimMoveWordForward => Motion::WordForward,
        Command::VimMoveWordBackward => Motion::WordBackward,
        Command::VimMoveWordEnd => Motion::WordEnd,
        Command::VimMoveToLineStart => Motion::LineStart,
        Command::VimMoveToFirstNonBlank => Motion::FirstNonBlank,
        Command::VimMoveToLineEnd => Motion::LineEnd,
        Command::VimMoveToTop => Motion::Top,
        Command::VimMoveToBottom => Motion::Bottom,
        _ => return None,
    })
}

fn operator_of(command: Command) -> Option<Operator> {
    match command {
        Command::VimDelete => Some(Operator::Delete),
        Command::VimChange => Some(Operator::Change),
        Command::VimYank => Some(Operator::Yank),
        _ => None,
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
    fn shift_esc_toggles_zoom_in_any_mode() {
        let shift_esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::SHIFT);
        let mut app = App::new(vec![]);
        assert!(!app.zoomed);
        app.handle_key(shift_esc);
        assert!(app.zoomed);
        // 素の Esc では解除されない
        app.handle_key(key(KeyCode::Esc));
        assert!(app.zoomed);
        // Insert 中でもモードを変えずに切り替わる
        app.handle_key(key(KeyCode::Char('i')));
        app.handle_key(shift_esc);
        assert!(!app.zoomed);
        assert_eq!(app.mode, Mode::Editing);
        app.handle_key(key(KeyCode::Esc));
        // Shift+Esc が届かない端末向けの Ctrl+W z
        app.handle_key(ctrl('w'));
        app.handle_key(key(KeyCode::Char('z')));
        assert!(app.zoomed);
        // 全画面のままペインを移動できる
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Request);
        assert!(app.zoomed);
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
    fn preview_zoom_keys_rerender() {
        let mut app = App::new(vec![]);
        app.focus = Focus::Response;
        app.set_response(Ok(html_response()));
        // Preview タブ以外では効かない
        app.handle_key(key(KeyCode::Char('+')));
        assert_eq!(app.preview_zoom, 100);

        app.handle_key(key(KeyCode::Char('3')));
        app.preview = PreviewState::Ready { cols: 80, rows: 25 };
        app.response_scroll = 5;
        let generation = app.preview_generation;
        app.handle_key(key(KeyCode::Char('+')));
        assert_eq!(app.preview_zoom, 125);
        assert_eq!(app.preview, PreviewState::Idle);
        assert_eq!(app.response_scroll, 0);
        assert!(app.preview_generation > generation);
        assert!(app.status.contains("125%"));

        app.handle_key(key(KeyCode::Char('-')));
        app.handle_key(key(KeyCode::Char('-')));
        assert_eq!(app.preview_zoom, 75);
        app.handle_key(key(KeyCode::Char('0')));
        assert_eq!(app.preview_zoom, 100);

        // 範囲外には出ない
        for _ in 0..30 {
            app.handle_key(key(KeyCode::Char('-')));
        }
        assert_eq!(app.preview_zoom, PREVIEW_ZOOM_MIN);
        for _ in 0..30 {
            app.handle_key(key(KeyCode::Char('=')));
        }
        assert_eq!(app.preview_zoom, PREVIEW_ZOOM_MAX);
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
        assert!(app.status.contains("Unknown command"));

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

    fn app_with_config(config: &str) -> App {
        let mut app = App::new(vec![Request {
            url: "http://a".into(),
            ..Default::default()
        }]);
        app.set_keymap(crate::config::parse(config).unwrap().keymap);
        app
    }

    #[test]
    fn config_command_requests_editor() {
        let mut app = App::new(vec![]);
        type_str(&mut app, ":config");
        assert_eq!(app.handle_key(key(KeyCode::Enter)), Action::EditConfig);
        assert_eq!(app.mode, Mode::Normal);
        type_str(&mut app, ":vimrc");
        assert_eq!(app.handle_key(key(KeyCode::Enter)), Action::EditVimrc);
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn ctrl_e_opens_focused_field_in_external_editor() {
        let mut app = App::new(vec![Request {
            url: "http://a".into(),
            body: "{\"a\":1}".into(),
            ..Default::default()
        }]);
        assert_eq!(
            app.handle_key(ctrl('e')),
            Action::EditText {
                field: TextField::Url,
                text: "http://a".into()
            }
        );
        app.focus = Focus::Request;
        app.request_tab = RequestTab::Body;
        assert_eq!(
            app.handle_key(ctrl('e')),
            Action::EditText {
                field: TextField::Body,
                text: "{\"a\":1}".into()
            }
        );
        // Insert モードからでも :edit で開ける (Normal に戻る)
        type_str(&mut app, "i");
        assert_eq!(app.mode, Mode::Editing);
        app.handle_key(key(KeyCode::Esc));
        type_str(&mut app, ":edit");
        assert!(matches!(
            app.handle_key(key(KeyCode::Enter)),
            Action::EditText {
                field: TextField::Body,
                ..
            }
        ));
        // cURL タブは読み取り専用なので開かない
        app.request_tab = RequestTab::Curl;
        type_str(&mut app, ":e");
        assert_eq!(app.handle_key(key(KeyCode::Enter)), Action::None);
    }

    #[test]
    fn external_edit_is_applied_and_undoable() {
        let mut app = App::new(vec![Request {
            url: "http://a".into(),
            ..Default::default()
        }]);
        app.focus = Focus::Request;
        app.request_tab = RequestTab::Headers;
        app.apply_external_edit(TextField::Headers, "Accept: */*\n# X-Debug: 1\n");
        // エディタが足した末尾の改行は 1 つだけ取り除く
        assert_eq!(app.headers.text(), "Accept: */*\n# X-Debug: 1");
        let req = &app.collection[0];
        assert_eq!(req.headers.len(), 2);
        assert!(!req.headers[1].enabled);
        assert!(app.curl_text.contains("Accept: */*"));
        type_str(&mut app, "u");
        assert_eq!(app.headers.text(), "");

        app.apply_external_edit(TextField::Url, "http://b\n");
        assert_eq!(app.url.text(), "http://b");
        assert_eq!(app.current_request().url, "http://b");
    }

    #[test]
    fn custom_bindings_add_override_and_disable() {
        let mut app = app_with_config(
            r#"{ "keymap": [
                { "context": "Normal", "bindings": {
                    "q": null,
                    "space w": "app::Save",
                    "ctrl-g": "app::Send",
                    "f2": "app::OpenConfig"
                } },
                { "context": "Normal && Collection", "bindings": { "x": "collection::Delete" } }
            ] }"#,
        );
        assert_eq!(app.handle_key(key(KeyCode::Char('q'))), Action::None);
        // 既定のキーは残る
        assert_eq!(app.handle_key(ctrl('c')), Action::Quit);
        app.handle_key(key(KeyCode::Char(' ')));
        assert_eq!(app.pending_keys(), "<space>");
        assert!(matches!(
            app.handle_key(key(KeyCode::Char('w'))),
            Action::Save(_)
        ));
        // Space は右移動 (既定) ではなくキー列の 1 打鍵目として使われた
        assert_eq!(app.url.cursor(), (0, 7));
        assert_eq!(app.handle_key(key(KeyCode::F(2))), Action::EditConfig);
        assert!(matches!(app.handle_key(ctrl('g')), Action::Send(_)));

        // コンテキストが合うときだけ効く: URL では x は 1 文字削除のまま
        app.handle_key(key(KeyCode::Char('x')));
        assert_eq!(app.url.text(), "http://");
        assert_eq!(app.collection.len(), 1);
        app.handle_key(ctrl('h'));
        app.handle_key(key(KeyCode::Char('o')));
        assert_eq!(app.collection.len(), 2);
        app.handle_key(key(KeyCode::Char('x')));
        assert_eq!(app.collection.len(), 1);
    }

    #[test]
    fn custom_sequence_in_insert_mode() {
        let mut app = app_with_config(
            r#"{ "keymap": [ { "context": "Insert", "bindings": { "j k": "vim::NormalMode" } } ] }"#,
        );
        app.handle_key(key(KeyCode::Char('A')));
        type_str(&mut app, "jxjjk");
        // 続きが来なかった j は普通の文字として入力される
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.url.text(), "http://ajxj");
    }

    fn app_with_vimrc(lines: &[&str]) -> App {
        let mut app = app_with_config("{}");
        app.set_vimrc(crate::vimrc::parse(lines).unwrap());
        app
    }

    #[test]
    fn vimrc_imap_jj_to_escape() {
        let mut app = app_with_vimrc(&["imap jj <Esc>"]);
        type_str(&mut app, "A");
        type_str(&mut app, "x");
        assert_eq!(app.url.text(), "http://ax");
        // 1 つ目の j は続きを待つ
        type_str(&mut app, "j");
        assert_eq!(app.url.text(), "http://ax");
        assert_eq!(app.pending_keys(), "j");
        assert!(app.key_deadline().is_some());
        type_str(&mut app, "j");
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.url.text(), "http://ax");
        assert_eq!(app.key_deadline(), None);

        // 続きが別のキーなら j はそのまま入力される
        type_str(&mut app, "Ajkj");
        assert_eq!(app.url.text(), "http://axjk");
        assert_eq!(app.mode, Mode::Editing);
        // タイムアウトでも j として入力される
        assert_eq!(app.key_timeout(), Action::None);
        assert_eq!(app.url.text(), "http://axjkj");
        assert_eq!(app.pending_keys(), "");
        // Normal モードでは j はマッピングされない
        app.handle_key(key(KeyCode::Esc));
        type_str(&mut app, "jj");
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.key_deadline(), None);
    }

    #[test]
    fn vimrc_leader_runs_command_line() {
        let mut app = app_with_vimrc(&[
            "let mapleader = ' '",
            "nnoremap <Leader>w :w<CR>",
            "nnoremap <Leader>q :wq<CR>",
            "nnoremap q <Nop>",
        ]);
        app.handle_key(key(KeyCode::Char(' ')));
        assert_eq!(app.pending_keys(), "<space>");
        assert!(matches!(
            app.handle_key(key(KeyCode::Char('w'))),
            Action::Save(_)
        ));
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.handle_key(key(KeyCode::Char('q'))), Action::None);
        // Space の後のタイムアウトは既定の Space (右へ) として動く
        type_str(&mut app, "0 ");
        app.key_timeout();
        assert_eq!(app.url.cursor(), (0, 1));
        app.handle_key(key(KeyCode::Char(' ')));
        assert!(matches!(
            app.handle_key(key(KeyCode::Char('q'))),
            Action::SaveAndQuit(_)
        ));
    }

    #[test]
    fn vimrc_recursive_and_noremap() {
        // map は展開後のキーにもマッピングを適用し、noremap は適用しない
        let mut app = app_with_vimrc(&["nmap X Y", "nmap Y $", "nnoremap Z X"]);
        type_str(&mut app, "X");
        assert_eq!(app.url.cursor(), (0, 7));
        type_str(&mut app, "0Z");
        // Z → X (マッピングしない) → 既定の X (前の 1 文字を削除) は行頭なので何もしない
        assert_eq!(app.url.cursor(), (0, 0));
        assert_eq!(app.url.text(), "http://a");

        // 右辺が左辺で始まるときは、最初のキーを再びマッピングしない
        let mut app = app_with_vimrc(&["nmap x xl"]);
        type_str(&mut app, "0x");
        assert_eq!(app.url.text(), "ttp://a");

        // 無限に再帰するマッピングは止める
        let mut app = app_with_vimrc(&["nmap a b", "nmap b a"]);
        type_str(&mut app, "a");
        assert!(app.status.contains("Recursive"), "{}", app.status);
        assert!(!app.has_queued_keys());
    }

    #[test]
    fn vimrc_multiple_actions_are_resumed() {
        // 1 つのマッピングで保存と送信: 最初の Action で止まり、残りは resume_keys で
        let mut app = app_with_vimrc(&["nnoremap S :w<CR><C-r>"]);
        assert!(matches!(
            app.handle_key(key(KeyCode::Char('S'))),
            Action::Save(_)
        ));
        assert!(app.has_queued_keys());
        assert!(matches!(app.resume_keys(), Action::Send(_)));
        assert!(!app.has_queued_keys());
    }

    #[test]
    fn vimrc_modes() {
        let mut app = app_with_vimrc(&[
            "vnoremap Y y",
            "onoremap L $",
            "cnoremap <C-a> config",
            "inoremap <C-l> <Right>",
            "set notimeout",
        ]);
        // オペレータの後: dL で行末まで削除
        type_str(&mut app, "0wdL");
        assert_eq!(app.url.text(), "http");
        // : コマンドライン
        type_str(&mut app, ":");
        app.handle_key(ctrl('a'));
        assert_eq!(app.handle_key(key(KeyCode::Enter)), Action::EditConfig);
        // Visual
        type_str(&mut app, "0vl");
        assert_eq!(copied(app.handle_key(key(KeyCode::Char('Y')))), "ht");
        // Insert
        type_str(&mut app, "0i");
        app.handle_key(ctrl('l'));
        type_str(&mut app, "X");
        assert_eq!(app.url.text(), "hXttp");
        // notimeout: 待ち時間なし (続きのキーが無いマッピングでは待たない)
        assert_eq!(app.key_deadline(), None);
    }

    #[test]
    fn operators_follow_rebound_keys() {
        // d を無効にして x をオペレータに、L を行末へのモーションにする
        let mut app = app_with_config(
            r#"{ "keymap": [ { "context": "Normal && Editor", "bindings": {
                "d": null,
                "x": "vim::Delete",
                "shift-l": "vim::MoveToLineEnd"
            } } ] }"#,
        );
        type_str(&mut app, "0wd");
        assert_eq!(app.pending_keys(), "");
        type_str(&mut app, "x");
        assert_eq!(app.pending_keys(), "x");
        type_str(&mut app, "L");
        assert_eq!(app.url.text(), "http");
        type_str(&mut app, "xx");
        assert_eq!(app.url.text(), "");
        // g の途中でも Ctrl+C は効く
        type_str(&mut app, "g");
        assert_eq!(app.handle_key(ctrl('c')), Action::Quit);
    }

    fn copied(action: Action) -> String {
        match action {
            Action::Copy(text) => text,
            other => panic!("Copy ではない: {other:?}"),
        }
    }

    #[test]
    fn visual_yank_in_response_copies_to_clipboard() {
        let mut app = App::new(vec![]);
        app.set_response(Ok(sample_response("hello world\nsecond line")));
        app.focus = Focus::Response;
        assert_eq!(app.response_body_view.cursor(), (0, 0));

        // 文字単位: w で world の先頭、v e で world を選択
        type_str(&mut app, "wve");
        assert_eq!(app.mode, Mode::Visual);
        let sel = app.selection().unwrap();
        assert_eq!((sel.start, sel.end, sel.linewise), ((0, 6), (0, 10), false));
        assert!(sel.contains(0, 6) && sel.contains(0, 10) && !sel.contains(0, 5));
        let action = app.handle_key(key(KeyCode::Char('y')));
        assert_eq!(copied(action), "world");
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.response_body_view.cursor(), (0, 6));
        assert!(app.status.contains("5 characters"));

        // 行単位: V j で 2 行
        type_str(&mut app, "Vj");
        assert!(app.selection().unwrap().linewise);
        let action = app.handle_key(key(KeyCode::Char('y')));
        assert_eq!(copied(action), "hello world\nsecond line");

        // 行をまたぐ文字単位の選択と、o で反対側の端へ
        type_str(&mut app, "ggwvj0o");
        assert_eq!(app.response_body_view.cursor(), (0, 6));
        let action = app.handle_key(key(KeyCode::Char('y')));
        assert_eq!(copied(action), "world\ns");

        // Esc や v で選択を解除できる
        type_str(&mut app, "v");
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Normal);
        type_str(&mut app, "vv");
        assert_eq!(app.mode, Mode::Normal);
        type_str(&mut app, "vV");
        assert!(app.selection().unwrap().linewise);
        type_str(&mut app, "V");
        assert_eq!(app.mode, Mode::Normal);

        // Headers タブ: yy / yw などのヤンクもクリップボードに入る
        app.handle_key(key(KeyCode::Char('L')));
        type_str(&mut app, "y");
        let action = app.handle_key(key(KeyCode::Char('y')));
        assert_eq!(copied(action), "content-type: application/json");
        type_str(&mut app, "y");
        let action = app.handle_key(key(KeyCode::Char('e')));
        assert_eq!(copied(action), "content");
    }

    #[test]
    fn read_only_views_cannot_be_modified() {
        let mut app = App::new(vec![]);
        app.set_response(Ok(sample_response("hello world\nsecond line")));
        let curl = app.curl_view.text();
        assert!(curl.contains("'https://httpbin.org/get'"));

        for (focus, tab) in [
            (Focus::Response, '1'),
            (Focus::Response, '2'),
            (Focus::Request, '4'),
        ] {
            app.focus = focus;
            app.handle_key(key(KeyCode::Char(tab)));
            // Normal モードの編集キー、Visual モード中の編集キーのどちらも効かない
            for keys in [
                "x", "X", "dd", "dw", "D", "cc", "cw", "C", "s", "S", "rZ", "p", "P",
            ] {
                type_str(&mut app, keys);
                assert_eq!(app.mode, Mode::Normal, "{keys}");
            }
            for keys in ["i", "a", "I", "A", "o", "O"] {
                type_str(&mut app, keys);
                type_str(&mut app, "zz");
                assert_eq!(app.mode, Mode::Normal, "{keys}");
            }
            for keys in ["x", "d", "c", "s", "rZ", "p", "i", "A", "J"] {
                type_str(&mut app, "v");
                type_str(&mut app, keys);
                assert_eq!(app.mode, Mode::Visual, "{keys}");
                app.handle_key(key(KeyCode::Esc));
            }
            app.handle_event(Event::Paste("pasted".into()));
            assert_eq!(app.response_body_view.text(), "hello world\nsecond line");
            assert_eq!(
                app.response_headers_view.text(),
                "content-type: application/json"
            );
            assert_eq!(app.curl_view.text(), curl);
        }
        assert_eq!(app.url.text(), "https://httpbin.org/get");
        assert_eq!(app.body.text(), "");
    }

    #[test]
    fn curl_tab_selects_and_copies_whole_command() {
        let mut app = App::new(vec![Request {
            url: "http://a".into(),
            headers: vec![KeyValue::new("X", "1")],
            ..Default::default()
        }]);
        app.focus = Focus::Request;
        app.handle_key(key(KeyCode::Char('4')));
        let curl = curl::to_curl(&app.current_request());
        assert!(curl.lines().count() > 1);
        type_str(&mut app, "ggVG");
        let action = app.handle_key(key(KeyCode::Char('y')));
        assert_eq!(copied(action), curl);

        // リクエストが変わると表示も追従し、カーソルは先頭に戻る
        type_str(&mut app, "G");
        app.handle_key(key(KeyCode::Char('m')));
        assert!(app.curl_view.text().contains("-X POST"));
        assert_eq!(app.curl_view.cursor(), (0, 0));
    }

    #[test]
    fn visual_yank_in_editable_field_keeps_text() {
        let mut app = App::new(vec![]);
        type_str(&mut app, "0vllll");
        // Visual モードでは削除や挿入はできない
        type_str(&mut app, "xdci");
        assert_eq!(app.mode, Mode::Visual);
        let action = app.handle_key(key(KeyCode::Char('y')));
        assert_eq!(copied(action), "https");
        assert_eq!(app.url.text(), "https://httpbin.org/get");
        assert_eq!(app.register.text, "https");
        // Normal モードの yw もコピーする (dw はコピーしない)
        type_str(&mut app, "y");
        assert_eq!(copied(app.handle_key(key(KeyCode::Char('w')))), "https");
        type_str(&mut app, "d");
        assert_eq!(app.handle_key(key(KeyCode::Char('w'))), Action::None);
    }

    #[test]
    fn visual_mode_ends_when_response_is_replaced() {
        let mut app = App::new(vec![]);
        app.set_response(Ok(sample_response("abc")));
        app.focus = Focus::Response;
        type_str(&mut app, "v");
        assert_eq!(app.mode, Mode::Visual);
        app.set_response(Ok(sample_response("xyz")));
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.selection(), None);
        // レスポンスが無いときは Visual モードに入れない
        app.set_response(Err("boom".into()));
        type_str(&mut app, "v");
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn user_bindings_cannot_edit_read_only_views() {
        use crate::keymap::Section;
        // 読み取り専用ビューや Visual モードに編集コマンドを割り当てても効かない
        let bindings = [
            "vim::DeleteChar",
            "vim::Delete",
            "vim::PasteAfter",
            "vim::Insert",
        ]
        .iter()
        .zip(["x", "d", "p", "i"])
        .map(|(command, key)| (key.to_string(), Some(command.to_string())))
        .collect::<Vec<_>>();
        let keymap = Keymap::with_user(&[
            Section {
                context: "Normal && TextView".into(),
                bindings: bindings.clone(),
            },
            Section {
                context: "Visual".into(),
                bindings,
            },
        ])
        .unwrap();
        let mut app = App::new(vec![]);
        app.set_keymap(keymap);
        app.set_response(Ok(sample_response("hello world")));
        app.focus = Focus::Response;
        app.register = Register {
            text: "zzz".into(),
            linewise: false,
        };
        type_str(&mut app, "xdwpi");
        assert_eq!(app.mode, Mode::Normal);
        type_str(&mut app, "vexdpi");
        assert_eq!(app.mode, Mode::Visual);
        assert_eq!(app.response_body_view.text(), "hello world");
    }
}
