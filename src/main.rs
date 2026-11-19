use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Result;
use crossterm::{
    cursor::SetCursorStyle,
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, EventStream, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::supports_keyboard_enhancement,
};
use futures::StreamExt;
use ratatui::DefaultTerminal;
use tokio::sync::mpsc;

use curl_tui::{
    app::{Action, App, Mode, PreviewState, ResponseState, TextField},
    clipboard, collection, config, http, i18n, kitty,
    model::Response,
    preview, t, tf, vimrc,
};

#[tokio::main]
async fn main() -> Result<()> {
    // 表示言語は設定ファイルで決まるので、何かを表示する前に読む
    let config_path = config::default_path();
    let vimrc_path = vimrc::default_path();
    let loaded = load_config(&config_path);
    let path = match std::env::args().nth(1).as_deref() {
        Some("-h" | "--help") => {
            println!(
                "{}",
                t!(
                    "Usage: curl_tui [collection-file.json]",
                    "使い方: curl_tui [コレクションファイル.json]"
                )
            );
            println!(
                "{}",
                tf!(
                    "Default: {p}",
                    "既定: {p}",
                    p = collection::default_path().display()
                )
            );
            println!(
                "{}",
                tf!(
                    "Config file: {p} (edit with :config)",
                    "設定ファイル: {p} (:config で編集)",
                    p = config_path.display()
                )
            );
            println!(
                "{}",
                tf!(
                    "vimrc (Vim-style mappings): {p} (edit with :vimrc)",
                    "vimrc (Vim 形式のマッピング): {p} (:vimrc で編集)",
                    p = vimrc_path.display()
                )
            );
            return Ok(());
        }
        Some(p) => PathBuf::from(p),
        None => collection::default_path(),
    };
    let requests = collection::load(&path)?;
    let mut app = App::new(requests);
    if let Some(zoom) = std::env::var("CURL_TUI_PREVIEW_ZOOM")
        .ok()
        .and_then(|v| v.trim().trim_end_matches('%').parse::<i32>().ok())
    {
        app.set_preview_zoom(zoom);
        app.status.clear();
    }
    let vimrc_error = vimrc::load(&vimrc_path)
        .map(|vimrc| app.set_vimrc(vimrc))
        .err();
    match loaded {
        Ok(config) => {
            app.set_keymap(config.keymap);
            if let Some(e) = vimrc_error {
                app.status = tf!(
                    "vimrc error (starting without mappings): {e:#}",
                    "vimrc のエラー (マッピングなしで起動): {e:#}"
                )
            }
        }
        // config.jsonc のエラーがあれば、vimrc のエラーよりそちらを表示する
        // 設定が壊れていても既定のキーで起動する
        Err(e) => {
            app.status = tf!(
                "Config error (starting with the default keys): {e:#}",
                "設定エラー (既定のキーで起動): {e:#}"
            )
        }
    }
    let browser = preview::find_browser();
    app.preview_unavailable = preview_unavailable(browser.as_deref());

    // ratatui::init は raw mode / 代替画面 / パニック時の復元フックを設定する
    let mut terminal = ratatui::init();
    // パニック時もマウスキャプチャを解除してから端末を復元する
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(std::io::stdout(), DisableMouseCapture);
        hook(info);
    }));
    enable_extras(enhanced())?;
    let files = ConfigFiles {
        config: config_path,
        vimrc: vimrc_path,
    };
    let result = run(&mut terminal, &mut app, &path, &files, browser).await;
    if app.preview_unavailable.is_none() {
        let mut out = std::io::stdout();
        let _ = out.write_all(kitty::free().as_bytes());
        let _ = out.flush();
    }
    disable_extras(enhanced());
    ratatui::restore();
    result
}

/// 設定ファイルを読み、表示言語を切り替えて設定を返す。キーマップなどに
/// エラーがあっても `"lang"` が読めれば言語だけは反映する (エラーもその言語で出す)。
fn load_config(path: &Path) -> Result<config::Config> {
    if let Some(lang) = config::peek_lang(path) {
        i18n::set(lang);
    }
    let config = config::load(path)?;
    i18n::set(config.lang);
    Ok(config)
}

