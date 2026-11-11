//! Ratatui による描画。

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};

use crate::app::{
    App, EditorRegion, Focus, Mode, PreviewState, Regions, RequestTab, ResponseState, ResponseTab,
    SPINNER,
};
use crate::curl;
use crate::editor::TextEditor;
use crate::model::{HttpMethod, format_size};

pub fn method_color(m: HttpMethod) -> Color {
    match m {
        HttpMethod::Get => Color::Green,
        HttpMethod::Post => Color::Yellow,
        HttpMethod::Put => Color::Blue,
        HttpMethod::Patch => Color::Magenta,
        HttpMethod::Delete => Color::Red,
        HttpMethod::Head | HttpMethod::Options => Color::Cyan,
    }
}

fn status_color(status: u16) -> Color {
    match status {
        200..=299 => Color::Green,
        300..=399 => Color::Cyan,
        400..=499 => Color::Yellow,
        _ => Color::Red,
    }
}

fn block<'a>(app: &App, focus: Focus, title: impl Into<Line<'a>>) -> Block<'a> {
    let focused = app.focus == focus;
    let color = match (focused, app.mode) {
        (true, Mode::Editing) => Color::Green,
        (true, _) => Color::Yellow,
        _ => Color::DarkGray,
    };
    Block::bordered()
        .border_type(if focused {
            BorderType::Thick
        } else {
            BorderType::Rounded
        })
        .border_style(Style::new().fg(color))
        .title(title)
}

