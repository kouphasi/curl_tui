//! Ratatui による描画。

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};

use crate::app::{App, Focus, Mode, RequestTab, ResponseState, ResponseTab, SPINNER};
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
        draw_editor(frame, &app.command, Block::new(), input, true, "");
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
    draw_editor(
        frame,
        &app.url,
        block(app, Focus::Url, title),
        url_area,
        editing,
        "https://...",
    );
}

fn draw_request(frame: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = RequestTab::ALL
        .iter()
        .map(|t| Line::from(t.title()))
        .collect();
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
    match app.request_tab {
        RequestTab::Params => draw_editor(
            frame,
            &app.params,
            Block::new(),
            body_area,
            editing,
            "key=value  (1 行 1 件 / # で無効化)",
        ),
        RequestTab::Headers => draw_editor(
            frame,
            &app.headers,
            Block::new(),
            body_area,
            editing,
            "Header-Name: value  (1 行 1 件 / # で無効化)",
        ),
        RequestTab::Body => draw_editor(
            frame,
            &app.body,
            Block::new(),
            body_area,
            editing,
            "リクエストボディ",
        ),
        RequestTab::Curl => {
            let curl = curl::to_curl(&app.current_request());
            frame.render_widget(
                Paragraph::new(curl)
                    .fg(Color::Cyan)
                    .wrap(Wrap { trim: false }),
                body_area,
            );
        }
    }
}

/// エディタを描画し、編集中ならカーソルを置く。カーソルが見えるようにスクロールする。
fn draw_editor(
    frame: &mut Frame,
    editor: &TextEditor,
    blk: Block,
    area: Rect,
    editing: bool,
    placeholder: &str,
) {
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
            let tabs = Tabs::new(vec![
                Line::from("Body"),
                Line::from(format!("Headers ({})", r.headers.len())),
            ])
            .select(app.response_tab.index())
            .highlight_style(Style::new().fg(Color::Yellow).bold().underlined())
            .divider("│");
            frame.render_widget(tabs, tab_area);

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
        (
            "o / dd / yy / p / r",
            "(Collection) 新規 / 削除 / ヤンク / 貼付 / 名前変更",
        ),
        ("Ctrl+W / Ctrl+H", "(Insert) 単語削除 / 1 文字削除"),
        (
            "Ctrl+A/E/K/U",
            "(Insert) 行頭 / 行末 / 行末まで削除 / 全消去",
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
    draw_editor(frame, &app.rename, blk, area, true, "");
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
    fn small_terminal_does_not_panic() {
        let app = App::new(vec![]);
        let mut terminal = Terminal::new(TestBackend::new(20, 5)).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
    }
}
