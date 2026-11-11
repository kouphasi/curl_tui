//! HTML レスポンスをヘッドレス Chromium で PNG に描画する。
//!
//! ブラウザは `CURL_TUI_BROWSER` で指定でき、未指定なら PATH などから
//! Chromium 系のブラウザ (chromium / google-chrome / msedge など) を探す。

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, bail};

/// 描画済みのページ画像。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub png: Vec<u8>,
    pub width: u32,
    /// ページ下端の余白 (最終行と同じ色の行) を除いた高さ
    pub height: u32,
}

/// 描画の依頼内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub html: String,
    /// 相対パスの画像や CSS を解決するための URL
    pub base_url: String,
    pub width: u32,
    /// 撮影する高さ (これより長いページは切り捨てる)
    pub height: u32,
}

/// 撮影する高さの上限 (ピクセル)。
pub const MAX_HEIGHT: u32 = 8000;

const CANDIDATES: &[&str] = &[
    // 撮影専用の軽量版 (npx @puppeteer/browsers install chrome-headless-shell) があれば優先する
    "chrome-headless-shell",
    "chromium",
    "chromium-browser",
    "google-chrome",
    "google-chrome-stable",
    "microsoft-edge",
    "brave-browser",
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
];

/// 使用するブラウザの実行ファイルを探す。
pub fn find_browser() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("CURL_TUI_BROWSER") {
        return Some(PathBuf::from(p));
    }
    CANDIDATES.iter().find_map(|c| which(c))
}

