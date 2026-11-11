//! kitty graphics protocol のエスケープシーケンス生成と対応端末の判定。
//!
//! 仕様: <https://sw.kovidgoyal.net/kitty/graphics-protocol/>
//! 画像データは 1 度だけ送り (`a=t`)、スクロール時は表示元の矩形を変えて
//! 同じ placement を置き直す (`a=p`) ことで、再送信なしにスクロールさせる。

use base64::Engine;

/// curl_tui が使う画像 ID (他のアプリの画像と衝突しにくい値にしておく)。
pub const IMAGE_ID: u32 = 0x6375_726c; // "curl"
/// 画像の placement ID。同じ ID で置き直すと kitty 側で置き換えてくれる。
pub const PLACEMENT_ID: u32 = 1;

/// 端末が kitty graphics protocol に対応していそうか。
///
/// `CURL_TUI_GRAPHICS=kitty` / `none` で強制的に切り替えられる。
pub fn detect() -> Result<(), String> {
    detect_from(|k| std::env::var(k).ok())
}

fn detect_from(env: impl Fn(&str) -> Option<String>) -> Result<(), String> {
    match env("CURL_TUI_GRAPHICS").as_deref() {
        Some("kitty") => return Ok(()),
        Some("none") => return Err("CURL_TUI_GRAPHICS=none で無効化されています".into()),
        _ => {}
    }
    if env("TMUX").is_some() {
        return Err(
            "tmux 内では画像を表示できません (CURL_TUI_GRAPHICS=kitty で強制有効化)".into(),
        );
    }
    let term = env("TERM").unwrap_or_default();
    let term_program = env("TERM_PROGRAM").unwrap_or_default();
    let supported = env("KITTY_WINDOW_ID").is_some()
        || env("GHOSTTY_RESOURCES_DIR").is_some()
        || env("WEZTERM_EXECUTABLE").is_some()
        || term.contains("kitty")
        || term.contains("ghostty")
        || matches!(term_program.as_str(), "ghostty" | "WezTerm");
    if supported {
        Ok(())
    } else {
        Err(format!(
            "この端末 (TERM={term}) は kitty graphics protocol に未対応のようです \
             (kitty / Ghostty / WezTerm で利用可。CURL_TUI_GRAPHICS=kitty で強制有効化)"
        ))
    }
}

/// PNG 画像を送信する (表示はしない)。同じ ID の古い画像は置き換えられる。
pub fn transmit_png(png: &[u8]) -> String {
    const CHUNK: usize = 4096;
    let data = base64::engine::general_purpose::STANDARD.encode(png);
    let chunks: Vec<&[u8]> = data.as_bytes().chunks(CHUNK).collect();
    let mut out = String::with_capacity(data.len() + chunks.len() * 32);
    for (i, chunk) in chunks.iter().enumerate() {
        let more = u8::from(i + 1 < chunks.len());
        // 制御データは最初のチャンクにだけ付ける
        if i == 0 {
            out.push_str(&format!("\x1b_Ga=t,f=100,i={IMAGE_ID},q=2,m={more};"));
        } else {
            out.push_str(&format!("\x1b_Gm={more};"));
        }
        // base64 は ASCII なので from_utf8 は失敗しない
        out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        out.push_str("\x1b\\");
    }
    out
}

/// 画像の表示位置と表示範囲。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// 表示先の左上セル (0 始まり)
    pub col: u16,
    pub row: u16,
    /// 表示先のセル数
    pub cols: u16,
    pub rows: u16,
    /// 画像中の切り出し範囲 (ピクセル)
    pub src_y: u32,
    pub src_w: u32,
    pub src_h: u32,
}

/// 送信済みの画像を指定のセル位置に表示する。カーソル位置は保存・復元する。
pub fn place(p: &Placement) -> String {
    format!(
        "\x1b7\x1b[{};{}H\x1b_Ga=p,i={IMAGE_ID},p={PLACEMENT_ID},x=0,y={},w={},h={},c={},r={},C=1,q=2\x1b\\\x1b8",
        p.row + 1,
        p.col + 1,
        p.src_y,
        p.src_w,
        p.src_h,
        p.cols,
        p.rows,
    )
}

/// 表示を消す (画像データは残すので、すぐに再表示できる)。
pub fn hide() -> String {
    format!("\x1b_Ga=d,d=i,i={IMAGE_ID},p={PLACEMENT_ID},q=2\x1b\\")
}

/// 表示を消し、画像データも解放する。
pub fn free() -> String {
    format!("\x1b_Ga=d,d=I,i={IMAGE_ID},q=2\x1b\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            vars.iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn detects_known_terminals_and_overrides() {
        assert!(detect_from(env(&[("KITTY_WINDOW_ID", "1")])).is_ok());
        assert!(detect_from(env(&[("TERM", "xterm-ghostty")])).is_ok());
        assert!(detect_from(env(&[("TERM_PROGRAM", "WezTerm")])).is_ok());
        assert!(detect_from(env(&[("TERM", "xterm-256color")])).is_err());
        assert!(detect_from(env(&[("KITTY_WINDOW_ID", "1"), ("TMUX", "x")])).is_err());
        assert!(detect_from(env(&[("TERM", "dumb"), ("CURL_TUI_GRAPHICS", "kitty")])).is_ok());
        assert!(
            detect_from(env(&[
                ("KITTY_WINDOW_ID", "1"),
                ("CURL_TUI_GRAPHICS", "none")
            ]))
            .is_err()
        );
    }

    #[test]
    fn transmit_splits_into_chunks() {
        let png = vec![0u8; 5000]; // base64 で 6668 文字 → 2 チャンク
        let out = transmit_png(&png);
        assert!(out.starts_with(&format!("\x1b_Ga=t,f=100,i={IMAGE_ID},q=2,m=1;")));
        assert!(out.contains("\x1b\\\x1b_Gm=0;"));
        assert_eq!(out.matches("\x1b_G").count(), 2);
        assert!(out.ends_with("\x1b\\"));
    }

    #[test]
    fn single_chunk_has_no_continuation() {
        let out = transmit_png(b"abc");
        assert_eq!(
            out,
            format!("\x1b_Ga=t,f=100,i={IMAGE_ID},q=2,m=0;YWJj\x1b\\")
        );
    }

    #[test]
    fn place_moves_cursor_and_restores_it() {
        let out = place(&Placement {
            col: 30,
            row: 20,
            cols: 80,
            rows: 10,
            src_y: 64,
            src_w: 800,
            src_h: 200,
        });
        assert!(out.starts_with("\x1b7\x1b[21;31H"));
        assert!(out.contains("y=64,w=800,h=200,c=80,r=10,C=1"));
        assert!(out.ends_with("\x1b8"));
    }
}
