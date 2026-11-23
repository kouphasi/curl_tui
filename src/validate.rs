//! Params / Headers / Body のテキストの検査。
//!
//! 画面とは独立した関数で、テキストを受け取って問題点の一覧 ([`Diagnostic`]) を返す。
//! `#` で始まる行 (無効にした行) は検査しない。

use reqwest::header::{HeaderName, HeaderValue};

use crate::model::HttpMethod;
use crate::{t, tf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// 送るとおかしなリクエストになる / 送信に失敗する
    Error,
    /// 送れるが、意図と違う可能性が高い
    Warning,
}

/// 1 件の問題点。`line` / `col` は 0 始まりの行と文字単位の列。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub line: usize,
    pub col: usize,
    pub severity: Severity,
    pub message: String,
}

impl Diagnostic {
    fn error(line: usize, col: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            col,
            severity: Severity::Error,
            message: message.into(),
        }
    }

    fn warning(line: usize, col: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            col,
            severity: Severity::Warning,
            message: message.into(),
        }
    }
}

/// 有効な (空でも `#` 始まりでもない) 行を、行番号・行頭の空白を除いた位置・中身と一緒に返す。
fn active_lines(text: &str) -> impl Iterator<Item = (usize, usize, &str)> {
    text.lines().enumerate().filter_map(|(i, raw)| {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let indent = raw.chars().count() - raw.trim_start().chars().count();
        Some((i, indent, line))
    })
}

/// Params (`key=value` 形式) を検査する。
pub fn check_params(text: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (i, indent, line) in active_lines(text) {
        match line.split_once('=') {
            Some((key, _)) if key.trim().is_empty() => out.push(Diagnostic::error(
                i,
                indent,
                t!(
                    "The key is empty (this line is ignored)",
                    "キーが空です (この行は送信されません)"
                ),
            )),
            Some(_) => {}
            None => out.push(Diagnostic::warning(
                i,
                indent + line.chars().count(),
                tf!(
                    "No `=`: sent as `{line}=` (empty value)",
                    "`=` がありません: 値が空の `{line}=` として送ります"
                ),
            )),
        }
    }
    out
}

/// Headers (`Key: Value` 形式) を検査する。
pub fn check_headers(text: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut content_type_seen = false;
    for (i, indent, line) in active_lines(text) {
        let Some((name_raw, value_raw)) = line.split_once(':') else {
            out.push(Diagnostic::error(
                i,
                indent + line.chars().count(),
                t!(
                    "No `:` (write it as `Name: value`)",
                    "`:` がありません (`Name: value` の形で書いてください)"
                ),
            ));
            continue;
        };
        let name = name_raw.trim();
        let value = value_raw.trim();
        if name.is_empty() {
            out.push(Diagnostic::error(
                i,
                indent,
                t!("The header name is empty", "ヘッダ名が空です"),
            ));
            continue;
        }
        if HeaderName::from_bytes(name.as_bytes()).is_err() {
            let col = name
                .chars()
                .position(|c| !is_token_char(c))
                .map_or(indent, |p| indent + p);
            out.push(Diagnostic::error(
                i,
                col,
                tf!(
                    "Invalid header name `{name}` (spaces and most symbols are not allowed)",
                    "ヘッダ名 `{name}` が不正です (空白やほとんどの記号は使えません)"
                ),
            ));
            continue;
        }
        let value_start = indent
            + name_raw.chars().count()
            + 1
            + (value_raw.chars().count() - value_raw.trim_start().chars().count());
        if HeaderValue::from_str(value).is_err() {
            let col = value
                .chars()
                .position(|c| c != '\t' && c.is_control())
                .map_or(value_start, |p| value_start + p);
            out.push(Diagnostic::error(
                i,
                col,
                t!(
                    "The header value contains control characters",
                    "ヘッダの値に制御文字が含まれています"
                ),
            ));
            continue;
        }
        if let Some(p) = value.chars().position(|c| !c.is_ascii()) {
            out.push(Diagnostic::warning(
                i,
                value_start + p,
                t!(
                    "Non-ASCII header value: sent as raw UTF-8 bytes, which many servers misread (percent-encode or Base64 it)",
                    "ヘッダの値に ASCII 以外の文字があります: UTF-8 のまま送られ、読み違えるサーバーが多いです (パーセントエンコードか Base64 にしてください)"
                ),
            ));
        }
        if name.eq_ignore_ascii_case("content-type") {
            if content_type_seen {
                out.push(Diagnostic::warning(
                    i,
                    indent,
                    t!(
                        "Content-Type is set more than once",
                        "Content-Type が複数あります"
                    ),
                ));
            }
            content_type_seen = true;
        } else if name.eq_ignore_ascii_case("content-length") || name.eq_ignore_ascii_case("host") {
            out.push(Diagnostic::warning(
                i,
                indent,
                tf!(
                    "{name} is set automatically; setting it by hand can break the request",
                    "{name} は自動で付きます。手で書くとリクエストが壊れることがあります"
                ),
            ));
        }
    }
    out
}

