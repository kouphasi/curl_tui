//! アプリケーション状態とキー入力処理。
//!
//! I/O を持たない純粋な状態機械にしておき、テストしやすくしている。
//! 実際の送信や保存は [`Action`] として呼び出し側 (main) に返す。

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::curl::Shell;
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
}

impl ResponseTab {
    pub fn index(self) -> usize {
        match self {
            ResponseTab::Body => 0,
            ResponseTab::Headers => 1,
        }
    }

    fn toggle(self) -> Self {
        match self {
            ResponseTab::Body => ResponseTab::Headers,
            ResponseTab::Headers => ResponseTab::Body,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    /// フォーカス中のテキスト欄を編集中
    Editing,
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
    /// cURL タブで出力するシェルの種類
    pub curl_shell: Shell,
    pub response_tab: ResponseTab,
    pub response: ResponseState,
    pub response_scroll: u16,
    pub spinner: usize,
    pub status: String,
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
            curl_shell: Shell::Posix,
            response_tab: ResponseTab::Body,
            response: ResponseState::Empty,
            response_scroll: 0,
            spinner: 0,
            status: "? でヘルプ / Ctrl+R で送信".to_string(),
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

    pub fn tick(&mut self) {
        self.spinner = (self.spinner + 1) % SPINNER.len();
    }

    /// 送信結果を反映する。
    pub fn set_response(&mut self, result: Result<Response, String>) {
        self.response_scroll = 0;
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
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.commit();
            }
            KeyCode::Tab => {
                self.mode = Mode::Normal;
                self.commit();
                self.focus = self.focus.next();
            }
            KeyCode::Enter if self.focus == Focus::Url => {
                self.mode = Mode::Normal;
                self.commit();
            }
            _ => {
                if let Some(ed) = self.active_editor() {
                    edit_key(ed, key);
                } else {
                    self.mode = Mode::Normal;
                }
            }
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char('q') => return Action::Quit,
            KeyCode::Char('?') => self.mode = Mode::Help,
            KeyCode::Tab => self.focus = self.focus.next(),
            KeyCode::BackTab => self.focus = self.focus.prev(),
            KeyCode::Char('m') => self.method = self.method.next(),
            KeyCode::Char('M') => self.method = self.method.prev(),
            _ => match self.focus {
                Focus::Collection => self.handle_collection_key(key),
                Focus::Url => self.handle_url_key(key),
                Focus::Request => self.handle_request_key(key),
                Focus::Response => self.handle_response_key(key),
            },
        }
        Action::None
    }

    fn handle_collection_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.select(self.selected - 1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => self.select(self.selected + 1),
            KeyCode::Enter => self.focus = Focus::Url,
            KeyCode::Char('n') => {
                self.commit();
                self.collection.push(Request::default());
                let last = self.collection.len() - 1;
                self.select(last);
                self.status = "新しいリクエストを追加しました".to_string();
            }
            KeyCode::Char('y') => {
                self.commit();
                let mut dup = self.collection[self.selected].clone();
                dup.name = format!("{} (copy)", dup.name);
                self.collection.insert(self.selected + 1, dup);
                self.select(self.selected + 1);
                self.status = "リクエストを複製しました".to_string();
            }
            KeyCode::Char('d') => {
                let removed = self.collection.remove(self.selected);
                if self.collection.is_empty() {
                    self.collection.push(Request::default());
                }
                self.selected = self.selected.min(self.collection.len() - 1);
                self.load_selected();
                self.status = format!("「{}」を削除しました", removed.name);
            }
            KeyCode::Char('r') => {
                self.rename = TextEditor::single_line(&self.collection[self.selected].name);
                self.mode = Mode::Rename;
            }
            _ => {}
        }
    }

    fn handle_url_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter | KeyCode::Char('i') => self.mode = Mode::Editing,
            KeyCode::Right | KeyCode::Char('l') => self.method = self.method.next(),
            KeyCode::Left | KeyCode::Char('h') => self.method = self.method.prev(),
            _ => {}
        }
    }

    fn handle_request_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter | KeyCode::Char('i') => {
                if self.request_tab != RequestTab::Curl {
                    self.mode = Mode::Editing;
                }
            }
            KeyCode::Right | KeyCode::Char('l') => self.request_tab = self.request_tab.cycle(1),
            KeyCode::Left | KeyCode::Char('h') => self.request_tab = self.request_tab.cycle(-1),
            KeyCode::Char(c @ '1'..='4') => {
                self.request_tab = RequestTab::ALL[(c as u8 - b'1') as usize];
            }
            KeyCode::Char('s') if self.request_tab == RequestTab::Curl => {
                self.curl_shell = self.curl_shell.next();
                self.status = format!("cURL の出力先: {}", self.curl_shell.label());
            }
            _ => {}
        }
    }

    fn handle_response_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.response_scroll = self
                    .response_scroll
                    .saturating_add(1)
                    .min(self.max_scroll())
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.response_scroll = self.response_scroll.saturating_sub(1)
            }
            KeyCode::PageDown | KeyCode::Char('d') => {
                self.response_scroll = self
                    .response_scroll
                    .saturating_add(20)
                    .min(self.max_scroll())
            }
            KeyCode::PageUp | KeyCode::Char('u') => {
                self.response_scroll = self.response_scroll.saturating_sub(20)
            }
            KeyCode::Char('g') | KeyCode::Home => self.response_scroll = 0,
            KeyCode::Char('G') | KeyCode::End => self.response_scroll = self.max_scroll(),
            KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l') => {
                self.response_tab = self.response_tab.toggle();
                self.response_scroll = 0;
            }
            KeyCode::Char('1') => self.response_tab = ResponseTab::Body,
            KeyCode::Char('2') => self.response_tab = ResponseTab::Headers,
            _ => {}
        }
    }

    /// 現在のレスポンスタブで表示する行数 - 1。
    fn max_scroll(&self) -> u16 {
        let lines = match &self.response {
            ResponseState::Done(r) => match self.response_tab {
                ResponseTab::Body => r.body.lines().count(),
                ResponseTab::Headers => r.headers.len(),
            },
            _ => 0,
        };
        lines.saturating_sub(1).min(u16::MAX as usize) as u16
    }
}

