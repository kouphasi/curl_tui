//! クリップボードへのコピー。
//!
//! OS のコピーコマンド (pbcopy / wl-copy / xclip / xsel) があればそれを使い、
//! あわせて OSC 52 のエスケープシーケンスを端末に送る。OSC 52 は端末側が
//! クリップボードに書き込むので、SSH 越しや Windows でも対応端末なら動く。

use std::{
    io::{self, Write},
    process::{Command, Stdio},
};

use base64::Engine;

/// OSC 52: 端末にクリップボードへの書き込みを依頼するエスケープシーケンス。
pub fn osc52(text: &str) -> String {
    let data = base64::engine::general_purpose::STANDARD.encode(text);
    format!("\x1b]52;c;{data}\x07")
}

/// この環境で使えそうなコピーコマンド (標準入力から読むもの)。
fn commands() -> Vec<(&'static str, &'static [&'static str])> {
    let mut list: Vec<(&'static str, &'static [&'static str])> = Vec::new();
    if cfg!(target_os = "macos") {
        list.push(("pbcopy", &[]));
    } else if cfg!(unix) {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            list.push(("wl-copy", &[]));
        }
        if std::env::var_os("DISPLAY").is_some() {
            list.push(("xclip", &["-selection", "clipboard"]));
            list.push(("xsel", &["--clipboard", "--input"]));
        }
    }
    list
}

/// `text` を標準入力に渡してコマンドを実行する。
fn pipe_to(program: &str, args: &[&str], text: &str) -> io::Result<()> {
    // xclip / wl-copy は常駐するので、出力は受け取らず終了だけを待つ
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes())?;
    }
    if child.wait()?.success() {
        Ok(())
    } else {
        Err(io::Error::other(crate::tf!(
            "{program} failed",
            "{program} が失敗しました"
        )))
    }
}

/// `text` をクリップボードにコピーする。`out` は端末 (OSC 52 の送り先)。
pub fn copy(text: &str, out: &mut impl Write) -> io::Result<()> {
    let _ = commands()
        .iter()
        .any(|(program, args)| pipe_to(program, args, text).is_ok());
    out.write_all(osc52(text).as_bytes())?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc52_encodes_text_as_base64() {
        assert_eq!(osc52("hello"), "\x1b]52;c;aGVsbG8=\x07");
        // マルチバイト文字や改行もそのまま UTF-8 で渡す
        assert_eq!(osc52("あ\n"), "\x1b]52;c;44GCCg==\x07");
    }

    #[test]
    fn pipe_to_reports_missing_command() {
        assert!(pipe_to("curl_tui-no-such-command", &[], "x").is_err());
    }
}