/// RFC 9110 の token に使える文字か (ヘッダ名に使える文字)。
fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c)
}

/// Body を検査する。`content_type` は有効な Content-Type ヘッダの値 (無ければ None)。
pub fn check_body(body: &str, content_type: Option<&str>, method: HttpMethod) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if body.is_empty() {
        return out;
    }
    if matches!(method, HttpMethod::Get | HttpMethod::Head) {
        out.push(Diagnostic::warning(
            0,
            0,
            tf!(
                "{method} with a body: many servers ignore or reject it",
                "{method} に Body があります: 多くのサーバーは無視するか拒否します"
            ),
        ));
    }
    let mime = content_type.map(|ct| {
        ct.split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase()
    });
    match mime.as_deref() {
        Some(m) if m == "application/json" || m.ends_with("+json") => {
            if let Some(d) = check_json(body, Severity::Error) {
                out.push(d);
            }
        }
        Some("application/x-www-form-urlencoded") => out.extend(check_form(body)),
        Some(_) => {}
        None => {
            let t = body.trim_start();
            if t.starts_with('{') || t.starts_with('[') {
                match check_json(body, Severity::Warning) {
                    Some(d) => out.push(d),
                    None => out.push(Diagnostic::warning(
                        0,
                        0,
                        t!(
                            "No Content-Type header: add `Content-Type: application/json` to send JSON",
                            "Content-Type ヘッダがありません: JSON を送るなら `Content-Type: application/json` を足してください"
                        ),
                    )),
                }
            }
        }
    }
    out
}

/// JSON として読めるか。読めなければ壊れている位置の問題点を返す。
fn check_json(body: &str, severity: Severity) -> Option<Diagnostic> {
    let err = serde_json::from_str::<serde_json::Value>(body).err()?;
    // serde_json の行・列は 1 始まりで、列はバイト単位 (行末の後ろを指すこともある)
    let line = err.line().saturating_sub(1);
    let text = body.lines().nth(line).unwrap_or("");
    let byte = err.column().saturating_sub(1).min(text.len());
    let col = text.char_indices().take_while(|(b, _)| *b < byte).count();
    let reason = err.to_string();
    // 末尾の " at line X column Y" は行番号として別に出すので取り除く
    let reason = reason
        .rfind(" at line ")
        .map_or(reason.as_str(), |p| &reason[..p]);
    Some(Diagnostic {
        line,
        col,
        severity,
        message: tf!("Invalid JSON: {reason}", "JSON が不正です: {reason}"),
    })
}

/// `application/x-www-form-urlencoded` の Body (`a=1&b=2`) を検査する。
fn check_form(body: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (i, line) in body.lines().enumerate() {
        let mut col = 0;
        for pair in line.split('&') {
            let len = pair.chars().count();
            if pair.is_empty() {
                // `a=1&&b=2` や末尾の `&` は無害なので何も言わない
            } else if pair.starts_with('=') {
                out.push(Diagnostic::warning(
                    i,
                    col,
                    t!("The key is empty", "キーが空です"),
                ));
            } else if pair.contains(' ') {
                out.push(Diagnostic::warning(
                    i,
                    col + pair.chars().position(|c| c == ' ').unwrap_or(0),
                    t!(
                        "Spaces must be encoded as `+` or `%20`",
                        "空白は `+` か `%20` にエンコードしてください"
                    ),
                ));
            }
            col += len + 1;
        }
    }
    out
}

