//! 表示言語の切り替え。言語はプロセス全体の設定なので、他のテストと混ざらないよう
//! このファイルのテストは 1 つだけにして順番に確認する。

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use curl_tui::{
    app::App,
    config,
    i18n::{self, Lang},
    keymap::Command,
    model::Request,
    ui,
};
use ratatui::{Terminal, backend::TestBackend};

fn render(app: &App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|f| ui::draw(f, app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut out = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    // 全角文字の後ろに入る埋め草のセルを除く
    out.replace(' ', "")
}

fn key(app: &mut App, c: char) {
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char(c),
        KeyModifiers::NONE,
    )));
}

#[test]
fn language_follows_config() {
    // 既定は英語
    assert_eq!(i18n::current(), Lang::En);
    assert_eq!(Command::AppQuit.description(), "Quit");
    let app = App::new(vec![Request::default()]);
    assert!(app.status.contains("for help"), "{}", app.status);
    assert!(render(&app).contains("Ctrl+R(orF5)"));
    let template = config::template();
    assert!(template.contains("\"lang\": \"en\""));
    assert!(template.contains("Default key bindings"));
    assert_eq!(config::parse(&template).unwrap().lang, Lang::En);

    // "lang": "ja" で日本語になる
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.jsonc");
    std::fs::write(&path, "{ \"lang\": \"ja\" }\n").unwrap();
    i18n::set(config::load(&path).unwrap().lang);
    assert_eq!(i18n::current(), Lang::Ja);
    assert_eq!(Command::AppQuit.description(), "終了");
    let mut app = App::new(vec![Request::default()]);
    assert!(app.status.contains("でヘルプ"), "{}", app.status);
    assert!(render(&app).contains("でリクエストを送信"));
    key(&mut app, '?');
    assert!(render(&app).contains("任意のキーで閉じる"));
    let err = config::parse("{ \"lang\": \"fr\" }")
        .unwrap_err()
        .to_string();
    assert!(err.contains("不明な"), "{err}");

    // 雛形とリファレンスも日本語になり、英語で作ったファイルのリファレンスを書き直せる
    let ja_template = config::template();
    assert!(ja_template.contains("\"lang\": \"ja\""));
    assert!(ja_template.contains("既定のキーバインド"));
    assert_eq!(config::parse(&ja_template).unwrap().lang, Lang::Ja);
    std::fs::write(
        &path,
        template.replace("\"lang\": \"en\"", "\"lang\": \"ja\""),
    )
    .unwrap();
    config::prepare(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("既定のキーバインド"));
    assert!(!text.contains("Default key bindings"));
    assert!(text.starts_with("// curl_tui config file"));

    // 英語に戻す
    i18n::set(Lang::En);
    config::prepare(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("Default key bindings"));
    assert!(!text.contains("既定のキーバインド"));
}