pub fn draw(frame: &mut Frame, app: &App) {
    app.preview_area.set(None);
    *app.regions.borrow_mut() = Regions::default();
    let [header, main, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    draw_header(frame, app, header);
    draw_footer(frame, app, footer);

    let [sidebar, content] =
        Layout::horizontal([Constraint::Length(28), Constraint::Min(20)]).areas(main);
    draw_collection(frame, app, sidebar);

    let [url_row, request, response] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Percentage(40),
        Constraint::Min(5),
    ])
    .areas(content);
    draw_url(frame, app, url_row);
    draw_request(frame, app, request);
    draw_response(frame, app, response);

    match app.mode {
        Mode::Help => draw_help(frame),
        Mode::Rename => draw_rename(frame, app),
        _ => {}
    }
    // 画像は文字より手前に描かれるため、ポップアップ表示中は画像を隠す
    if matches!(app.mode, Mode::Help | Mode::Rename) {
        app.preview_area.set(None);
    }
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let mode = match app.mode {
        Mode::Normal => Span::raw(" NORMAL ").black().on_blue(),
        Mode::Editing => Span::raw(" INSERT ").black().on_green(),
        Mode::Command => Span::raw(" COMMAND ").black().on_yellow(),
        Mode::Rename => Span::raw(" RENAME ").black().on_magenta(),
        Mode::Help => Span::raw(" HELP ").black().on_cyan(),
    };
    let line = Line::from(vec![
        Span::raw(" curl_tui ").bold().fg(Color::Cyan),
        mode,
        Span::raw("  "),
        Span::raw(app.status.clone()).fg(Color::Gray),
    ]);
    frame.render_widget(Paragraph::new(line), area);
    // 入力途中のキー (Vim の showcmd 相当) を右端に出す
    let pending = app.pending_keys();
    if !pending.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::raw(format!("{pending} ")).bold().fg(Color::Yellow))
                .right_aligned(),
            area,
        );
    }
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    if app.mode == Mode::Command {
        let [colon, input] =
            Layout::horizontal([Constraint::Length(1), Constraint::Min(1)]).areas(area);
        frame.render_widget(Paragraph::new(":"), colon);
        let _ = draw_editor(frame, &app.command, Block::new(), input, true, "");
        return;
    }
    let editor_focused = editor_focused(app);
    let hints: &[(&str, &str)] = match (app.mode, app.focus) {
        (Mode::Editing, _) => &[
            ("Esc", "Normal へ"),
            ("Ctrl+W", "単語削除"),
            ("Ctrl+R", "送信"),
        ],
        (Mode::Rename, _) => &[("Enter", "確定"), ("Esc", "キャンセル")],
        (_, Focus::Collection) => &[
            ("j/k", "選択"),
            ("o", "新規"),
            ("yy/p", "複製"),
            ("dd", "削除"),
            ("r", "名前変更"),
            (":w", "保存"),
        ],
        (_, Focus::Url) => &[
            ("i/a/A", "挿入"),
            ("hjkl/w/b", "移動"),
            ("m/M", "メソッド"),
            ("Ctrl+R", "送信"),
        ],
        (_, Focus::Request) if editor_focused => &[
            ("i/a/o", "挿入"),
            ("dd/x/p/u", "編集"),
            ("H/L", "タブ"),
            ("Ctrl+R", "送信"),
        ],
        (_, Focus::Request) => &[("H/L,1-4", "タブ"), ("Ctrl+R", "送信")],
        (_, Focus::Response) => &[
            ("j/k", "スクロール"),
            ("Ctrl+D/U", "半ページ"),
            ("gg/G", "先頭/末尾"),
            ("H/L", "タブ"),
        ],
    };
    let mut spans = Vec::new();
    for (k, desc) in hints
        .iter()
        .chain([("Ctrl+HJKL", "ペイン移動"), ("?", "ヘルプ"), (":q", "終了")].iter())
    {
        // 背景色に依存せず、どのテーマでも読みやすいよう前景色のみで強調する
        spans.push(Span::raw(format!(" {k}")).fg(Color::Cyan).bold());
        spans.push(Span::raw(format!(" {desc}  ")).fg(Color::Gray));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// フォーカス中のペインが編集可能なテキスト欄か。
fn editor_focused(app: &App) -> bool {
    match app.focus {
        Focus::Url => true,
        Focus::Request => app.request_tab != RequestTab::Curl,
        _ => false,
    }
}

/// テキスト欄にカーソルを表示するか (Normal / Insert どちらでも表示する)。
fn show_cursor(app: &App, focus: Focus) -> bool {
    app.focus == focus && matches!(app.mode, Mode::Normal | Mode::Editing)
}

fn draw_collection(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .collection
        .iter()
        .enumerate()
        .map(|(i, r)| {
            // 選択中のリクエストは編集中のメソッドを表示する
            let method = if i == app.selected {
                app.method
            } else {
                r.method
            };
            ListItem::new(Line::from(vec![
                Span::raw(format!("{:<7}", method.as_str()))
                    .fg(method_color(method))
                    .bold(),
                Span::raw(r.name.clone()),
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(block(app, Focus::Collection, " Collection "))
        .highlight_style(
            Style::new()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    let mut state = ListState::default().with_selected(Some(app.selected));
    frame.render_stateful_widget(list, area, &mut state);
    let mut regions = app.regions.borrow_mut();
    regions.collection = area;
    regions.collection_list = Block::bordered().inner(area);
    regions.collection_offset = state.offset();
}

fn draw_url(frame: &mut Frame, app: &App, area: Rect) {
    let [method_area, url_area] =
        Layout::horizontal([Constraint::Length(11), Constraint::Min(10)]).areas(area);
    let method = Paragraph::new(Line::from(
        Span::raw(app.method.as_str())
            .bold()
            .fg(method_color(app.method)),
    ))
    .centered()
    .block(block(app, Focus::Url, " Method "));
    frame.render_widget(method, method_area);

    let editing = show_cursor(app, Focus::Url);
    let title = format!(" URL  [{}] ", app.collection[app.selected].name);
    let region = draw_editor(
        frame,
        &app.url,
        block(app, Focus::Url, title),
        url_area,
        editing,
        "https://...",
    );
    let mut regions = app.regions.borrow_mut();
    regions.url = area;
    regions.url_editor = Some(region);
}

fn draw_request(frame: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = RequestTab::ALL
        .iter()
        .map(|t| Line::from(t.title()))
        .collect();
    let tab_widths: Vec<usize> = titles.iter().map(Line::width).collect();
    let tabs = Tabs::new(titles)
        .select(app.request_tab.index())
        .highlight_style(Style::new().fg(Color::Yellow).bold().underlined())
        .divider("│");
    let blk = block(app, Focus::Request, " Request ");
    let inner = blk.inner(area);
    frame.render_widget(blk, area);
    let [tab_area, body_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
    frame.render_widget(tabs, tab_area);

    let editing = show_cursor(app, Focus::Request);
    let editor = match app.request_tab {
        RequestTab::Params => Some(draw_editor(
            frame,
            &app.params,
            Block::new(),
            body_area,
            editing,
            "key=value  (1 行 1 件 / # で無効化)",
        )),
        RequestTab::Headers => Some(draw_editor(
            frame,
            &app.headers,
            Block::new(),
            body_area,
            editing,
            "Header-Name: value  (1 行 1 件 / # で無効化)",
        )),
        RequestTab::Body => Some(draw_editor(
            frame,
            &app.body,
            Block::new(),
            body_area,
            editing,
            "リクエストボディ",
        )),
        RequestTab::Curl => {
            let curl = curl::to_curl(&app.current_request());
            frame.render_widget(
                Paragraph::new(curl)
                    .fg(Color::Cyan)
                    .wrap(Wrap { trim: false }),
                body_area,
            );
            None
        }
    };
    let mut regions = app.regions.borrow_mut();
    regions.request = area;
    regions.request_tabs = tab_rects(tab_area, &tab_widths);
    regions.request_editor = editor;
}

/// `Tabs` が各タイトルを描く位置 (前後の空白 1 つずつを含む)。クリック判定に使う。
fn tab_rects(area: Rect, widths: &[usize]) -> Vec<Rect> {
    let mut x = area.x;
    widths
        .iter()
        .map(|w| {
            // 空白 + タイトル + 空白、その後ろに区切り線 1 つ
            let width = (*w as u16).saturating_add(2);
            let start = x.min(area.right());
            let rect = Rect::new(
                start,
                area.y,
                width.min(area.right() - start),
                area.height.min(1),
            );
            x = x.saturating_add(width).saturating_add(1);
            rect
        })
        .collect()
}

/// エディタを描画し、編集中ならカーソルを置く。カーソルが見えるようにスクロールする。
/// 描画した領域とスクロール量を返す。
#[must_use]
fn draw_editor(
    frame: &mut Frame,
    editor: &TextEditor,
    blk: Block,
    area: Rect,
    editing: bool,
    placeholder: &str,
) -> EditorRegion {
    let inner = blk.inner(area);
    let (row, _) = editor.cursor();
    let col = editor.cursor_display_col();
    let scroll_y = (row + 1).saturating_sub(inner.height as usize) as u16;
    let scroll_x = (col + 1).saturating_sub(inner.width as usize) as u16;

    let text: Text = if editor.is_empty() && !editing {
        Text::from(
            Span::raw(placeholder.to_string())
                .fg(Color::DarkGray)
                .italic(),
        )
    } else {
        Text::from(
            editor
                .lines()
                .iter()
                .map(|l| Line::from(l.as_str()))
                .collect::<Vec<_>>(),
        )
    };
    frame.render_widget(
        Paragraph::new(text).block(blk).scroll((scroll_y, scroll_x)),
        area,
    );

    if editing && inner.width > 0 && inner.height > 0 {
        let x = inner.x + (col as u16).saturating_sub(scroll_x);
        let y = inner.y + (row as u16).saturating_sub(scroll_y);
        frame.set_cursor_position((x, y));
    }
    EditorRegion {
        area: inner,
        scroll_x,
        scroll_y,
    }
}

fn draw_response(frame: &mut Frame, app: &App, area: Rect) {
    let mut title = vec![Span::raw(" Response ")];
    if let ResponseState::Done(r) = &app.response {
        title.push(
            Span::raw(format!(" {} {} ", r.status, r.status_text))
                .bold()
                .black()
                .bg(status_color(r.status)),
        );
        title.push(Span::raw(format!(
            " {} ms · {} ",
            r.elapsed_ms,
            format_size(r.size)
        )));
    }
    let blk = block(app, Focus::Response, Line::from(title));
    let inner = blk.inner(area);
    frame.render_widget(blk, area);
    app.regions.borrow_mut().response = area;

    match &app.response {
        ResponseState::Empty => frame.render_widget(
            Paragraph::new("Ctrl+R (または F5) でリクエストを送信")
                .fg(Color::DarkGray)
                .centered(),
            inner,
        ),
        ResponseState::Loading => frame.render_widget(
            Paragraph::new(format!("{} 送信中…", SPINNER[app.spinner]))
                .fg(Color::Yellow)
                .centered(),
            inner,
        ),
        ResponseState::Failed(e) => frame.render_widget(
            Paragraph::new(e.as_str())
                .fg(Color::Red)
                .wrap(Wrap { trim: false }),
            inner,
        ),
        ResponseState::Done(r) => {
            let [tab_area, body_area] =
                Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
            app.response_height.set(body_area.height);
            let titles: Vec<Line> = app
                .response_tabs()
                .iter()
                .map(|t| match t {
                    ResponseTab::Body => Line::from("Body"),
                    ResponseTab::Headers => Line::from(format!("Headers ({})", r.headers.len())),
                    ResponseTab::Preview => Line::from("Preview"),
                })
                .collect();
            let tab_widths: Vec<usize> = titles.iter().map(Line::width).collect();
            app.regions.borrow_mut().response_tabs = tab_rects(tab_area, &tab_widths);
            let tabs = Tabs::new(titles)
                .select(app.response_tab.index())
                .highlight_style(Style::new().fg(Color::Yellow).bold().underlined())
                .divider("│");
            frame.render_widget(tabs, tab_area);

            if app.response_tab == ResponseTab::Preview {
                draw_preview(frame, app, body_area);
                return;
            }
            let text: Text = match app.response_tab {
                ResponseTab::Body => Text::from(r.body.as_str()),
                ResponseTab::Headers => Text::from(
                    r.headers
                        .iter()
                        .map(|(k, v)| {
                            Line::from(vec![
                                Span::raw(k.clone()).fg(Color::Cyan),
                                Span::raw(": "),
                                Span::raw(v.clone()),
                            ])
                        })
                        .collect::<Vec<_>>(),
                ),
                ResponseTab::Preview => unreachable!(),
            };
            frame.render_widget(
                Paragraph::new(text)
                    .wrap(Wrap { trim: false })
                    .scroll((app.response_scroll, 0)),
                body_area,
            );
        }
    }
}

/// Preview タブ。画像そのものは main が kitty graphics protocol で上に重ねる。
fn draw_preview(frame: &mut Frame, app: &App, area: Rect) {
    if let Some(reason) = &app.preview_unavailable {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(format!("プレビューを表示できません: {reason}")).fg(Color::Yellow),
                Line::from("HTML のソースは Body タブで確認できます").fg(Color::DarkGray),
            ])
            .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    // 描画済みでも幅が変われば main が描き直すので、領域は常に知らせる
    app.preview_area.set(Some(area));
    let msg = match &app.preview {
        PreviewState::Ready { .. } => return,
        PreviewState::Idle | PreviewState::Rendering => {
            Paragraph::new(format!("{} ブラウザで描画中…", SPINNER[app.spinner]))
                .fg(Color::Yellow)
                .centered()
        }
        PreviewState::Failed(e) => Paragraph::new(e.as_str())
            .fg(Color::Red)
            .wrap(Wrap { trim: false }),
    };
    frame.render_widget(msg, area);
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [v] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    let [h] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(v);
    h
}

fn draw_help(frame: &mut Frame) {
    let rows: &[(&str, &str)] = &[
        ("Ctrl+H/J/K/L", "ペイン移動 (左 / 下 / 上 / 右)"),
        ("Ctrl+W h/j/k/l/w", "ペイン移動 (Vim のウィンドウ操作)"),
        ("Ctrl+R / F5", "リクエスト送信 (:send も可)"),
        (":w / :q / :wq", "保存 / 終了 / 保存して終了"),
        ("m / M", "メソッド切り替え"),
        ("H / L, gt / gT", "タブ切り替え (1-4 で直接選択)"),
        ("h j k l w b e", "カーソル移動 (テキスト欄)"),
        ("0 ^ $ gg G", "行頭 / 行頭の文字 / 行末 / 先頭 / 末尾"),
        ("i a I A o O", "Insert モードへ (Esc で Normal)"),
        ("x X r s S D C", "文字削除 / 置換 / 行末まで削除 など"),
        ("d c y + 移動", "dw, cw, d$, dd, cc, yy など"),
        ("p / P / u", "貼り付け / 前に貼り付け / 元に戻す"),
        ("Ctrl+D/U/F/B", "レスポンスのページ送り"),
        ("3 (Response)", "HTML のプレビュー (kitty 対応端末)"),
        (
            "o / dd / yy / p / r",
            "(Collection) 新規 / 削除 / ヤンク / 貼付 / 名前変更",
        ),
        ("Ctrl+W / Ctrl+H", "(Insert) 単語削除 / 1 文字削除"),
        (
            "Ctrl+A/E/K/U",
            "(Insert) 行頭 / 行末 / 行末まで削除 / 全消去",
        ),
        (
            "クリック / ホイール",
            "ペイン・リクエスト・タブ・カーソル位置の選択 / スクロール",
        ),
        ("q / Ctrl+C", "終了"),
    ];
    let lines: Vec<Line> = rows
        .iter()
        .map(|(k, d)| {
            Line::from(vec![
                Span::raw(format!("{k:>20}  ")).fg(Color::Yellow),
                Span::raw(*d),
            ])
        })
        .collect();
    let area = centered(frame.area(), 76, rows.len() as u16 + 2);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_type(BorderType::Thick)
                .border_style(Style::new().fg(Color::Cyan))
                .title(" Help (任意のキーで閉じる) "),
        ),
        area,
    );
}

fn draw_rename(frame: &mut Frame, app: &App) {
    let area = centered(frame.area(), 50, 3);
    frame.render_widget(Clear, area);
    let blk = Block::bordered()
        .border_type(BorderType::Thick)
        .border_style(Style::new().fg(Color::Magenta))
        .title(" リクエスト名 ");
    let _ = draw_editor(frame, &app.rename, blk, area, true, "");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Request, Response};
    use ratatui::{Terminal, backend::TestBackend};

    fn render(app: &App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buf = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..buf.area.height {
            let mut x = 0;
            while x < buf.area.width {
                let sym = buf[(x, y)].symbol();
                out.push_str(sym);
                // 全角文字の後ろのセルは埋め草なので読み飛ばす
                x += unicode_width::UnicodeWidthStr::width(sym).max(1) as u16;
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn renders_main_panes() {
        let app = App::new(vec![Request {
            name: "Users".into(),
            url: "https://api.example.com/users".into(),
            ..Default::default()
        }]);
        let screen = render(&app);
        for s in [
            "curl_tui",
            "Collection",
            "Users",
            "Method",
            "GET",
            "https://api.example.com/users",
            "Params",
            "Headers",
            "Body",
            "cURL",
            "Response",
        ] {
            assert!(screen.contains(s), "missing {s:?}\n{screen}");
        }
    }

    #[test]
    fn renders_response_status_and_body() {
        let mut app = App::new(vec![]);
        app.set_response(Ok(Response {
            status: 201,
            status_text: "Created".into(),
            elapsed_ms: 42,
            headers: vec![("x-custom".into(), "yes".into())],
            body: "{\n  \"id\": 7\n}".into(),
            size: 2048,
            ..Default::default()
        }));
        let screen = render(&app);
        assert!(screen.contains("201 Created"));
        assert!(screen.contains("42 ms"));
        assert!(screen.contains("2.0 KB"));
        assert!(screen.contains("\"id\": 7"));
        assert!(screen.contains("Headers (1)"));
    }

    #[test]
    fn renders_error_and_curl_tab() {
        let mut app = App::new(vec![]);
        app.request_tab = RequestTab::Curl;
        app.set_response(Err("接続エラー: refused".into()));
        let screen = render(&app);
        assert!(screen.contains("接続エラー"));
        assert!(screen.contains("curl"));
        assert!(screen.contains("'https://httpbin.org/get'"));
    }

    #[test]
    fn renders_help_and_rename_popups() {
        let mut app = App::new(vec![]);
        app.mode = Mode::Help;
        assert!(render(&app).contains("Help"));
        app.mode = Mode::Rename;
        assert!(render(&app).contains("リクエスト名"));
    }

    #[test]
    fn cursor_is_placed_in_focused_text_field() {
        let mut app = App::new(vec![]);
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        // Normal モードでも URL 欄の最後の文字にカーソルが置かれる
        terminal.draw(|f| draw(f, &app)).unwrap();
        let pos = terminal.get_cursor_position().unwrap();
        // サイドバー(28) + メソッド(11) + 枠(1) + URL 文字数 - 1
        let base = 28 + 11 + 1 + "https://httpbin.org/get".len();
        assert_eq!(pos.x as usize, base - 1);
        assert_eq!(pos.y, 2);
        // A で行末に挿入
        app.handle_key(crossterm::event::KeyEvent::from(
            crossterm::event::KeyCode::Char('A'),
        ));
        terminal.draw(|f| draw(f, &app)).unwrap();
        assert_eq!(terminal.get_cursor_position().unwrap().x as usize, base);
    }

    #[test]
    fn renders_command_line_and_pending_keys() {
        let mut app = App::new(vec![]);
        app.handle_key(crossterm::event::KeyEvent::from(
            crossterm::event::KeyCode::Char('d'),
        ));
        let screen = render(&app);
        assert!(screen.lines().next().unwrap().trim_end().ends_with('d'));
        app.handle_key(crossterm::event::KeyEvent::from(
            crossterm::event::KeyCode::Esc,
        ));
        for c in ":wq".chars() {
            app.handle_key(crossterm::event::KeyEvent::from(
                crossterm::event::KeyCode::Char(c),
            ));
        }
        let screen = render(&app);
        assert!(screen.contains("COMMAND"));
        assert!(screen.lines().last().unwrap().starts_with(":wq"));
    }

    #[test]
    fn preview_tab_shows_status_and_reports_area() {
        let mut app = App::new(vec![]);
        app.set_response(Ok(Response {
            status: 200,
            headers: vec![("content-type".into(), "text/html".into())],
            body: "<html></html>".into(),
            ..Default::default()
        }));
        let screen = render(&app);
        assert!(screen.contains("Preview"));
        assert_eq!(app.preview_area.get(), None);

        app.focus = Focus::Response;
        app.handle_key(crossterm::event::KeyEvent::from(
            crossterm::event::KeyCode::Char('3'),
        ));
        assert!(render(&app).contains("ブラウザで描画中"));
        assert!(app.preview_area.get().is_some());

        // ヘルプ表示中は画像を出さない
        app.mode = Mode::Help;
        render(&app);
        assert_eq!(app.preview_area.get(), None);
        app.mode = Mode::Normal;

        app.preview_unavailable = Some("未対応".into());
        let screen = render(&app);
        assert!(screen.contains("プレビューを表示できません: 未対応"));
        assert_eq!(app.preview_area.get(), None);
    }

    #[test]
    fn json_response_has_no_preview_tab() {
        let mut app = App::new(vec![]);
        app.set_response(Ok(Response {
            status: 200,
            headers: vec![("content-type".into(), "application/json".into())],
            body: "{}".into(),
            ..Default::default()
        }));
        assert!(!render(&app).contains("Preview"));
    }

    fn click(app: &mut App, x: u16, y: u16) {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        });
    }

    /// 画面上で `needle` が始まるセルの位置 (ASCII のみの行を想定)。
    fn find(screen: &str, needle: &str) -> (u16, u16) {
        for (y, line) in screen.lines().enumerate() {
            if let Some(i) = line.find(needle) {
                return (line[..i].chars().count() as u16, y as u16);
            }
        }
        panic!("{needle:?} not found\n{screen}");
    }

    fn two_requests() -> App {
        App::new(vec![
            Request {
                name: "First".into(),
                url: "http://one.example/path".into(),
                body: "line0\nline1\nline2".into(),
                ..Default::default()
            },
            Request {
                name: "Second".into(),
                url: "http://two.example".into(),
                ..Default::default()
            },
        ])
    }

    #[test]
    fn click_selects_collection_item() {
        let mut app = two_requests();
        let screen = render(&app);
        let (x, y) = find(&screen, "Second");
        click(&mut app, x, y);
        assert_eq!(app.focus, Focus::Collection);
        assert_eq!(app.selected, 1);
        assert_eq!(app.url.text(), "http://two.example");
        // 項目のない行をクリックしても選択は変わらない
        render(&app);
        click(&mut app, x, y + 5);
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn click_selects_pane_and_tab() {
        let mut app = two_requests();
        let screen = render(&app);
        let (x, y) = find(&screen, "Body");
        click(&mut app, x + 1, y);
        assert_eq!(app.focus, Focus::Request);
        assert_eq!(app.request_tab, RequestTab::Body);
        let (x, y) = find(&render(&app), "cURL");
        click(&mut app, x, y);
        assert_eq!(app.request_tab, RequestTab::Curl);
        let (x, y) = find(&render(&app), "Method");
        click(&mut app, x, y + 1);
        assert_eq!(app.focus, Focus::Url);

        app.set_response(Ok(Response {
            status: 200,
            headers: vec![("x-a".into(), "1".into())],
            body: "hello".into(),
            ..Default::default()
        }));
        let (x, y) = find(&render(&app), "Headers (1)");
        click(&mut app, x, y);
        assert_eq!(app.focus, Focus::Response);
        assert_eq!(app.response_tab, ResponseTab::Headers);
        let (x, y) = find(&render(&app), "x-a");
        click(&mut app, x, y);
        assert_eq!(app.response_tab, ResponseTab::Headers);
    }

    #[test]
    fn click_places_cursor_in_text_fields() {
        let mut app = two_requests();
        let (x, y) = find(&render(&app), "one.example");
        click(&mut app, x, y);
        assert_eq!(app.focus, Focus::Url);
        assert_eq!(app.url.cursor(), (0, "http://".len()));
        // 行末より右は最後の文字に丸める (Normal モード)
        click(&mut app, x + 60, y);
        assert_eq!(app.url.cursor(), (0, "http://one.example/path".len() - 1));

        app.request_tab = RequestTab::Body;
        let (x, y) = find(&render(&app), "line1");
        click(&mut app, x + 2, y);
        assert_eq!(app.focus, Focus::Request);
        assert_eq!(app.body.cursor(), (1, 2));
        // 最終行より下は最終行に丸める
        click(&mut app, x, y + 4);
        assert_eq!(app.body.cursor(), (2, 0));
    }

    #[test]
    fn click_handles_wide_chars_and_insert_mode() {
        let mut app = App::new(vec![Request {
            url: "http://a".into(),
            body: "あいう".into(),
            ..Default::default()
        }]);
        app.request_tab = RequestTab::Body;
        app.focus = Focus::Request;
        let region = {
            render(&app);
            app.regions.borrow().request_editor.unwrap()
        };
        // 全角 2 文字目の右半分をクリック
        click(&mut app, region.area.x + 3, region.area.y);
        assert_eq!(app.body.cursor(), (0, 1));

        // Insert 中に同じ欄をクリックすると Insert のまま (行末の 1 つ先にも置ける)
        app.handle_key(crossterm::event::KeyEvent::from(
            crossterm::event::KeyCode::Char('i'),
        ));
        render(&app);
        click(&mut app, region.area.x + 20, region.area.y);
        assert_eq!(app.mode, Mode::Editing);
        assert_eq!(app.body.cursor(), (0, 3));
        // 別のペインをクリックすると Normal に戻る
        let (x, y) = find(&render(&app), "http://a");
        click(&mut app, x, y);
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.focus, Focus::Url);
    }

    #[test]
    fn click_closes_help_and_wheel_scrolls_response() {
        use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
        let mut app = two_requests();
        app.mode = Mode::Help;
        render(&app);
        click(&mut app, 1, 3);
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.focus, Focus::Url);

        app.set_response(Ok(Response {
            status: 200,
            body: "a\nb\nc\nd\ne\nf".into(),
            ..Default::default()
        }));
        render(&app);
        let area = app.regions.borrow().response;
        let wheel = |kind, row| MouseEvent {
            kind,
            column: area.x + 2,
            row,
            modifiers: KeyModifiers::NONE,
        };
        app.handle_mouse(wheel(MouseEventKind::ScrollDown, area.y + 3));
        assert_eq!(app.response_scroll, 3);
        app.handle_mouse(wheel(MouseEventKind::ScrollDown, area.y + 3));
        assert_eq!(app.response_scroll, 5);
        app.handle_mouse(wheel(MouseEventKind::ScrollUp, area.y + 3));
        assert_eq!(app.response_scroll, 2);
        // Response の外では何もしない
        app.handle_mouse(wheel(MouseEventKind::ScrollUp, 2));
        assert_eq!(app.response_scroll, 2);
        assert_eq!(app.focus, Focus::Url);
    }

    #[test]
    fn small_terminal_does_not_panic() {
        let app = App::new(vec![]);
        let mut terminal = Terminal::new(TestBackend::new(20, 5)).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
    }
}