/// エディタ共通のキー操作。
fn edit_key(ed: &mut TextEditor, key: KeyEvent) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
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
        app.handle_key(key(KeyCode::Char('i')));
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

        app.handle_key(key(KeyCode::Right)); // Body
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
    fn s_cycles_curl_shell_only_on_curl_tab() {
        let mut app = App::new(vec![]);
        app.focus = Focus::Request;
        app.handle_key(key(KeyCode::Char('s')));
        assert_eq!(app.curl_shell, Shell::Posix);
        app.handle_key(key(KeyCode::Char('4')));
        app.handle_key(key(KeyCode::Char('s')));
        assert_eq!(app.curl_shell, Shell::Fish);
        app.handle_key(key(KeyCode::Char('s')));
        assert_eq!(app.curl_shell, Shell::PowerShell);
        assert!(app.status.contains("PowerShell"));
    }

    #[test]
    fn request_tabs_wrap_around() {
        let mut app = App::new(vec![]);
        app.focus = Focus::Request;
        app.handle_key(key(KeyCode::Left));
        assert_eq!(app.request_tab, RequestTab::Curl);
        app.handle_key(key(KeyCode::Right));
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
        app.handle_key(key(KeyCode::Char('n')));
        assert_eq!(app.collection.len(), 2);
        assert_eq!(app.selected, 1);

        app.handle_key(key(KeyCode::Char('y')));
        assert_eq!(app.collection.len(), 3);
        assert_eq!(app.collection[2].name, "New Request (copy)");
        assert_eq!(app.selected, 2);

        app.handle_key(key(KeyCode::Char('d')));
        app.handle_key(key(KeyCode::Char('d')));
        app.handle_key(key(KeyCode::Char('d')));
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

    #[test]
    fn response_scroll_is_clamped() {
        let mut app = App::new(vec![]);
        app.set_response(Ok(sample_response("a\nb\nc")));
        app.focus = Focus::Response;
        for _ in 0..10 {
            app.handle_key(key(KeyCode::Char('j')));
        }
        assert_eq!(app.response_scroll, 2);
        app.handle_key(key(KeyCode::Char('g')));
        assert_eq!(app.response_scroll, 0);
        app.handle_key(key(KeyCode::Char('G')));
        assert_eq!(app.response_scroll, 2);
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.response_scroll, 1);
        app.handle_key(key(KeyCode::Char('l')));
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
        app.handle_key(key(KeyCode::Right));
        assert_eq!(app.method, HttpMethod::Post);
        app.handle_key(key(KeyCode::Left));
        app.handle_key(key(KeyCode::Char('M')));
        assert_eq!(app.method, HttpMethod::Options);
    }
}