/// HTML プレビューを表示できない理由 (表示できるなら `None`)。
fn preview_unavailable(browser: Option<&Path>) -> Option<String> {
    match (kitty::detect(), browser) {
        (Err(e), _) => Some(e),
        (Ok(()), None) => Some(
            t!(
                "no Chromium-based browser was found (set CURL_TUI_BROWSER to the executable)",
                "Chromium 系のブラウザが見つかりません (CURL_TUI_BROWSER で実行ファイルを指定できます)"
            )
            .into(),
        ),
        (Ok(()), Some(_)) => None,
    }
}

/// 通常の端末では Shift+Esc が素の Esc と区別できない。kitty keyboard protocol に
/// 対応した端末では修飾キー付きの Esc を受け取れるようにする。
fn enhanced() -> bool {
    static ENHANCED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENHANCED.get_or_init(|| supports_keyboard_enhancement().unwrap_or(false))
}

/// raw mode / 代替画面以外の端末設定 (ペースト・マウス・キーボード拡張) を有効にする。
fn enable_extras(enhanced: bool) -> std::io::Result<()> {
    execute!(std::io::stdout(), EnableBracketedPaste, EnableMouseCapture)?;
    if enhanced {
        execute!(
            std::io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
    }
    Ok(())
}

fn disable_extras(enhanced: bool) {
    if enhanced {
        let _ = execute!(std::io::stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(
        std::io::stdout(),
        DisableBracketedPaste,
        DisableMouseCapture,
        SetCursorStyle::DefaultUserShape
    );
}

/// 設定ファイルの場所。
struct ConfigFiles {
    /// config.jsonc (表示言語と "keymap")
    config: PathBuf,
    /// vimrc (Vim 形式のマッピング)
    vimrc: PathBuf,
}

/// エディタで開く設定ファイル。
#[derive(Debug, Clone, Copy)]
enum EditTarget {
    Config,
    Vimrc,
}

/// config.jsonc を読み直して反映する。戻り値はステータス行に出す文言。
fn reload_config(app: &mut App, path: &Path) -> String {
    match load_config(path) {
        Ok(config) => {
            let count = config.keymap.user_count();
            app.set_keymap(config.keymap);
            tf!(
                "Config applied ({count} key bindings): {p}",
                "設定を反映しました (キーバインド {count} 件): {p}",
                p = path.display()
            )
        }
        Err(e) => tf!(
            "Config error (not applied): {e:#}",
            "設定エラー (未反映): {e:#}"
        ),
    }
}

/// vimrc を読み直して反映する。戻り値はステータス行に出す文言。
fn reload_vimrc(app: &mut App, path: &Path) -> String {
    match vimrc::load(path) {
        Ok(vimrc) => {
            let count = vimrc.maps.len();
            app.set_vimrc(vimrc);
            tf!(
                "vimrc applied ({count} mappings): {p}",
                "vimrc を反映しました (マッピング {count} 件): {p}",
                p = path.display()
            )
        }
        Err(e) => tf!(
            "vimrc error (not applied): {e:#}",
            "vimrc のエラー (未反映): {e:#}"
        ),
    }
}

/// TUI を一時的に抜けて `path` をエディタ ($VISUAL / $EDITOR) で開き、閉じるまで待つ。
/// エディタを起動できなければ、ステータス行に出す文言を返す。
fn run_editor(terminal: &mut DefaultTerminal, path: &Path) -> Result<Result<(), String>> {
    disable_extras(enhanced());
    ratatui::restore();
    let (program, args) = config::editor_command();
    let edited = std::process::Command::new(&program)
        .args(&args)
        .arg(path)
        .status();
    // 端末を作り直して全体を描き直す (Terminal::clear はカーソル位置の問い合わせで失敗しうる)
    *terminal = ratatui::init();
    enable_extras(enhanced())?;
    Ok(edited.map(|_| ()).map_err(|e| {
        tf!(
            "Cannot launch the editor `{program}` (check $EDITOR): {e}",
            "エディタ `{program}` を起動できません ($EDITOR を確認してください): {e}"
        )
    }))
}

/// TUI を一時的に抜けて設定ファイルをエディタで開き、閉じたら読み直す。
fn edit_config(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    target: EditTarget,
    files: &ConfigFiles,
    browser: Option<&Path>,
) -> Result<()> {
    let (path, prepared) = match target {
        EditTarget::Config => (&files.config, config::prepare(&files.config)),
        EditTarget::Vimrc => (&files.vimrc, vimrc::prepare(&files.vimrc)),
    };
    if let Err(e) = prepared {
        app.status = tf!(
            "Cannot prepare the config file: {e:#}",
            "設定ファイルを用意できません: {e:#}"
        );
        return Ok(());
    }
    app.status = match run_editor(terminal, path)? {
        Err(msg) => msg,
        Ok(()) => match target {
            EditTarget::Config => reload_config(app, path),
            EditTarget::Vimrc => reload_vimrc(app, path),
        },
    };
    // 言語が変わったかもしれないので、表示済みの文言を作り直す
    app.preview_unavailable = preview_unavailable(browser);
    Ok(())
}

/// テキスト欄の内容を一時ファイルに書いてエディタで開き、閉じたら書き戻す (lazygit と同じ方式)。
fn edit_text(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    field: TextField,
    text: &str,
) -> Result<()> {
    let path = std::env::temp_dir().join(format!(
        "curl_tui-{}-{}.{}",
        std::process::id(),
        field.name().to_ascii_lowercase(),
        temp_extension(field, text)
    ));
    // 多くのエディタは最終行に改行を付けるので、こちらも付けて渡す (戻すときに 1 つ取る)
    let content = if text.is_empty() {
        String::new()
    } else {
        format!("{text}\n")
    };
    if let Err(e) = std::fs::write(&path, content) {
        app.status = tf!(
            "Cannot create a temporary file: {e}",
            "一時ファイルを作れません: {e}"
        );
        return Ok(());
    }
    let result = run_editor(terminal, &path)?;
    let edited = std::fs::read_to_string(&path);
    let _ = std::fs::remove_file(&path);
    match (result, edited) {
        (Err(msg), _) => app.status = msg,
        (Ok(()), Err(e)) => {
            app.status = tf!(
                "Cannot read the edited file (not applied): {e}",
                "編集したファイルを読めません (未反映): {e}"
            )
        }
        (Ok(()), Ok(edited)) => app.apply_external_edit(field, &edited),
    }
    Ok(())
}

/// 一時ファイルの拡張子。エディタの構文ハイライトが効くよう、Body は中身から推測する。
fn temp_extension(field: TextField, text: &str) -> &'static str {
    match field {
        TextField::Body => {
            let t = text.trim_start();
            if t.starts_with('{') || t.starts_with('[') {
                "json"
            } else if t.starts_with('<') {
                if t.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("<?xml")) {
                    "xml"
                } else {
                    "html"
                }
            } else {
                "txt"
            }
        }
        _ => "txt",
    }
}

/// 描画結果 (どのレスポンスのものか, 描画した幅のセル数, 結果)。
type PreviewResult = (u64, u16, Result<preview::Rendered, String>);

async fn run(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    path: &Path,
    files: &ConfigFiles,
    browser: Option<PathBuf>,
) -> Result<()> {
    let client = http::build_client(Duration::from_secs(30))?;
    let (tx, mut rx) = mpsc::unbounded_channel::<Result<Response, String>>();
    let (ptx, mut prx) = mpsc::unbounded_channel::<PreviewResult>();
    // 画像のピクセルサイズ (表示範囲の計算用) と、現在の表示位置
    let mut image_size = (0u32, 0u32);
    let mut placed: Option<kitty::Placement> = None;
    let mut events = EventStream::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(100));

    let mut cursor_mode = None;
    'main: loop {
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
            update_placement(app, image_size, cell_w, cell_h, &mut placed)?;
        }

        let mut open_config = None;
        let mut open_text = None;
        // "vimrc" のマッピングの続きのキーを待っているなら、その期限
        let deadline = app.key_deadline();
        let mut action = tokio::select! {
            maybe = events.next() => {
                let Some(event) = maybe else { break };
                let event = event?;
                if matches!(event, Event::Resize(..)) {
                    // 画面がクリアされると画像も消えるので置き直す
                    placed = None;
                }
                app.handle_event(event)
            }
            _ = sleep_until(deadline), if deadline.is_some() => app.key_timeout(),
            Some(res) = rx.recv() => {
                app.set_response(res);
                Action::None
            }
            Some((generation, cols, res)) = prx.recv() => {
                if generation != app.preview_generation {
                    continue; // 別のレスポンスの描画結果
                }
                match res {
                    Ok(img) => {
                        let mut out = std::io::stdout();
                        out.write_all(kitty::transmit_png(&img.png).as_bytes())?;
                        out.flush()?;
                        image_size = (img.width, img.height);
                        placed = None;
                        let (_, cell_h) = cell_size();
                        let rows = img.height.div_ceil(cell_h).min(u16::MAX as u32) as u16;
                        app.preview = PreviewState::Ready { cols, rows };
                    }
                    Err(e) => app.preview = PreviewState::Failed(e),
                }
                Action::None
            }
            _ = ticker.tick(), if app.is_busy() => {
                app.tick();
                Action::None
            }
        };
        // マッピングで 1 度に複数のキーが入ると、Action が複数出ることがある
        loop {
            match action {
                Action::None => {}
                Action::Quit => break 'main,
                Action::Send(req) => {
                    let client = client.clone();
                    let tx = tx.clone();
                    // 送信は別タスクで行い UI をブロックしない
                    tokio::spawn(async move {
                        let res = http::send(&client, &req)
                            .await
                            .map_err(|e| format!("{e:#}"));
                        let _ = tx.send(res);
                    });
                }
                Action::Save(reqs) => {
                    app.status = match collection::save(path, &reqs) {
                        Ok(()) => tf!("Saved: {p}", "保存しました: {p}", p = path.display()),
                        Err(e) => tf!("Failed to save: {e:#}", "保存に失敗: {e:#}"),
                    };
                }
                Action::SaveAndQuit(reqs) => match collection::save(path, &reqs) {
                    Ok(()) => break 'main,
                    // 保存できなければ終了しない
                    Err(e) => app.status = tf!("Failed to save: {e:#}", "保存に失敗: {e:#}"),
                },
                Action::EditConfig => open_config = Some(EditTarget::Config),
                Action::EditVimrc => open_config = Some(EditTarget::Vimrc),
                Action::EditText { field, text } => open_text = Some((field, text)),
                Action::Copy(text) => {
                    if let Err(e) = clipboard::copy(&text, &mut std::io::stdout()) {
                        app.status = tf!("Failed to copy: {e}", "コピーに失敗: {e}");
                    }
                }
            }
            if !app.has_queued_keys() {
                break;
            }
            action = app.resume_keys();
        }
        if open_config.is_some() || open_text.is_some() {
            // 入力の読み取りを止めてからエディタに端末を渡す (止めないとキー入力を奪い合う)
            drop(events);
            if placed.take().is_some() {
                let mut out = std::io::stdout();
                out.write_all(kitty::hide().as_bytes())?;
                out.flush()?;
            }
            let edited = match (open_config, open_text) {
                (Some(target), _) => edit_config(terminal, app, target, files, browser.as_deref()),
                (None, Some((field, text))) => edit_text(terminal, app, field, &text),
                (None, None) => Ok(()),
            };
            events = EventStream::new();
            cursor_mode = None;
            edited?;
        }
    }
    Ok(())
}

/// `deadline` まで待つ (None なら呼ばれない。select! の条件で除外する)。
async fn sleep_until(deadline: Option<std::time::Instant>) {
    if let Some(deadline) = deadline {
        tokio::time::sleep_until(deadline.into()).await;
    }
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
        // 高解像度の画面でも端末の文字と同程度の大きさになるよう、セルの大きさに合わせる
        scale_percent: preview::auto_scale_percent(cell_h) * app.preview_zoom as u32 / 100,
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
    (image_width, image_height): (u32, u32),
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
                // 倍率の端数で画像の幅がペインと 1px ほどずれることがある
                src_w: (area.width as u32 * cell_w).min(image_width.max(1)),
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