fn which(name: &str) -> Option<PathBuf> {
    let p = Path::new(name);
    if p.is_absolute() {
        return p.is_file().then(|| p.to_path_buf());
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// `<base href>` を差し込み、相対 URL がレスポンスの URL 基準で解決されるようにする。
pub fn inject_base(html: &str, base_url: &str) -> String {
    let lower = html.to_ascii_lowercase();
    if base_url.is_empty() || lower.contains("<base ") {
        return html.to_string();
    }
    let escaped = base_url.replace('&', "&amp;").replace('"', "&quot;");
    let tag = format!("<base href=\"{escaped}\">");
    // <head> または <head ...> の直後に入れる。見つからなければ先頭に置く
    // (先頭に置いてもブラウザは head 内のものとして扱う)
    let head = lower
        .match_indices("<head")
        .find(|(i, _)| {
            matches!(
                lower.as_bytes().get(i + 5),
                Some(b'>' | b' ' | b'\t' | b'\n' | b'\r')
            )
        })
        .and_then(|(i, _)| lower[i..].find('>').map(|end| i + end + 1));
    match head {
        Some(pos) => format!("{}{tag}{}", &html[..pos], &html[pos..]),
        None => format!("{tag}{html}"),
    }
}

/// HTML を描画して PNG を返す。
pub async fn render(browser: &Path, job: &Job) -> anyhow::Result<Rendered> {
    let dir = TempDir::new()?;
    let html_path = dir.path().join("page.html");
    let png_path = dir.path().join("page.png");
    std::fs::write(&html_path, inject_base(&job.html, &job.base_url))
        .context("一時ファイルの作成に失敗しました")?;

    let mut cmd = tokio::process::Command::new(browser);
    cmd.arg("--headless")
        .arg("--disable-gpu")
        .arg("--hide-scrollbars")
        .arg("--mute-audio")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-extensions")
        .arg("--force-device-scale-factor=1")
        // スクリプトや画像の読み込みを少し待ってから撮影する
        .arg("--virtual-time-budget=3000")
        .arg(format!(
            "--user-data-dir={}",
            dir.path().join("profile").display()
        ))
        .arg(format!("--window-size={},{}", job.width, job.height))
        .arg(format!("--screenshot={}", png_path.display()))
        .arg(file_url(&html_path));
    if running_as_root() {
        // root では Chromium のサンドボックスが起動できない
        cmd.arg("--no-sandbox");
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let child = cmd
        .spawn()
        .with_context(|| format!("ブラウザを起動できません: {}", browser.display()))?;
    let output = tokio::time::timeout(Duration::from_secs(20), child.wait_with_output())
        .await
        .context("ブラウザでの描画がタイムアウトしました")??;
    if !png_path.is_file() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let last = stderr
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
        bail!("ブラウザでの描画に失敗しました ({}): {last}", output.status);
    }
    let png = std::fs::read(&png_path).context("スクリーンショットを読めません")?;
    let (width, height) = content_size(&png)?;
    Ok(Rendered { png, width, height })
}

fn file_url(path: &Path) -> String {
    let s = path.display().to_string().replace('\\', "/");
    let s = s
        .replace('%', "%25")
        .replace(' ', "%20")
        .replace('#', "%23");
    if s.starts_with('/') {
        format!("file://{s}")
    } else {
        format!("file:///{s}")
    }
}

#[cfg(unix)]
fn running_as_root() -> bool {
    use std::os::unix::fs::MetadataExt;
    // /proc/self の所有者 = 実行ユーザー (Linux)。取れなければ HOME で推測する
    std::fs::metadata("/proc/self")
        .map(|m| m.uid() == 0)
        .unwrap_or_else(|_| std::env::var("USER").is_ok_and(|u| u == "root"))
}

#[cfg(not(unix))]
fn running_as_root() -> bool {
    false
}

/// PNG の幅と、下端の余白を除いた高さを返す。
///
/// ウィンドウの高さで撮影するため、短いページは下が背景色で埋まる。
/// 最終行と同じ色だけの行を下から取り除き、スクロールできる範囲を内容に合わせる。
pub fn content_size(png: &[u8]) -> anyhow::Result<(u32, u32)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().context("PNG を解析できません")?;
    let mut buf = vec![0; reader.output_buffer_size().context("PNG が大きすぎます")?];
    let info = reader
        .next_frame(&mut buf)
        .context("PNG を展開できません")?;
    let (w, h) = (info.width, info.height);
    if w == 0 || h == 0 {
        return Ok((w, h));
    }
    let stride = info.line_size;
    let row = |y: u32| &buf[y as usize * stride..(y as usize + 1) * stride];
    let bpp = stride / w as usize;
    let bottom = row(h - 1);
    let bg = &bottom[..bpp];
    let uniform = |r: &[u8]| r.chunks_exact(bpp).all(|px| px == bg);
    if !uniform(bottom) {
        return Ok((w, h));
    }
    let content = (0..h)
        .rev()
        .find(|&y| !uniform(row(y)))
        .map_or(1, |y| y + 1);
    Ok((w, content))
}

/// 終了時に削除される一時ディレクトリ。
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> anyhow::Result<Self> {
        use std::sync::atomic::{AtomicU32, Ordering};
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "curl_tui-preview-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        std::fs::create_dir_all(&path).context("一時ディレクトリの作成に失敗しました")?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(w: u32, h: u32, pixel: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().unwrap();
        let mut data = Vec::new();
        for y in 0..h {
            for x in 0..w {
                data.extend(pixel(x, y));
            }
        }
        writer.write_image_data(&data).unwrap();
        drop(writer);
        out
    }

    #[test]
    fn content_size_trims_bottom_background() {
        let png = encode(
            4,
            10,
            |x, y| if y < 3 && x == 1 { [0, 0, 0] } else { [255; 3] },
        );
        assert_eq!(content_size(&png).unwrap(), (4, 3));
    }

    #[test]
    fn content_size_keeps_full_height_when_bottom_has_content() {
        let png = encode(4, 10, |x, y| {
            if y == 9 && x == 0 {
                [1, 2, 3]
            } else {
                [255; 3]
            }
        });
        assert_eq!(content_size(&png).unwrap(), (4, 10));
    }

    #[test]
    fn content_size_of_blank_page_is_one_row() {
        let png = encode(4, 10, |_, _| [255; 3]);
        assert_eq!(content_size(&png).unwrap(), (4, 1));
    }

    #[test]
    fn inject_base_after_head() {
        assert_eq!(
            inject_base(
                "<html><HEAD lang=x><title>t</title>",
                "https://a.com/x?a=1&b=2"
            ),
            "<html><HEAD lang=x><base href=\"https://a.com/x?a=1&amp;b=2\"><title>t</title>"
        );
        // <header> は <head> ではない
        assert_eq!(
            inject_base("<header>x</header>", "http://a/"),
            "<base href=\"http://a/\"><header>x</header>"
        );
        // 既に base があれば触らない
        assert_eq!(inject_base("<base href=/>", "http://a/"), "<base href=/>");
    }

    #[test]
    fn file_url_escapes_specials() {
        assert_eq!(
            file_url(Path::new("/tmp/a b#c.html")),
            "file:///tmp/a%20b%23c.html"
        );
    }
}
