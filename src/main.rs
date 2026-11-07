use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Result;
use crossterm::{
    cursor::SetCursorStyle,
    event::{DisableBracketedPaste, EnableBracketedPaste, EventStream},
    execute,
};
use futures::StreamExt;
use ratatui::DefaultTerminal;
use tokio::sync::mpsc;

use curl_tui::{
    app::{Action, App, Mode},
    collection, http,
    model::Response,
};

#[tokio::main]
async fn main() -> Result<()> {
    let path = match std::env::args().nth(1).as_deref() {
        Some("-h" | "--help") => {
            println!("使い方: curl_tui [コレクションファイル.json]");
            println!("既定: {}", collection::default_path().display());
            return Ok(());
        }
        Some(p) => PathBuf::from(p),
        None => collection::default_path(),
    };
    let requests = collection::load(&path)?;
    let mut app = App::new(requests);

    // ratatui::init は raw mode / 代替画面 / パニック時の復元フックを設定する
    let mut terminal = ratatui::init();
    execute!(std::io::stdout(), EnableBracketedPaste)?;
    let result = run(&mut terminal, &mut app, &path).await;
    let _ = execute!(
        std::io::stdout(),
        DisableBracketedPaste,
        SetCursorStyle::DefaultUserShape
    );
    ratatui::restore();
    result
}

async fn run(terminal: &mut DefaultTerminal, app: &mut App, path: &Path) -> Result<()> {
    let client = http::build_client(Duration::from_secs(30))?;
    let (tx, mut rx) = mpsc::unbounded_channel::<Result<Response, String>>();
    let mut events = EventStream::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(100));

    let mut cursor_mode = None;
    loop {
        // Vim と同じく Normal はブロック、Insert 系はバーのカーソルにする
        let insert_like = matches!(app.mode, Mode::Editing | Mode::Rename | Mode::Command);
        if cursor_mode != Some(insert_like) {
            let style = if insert_like {
                SetCursorStyle::SteadyBar
            } else {
                SetCursorStyle::SteadyBlock
            };
            execute!(std::io::stdout(), style)?;
            cursor_mode = Some(insert_like);
        }
        terminal.draw(|f| curl_tui::ui::draw(f, app))?;

        tokio::select! {
            maybe = events.next() => {
                let Some(event) = maybe else { break };
                match app.handle_event(event?) {
                    Action::None => {}
                    Action::Quit => break,
                    Action::Send(req) => {
                        let client = client.clone();
                        let tx = tx.clone();
                        // 送信は別タスクで行い UI をブロックしない
                        tokio::spawn(async move {
                            let res = http::send(&client, &req).await.map_err(|e| format!("{e:#}"));
                            let _ = tx.send(res);
                        });
                    }
                    Action::Save(reqs) => {
                        app.status = match collection::save(path, &reqs) {
                            Ok(()) => format!("保存しました: {}", path.display()),
                            Err(e) => format!("保存に失敗: {e:#}"),
                        };
                    }
                    Action::SaveAndQuit(reqs) => match collection::save(path, &reqs) {
                        Ok(()) => break,
                        // 保存できなければ終了しない
                        Err(e) => app.status = format!("保存に失敗: {e:#}"),
                    },
                }
            }
            Some(res) = rx.recv() => app.set_response(res),
            _ = ticker.tick(), if app.is_loading() => app.tick(),
        }
    }
    Ok(())
}