/// 有効な Headers の行から Content-Type の値を探す (複数あれば最後のもの)。
pub fn content_type(headers: &str) -> Option<String> {
    active_lines(headers)
        .filter_map(|(_, _, line)| line.split_once(':'))
        .filter(|(name, _)| name.trim().eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| value.trim().to_string())
        .last()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(ds: &[Diagnostic]) -> Vec<(usize, usize, Severity)> {
        ds.iter().map(|d| (d.line, d.col, d.severity)).collect()
    }

    #[test]
    fn params_report_empty_keys_and_missing_equals() {
        let ds = check_params("a=1\n=oops\n  flag\n# =disabled\n\nb = two words");
        assert_eq!(
            summary(&ds),
            vec![(1, 0, Severity::Error), (2, 6, Severity::Warning)]
        );
    }

    #[test]
    fn params_allow_repeated_keys() {
        assert!(check_params("a=1\na=2").is_empty());
    }

    #[test]
    fn headers_report_missing_colon_and_bad_names() {
        let ds = check_headers(
            "Accept: */*\nContent-Type application/json\nX Foo: 1\n: x\n# Bad Name\nX-Time: 12:30",
        );
        assert_eq!(
            summary(&ds),
            vec![
                (1, 29, Severity::Error),
                (2, 1, Severity::Error),
                (3, 0, Severity::Error),
            ]
        );
    }

    #[test]
    fn headers_report_non_ascii_and_control_characters() {
        let ds = check_headers("X-Name: 山田\nX-Bad: a\u{7f}b");
        assert_eq!(
            summary(&ds),
            vec![(0, 8, Severity::Warning), (1, 8, Severity::Error)]
        );
    }

    #[test]
    fn headers_warn_on_duplicates_and_automatic_headers() {
        let ds =
            check_headers("Content-Type: a/b\ncontent-type: c/d\nHost: x\n# Content-Length: 1");
        assert_eq!(
            summary(&ds),
            vec![(1, 0, Severity::Warning), (2, 0, Severity::Warning)]
        );
    }

    #[test]
    fn json_body_errors_point_at_the_broken_position() {
        let body = "{\n  \"名前\": 1,\n  \"b\": 2,\n}";
        let ds = check_body(
            body,
            Some("application/json; charset=utf-8"),
            HttpMethod::Post,
        );
        assert_eq!(ds.len(), 1);
        assert_eq!((ds[0].line, ds[0].severity), (3, Severity::Error));
        assert!(!ds[0].message.contains(" at line "));

        // 列は文字単位
        let ds = check_body("{\"名前\": x}", Some("application/json"), HttpMethod::Post);
        assert_eq!(summary(&ds), vec![(0, 7, Severity::Error)]);

        assert!(
            check_body(
                "{\"a\": [1, 2]}",
                Some("application/vnd.api+json"),
                HttpMethod::Post
            )
            .is_empty()
        );
    }

    #[test]
    fn body_without_content_type_is_only_warned() {
        let ds = check_body("{\"a\": 1}", None, HttpMethod::Post);
        assert_eq!(summary(&ds), vec![(0, 0, Severity::Warning)]);
        assert!(ds[0].message.contains("Content-Type"));

        let ds = check_body("{oops", None, HttpMethod::Post);
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0].severity, Severity::Warning);

        assert!(check_body("plain text", None, HttpMethod::Post).is_empty());
        assert!(check_body("<a/>", Some("application/xml"), HttpMethod::Post).is_empty());
    }

    #[test]
    fn get_with_body_is_warned() {
        let ds = check_body("x", Some("text/plain"), HttpMethod::Get);
        assert_eq!(summary(&ds), vec![(0, 0, Severity::Warning)]);
        assert!(check_body("", Some("application/json"), HttpMethod::Get).is_empty());
    }

    #[test]
    fn form_body_is_checked() {
        let ds = check_body(
            "a=1&=x&b=hello world&&c",
            Some("application/x-www-form-urlencoded"),
            HttpMethod::Post,
        );
        assert_eq!(
            summary(&ds),
            vec![(0, 4, Severity::Warning), (0, 14, Severity::Warning)]
        );
    }

    #[test]
    fn content_type_skips_disabled_lines() {
        assert_eq!(
            content_type("# Content-Type: a/b\nAccept: */*\ncontent-type : application/json"),
            Some("application/json".into())
        );
        assert_eq!(content_type("Accept: */*"), None);
    }
}
