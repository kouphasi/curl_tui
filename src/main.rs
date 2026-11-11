use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Result;
use crossterm::{
    cursor::SetCursorStyle,
    event::{
        DisableBracketedPaste, EnableBracketedPaste, Event, EventStream, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::supports_keyboard_enhancement,
};
use futures::StreamExt;
use ratatui::DefaultTerminal;
use tokio::sync::mpsc;

use curl_tui::{
    app::{Action, App, Mode, PreviewState, ResponseState},
    collection, http, kitty,
    model::Response,
    preview,
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
    let browser = preview::find_browser();
    app.preview_unavailable = match (kitty::detect(), &browser) {
        (Err(e), _) => Some(e),
        (Ok(()), None) => Some(
            "Chromium 系のブラウザが見つかりません (CURL_TUI_BROWSER で実行ファイルを指定できます)"
                .into(),
        ),
        (Ok(()), Some(_)) => None,
    };

    // ratatui::init は raw mode / 代替画面 / パニック時の復元フックを設定する
    let mut terminal = ratatui::init();
    execute!(std::io::stdout(), EnableBracketedPaste)?;
    // 通常の端末では Shift+Esc が素の Esc と区別できない。kitty keyboard protocol に
    // 対応した端末では修飾キー付きの Esc を受け取れるようにする
    let enhanced = supports_keyboard_enhancement().unwrap_or(false);
    if enhanced {
        execute!(
            std::io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
    }
    let result = run(&mut terminal, &mut app, &path, browser).await;
    if app.preview_unavailable.is_none() {
        let mut out = std::io::stdout();
        let _ = out.write_all(kitty::free().as_bytes());
        let _ = out.flush();
    }
    if enhanced {
        let _ = execute!(std::io::stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(
        std::io::stdout(),
        DisableBracketedPaste,
        SetCursorStyle::DefaultUserShape
    );
    ratatui::restore();
    result
}

/// 描画結果 (どのレスポンスのものか, 描画した幅のセル数, 結果)。
type PreviewResult = (u64, u16, Result<preview::Rendered, String>);

async fn run(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    path: &Path,
    browser: Option<PathBuf>,
) -> Result<()> {
    let client = http::build_client(Duration::from_secs(30))?;
    let (tx, mut rx) = mpsc::unbounded_channel::<Result<Response, String>>();
    let (ptx, mut prx) = mpsc::unbounded_channel::<PreviewResult>();
    // 画像のピクセル高さ (スクロール範囲の計算用) と、現在の表示位置
    let mut image_height = 0u32;
    let mut placed: Option<kitty::Placement> = None;
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
        if let Some(browser) = &browser {
            let (cell_w, cell_h) = cell_size();
            start_preview(app, browser, cell_w, cell_h, &ptx);
            update_placement(app, image_height, cell_w, cell_h, &mut placed)?;
        }

        tokio::select! {
            maybe = events.next() => {
                let Some(event) = maybe else { break };
                let event = event?;
                if matches!(event, Event::Resize(..)) {
                    // 画面がクリアされると画像も消えるので置き直す
                    placed = None;
                }
                match app.handle_event(event) {
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
            Some((generation, cols, res)) = prx.recv() => {
                if generation != app.preview_generation {
                    continue; // 別のレスポンスの描画結果
                }
                match res {
                    Ok(img) => {
                        let mut out = std::io::stdout();
                        out.write_all(kitty::transmit_png(&img.png).as_bytes())?;
                        out.flush()?;
                        image_height = img.height;
                        placed = None;
                        let (_, cell_h) = cell_size();
                        let rows = img.height.div_ceil(cell_h).min(u16::MAX as u32) as u16;
                        app.preview = PreviewState::Ready { cols, rows };
                    }
                    Err(e) => app.preview = PreviewState::Failed(e),
                }
            }
            _ = ticker.tick(), if app.is_busy() => app.tick(),
        }
    }
    Ok(())
}

/// 1 セルのピクセルサイズ。端末が教えてくれない場合は一般的な値を仮定する。
fn cell_size() -> (u32, u32) {
    match crossterm::terminal::window_size() {
        Ok(ws) if ws.width > 0 && ws.height > 0 && ws.columns > 0 && ws.rows > 0 => (
            (ws.width / ws.columns).max(1) as u32,
            (ws.height / ws.rows).max(1) as u32,
        ),
        _ => (10, 20),
    }
}

/// Preview タブが表示されていて未描画 (または幅が変わった) なら、描画を始める。
fn start_preview(
    app: &mut App,
    browser: &Path,
    cell_w: u32,
    cell_h: u32,
    ptx: &mpsc::UnboundedSender<PreviewResult>,
) {
    let Some(area) = app.preview_area.get() else {
        return;
    };
    let needs_render = match app.preview {
        PreviewState::Idle => true,
        PreviewState::Ready { cols, .. } => cols != area.width,
        PreviewState::Rendering | PreviewState::Failed(_) => false,
    };
    let ResponseState::Done(r) = &app.response else {
        return;
    };
    if !needs_render || area.width == 0 || area.height == 0 {
        return;
    }
    let width = area.width as u32 * cell_w;
    let viewport = area.height as u32 * cell_h;
    // 1 画面分だけでなく、スクロールして読めるよう数画面分を撮っておく
    let job = preview::Job {
        html: r.body.clone(),
        base_url: r.url.clone(),
        width,
        height: (viewport * 4).clamp(viewport, preview::MAX_HEIGHT.max(viewport)),
    };
    app.preview = PreviewState::Rendering;
    app.response_scroll = 0;
    let generation = app.preview_generation;
    let cols = area.width;
    let browser = browser.to_path_buf();
    let ptx = ptx.clone();
    tokio::spawn(async move {
        let res = preview::render(&browser, &job)
            .await
            .map_err(|e| format!("{e:#}"));
        let _ = ptx.send((generation, cols, res));
    });
}

/// 画像の表示位置を描画状態に合わせる。変化がなければ何も送らない。
fn update_placement(
    app: &App,
    image_height: u32,
    cell_w: u32,
    cell_h: u32,
    placed: &mut Option<kitty::Placement>,
) -> Result<()> {
    let want = match (app.preview_area.get(), &app.preview) {
        (Some(area), PreviewState::Ready { cols, .. })
            if *cols == area.width && area.height > 0 =>
        {
            let src_y = (app.response_scroll as u32 * cell_h).min(image_height.saturating_sub(1));
            let src_h = (area.height as u32 * cell_h).min(image_height - src_y);
            Some(kitty::Placement {
                col: area.x,
                row: area.y,
                cols: area.width,
                rows: src_h.div_ceil(cell_h) as u16,
                src_y,
                src_w: area.width as u32 * cell_w,
                src_h,
            })
        }
        _ => None,
    };
    if want == *placed {
        return Ok(());
    }
    let seq = match &want {
        Some(p) => kitty::place(p),
        None => kitty::hide(),
    };
    let mut out = std::io::stdout();
    out.write_all(seq.as_bytes())?;
    out.flush()?;
    *placed = want;
    Ok(())
}
