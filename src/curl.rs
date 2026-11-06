//! リクエストから curl コマンドを生成する。
//!
//! シェルごとにクォートの規則が違うため、出力先のシェルを [`Shell`] で選ぶ。
//!
//! | シェル | クォート | 行継続 |
//! | --- | --- | --- |
//! | bash / zsh / sh | `'…'` 内はすべて文字どおり。`'` は `'\''` | `\` |
//! | fish | `'…'` 内でも `\\` と `\'` はエスケープになる | `\` |
//! | PowerShell | `'…'` 内で `'` (と ‘ ’ ‚ ‛) は 2 つ重ねる | `` ` `` |

use crate::model::{HttpMethod, Request};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shell {
    /// bash / zsh / sh (dash) などの POSIX 系シェル
    #[default]
    Posix,
    Fish,
    /// PowerShell 7.3 以降 (Windows の `curl.exe` を呼び出す)
    PowerShell,
}

impl Shell {
    pub const ALL: [Shell; 3] = [Shell::Posix, Shell::Fish, Shell::PowerShell];

    pub fn label(self) -> &'static str {
        match self {
            Shell::Posix => "bash / zsh / sh",
            Shell::Fish => "fish",
            Shell::PowerShell => "PowerShell 7.3+",
        }
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|s| *s == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    /// 環境変数 `SHELL` と OS から既定のシェルを推測する。
    pub fn detect() -> Self {
        Self::from_env(std::env::var("SHELL").ok().as_deref(), cfg!(windows))
    }

    fn from_env(shell_var: Option<&str>, windows: bool) -> Self {
        match shell_var {
            Some(s)
                if s.rsplit(['/', '\\'])
                    .next()
                    .is_some_and(|n| n.starts_with("fish")) =>
            {
                Shell::Fish
            }
            Some(s) if s.to_ascii_lowercase().contains("pwsh") => Shell::PowerShell,
            Some(_) => Shell::Posix,
            None if windows => Shell::PowerShell,
            None => Shell::Posix,
        }
    }

    fn program(self) -> &'static str {
        match self {
            // Windows PowerShell 5.1 では `curl` が Invoke-WebRequest のエイリアスなので .exe を明示する
            Shell::PowerShell => "curl.exe",
            _ => "curl",
        }
    }

    fn continuation(self) -> &'static str {
        match self {
            Shell::PowerShell => " `\n  ",
            _ => " \\\n  ",
        }
    }

    /// 1 つの引数として解釈されるようにクォートする。
    pub fn quote(self, s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('\'');
        for c in s.chars() {
            match (self, c) {
                (Shell::Posix, '\'') => out.push_str(r"'\''"),
                (Shell::Fish, '\'' | '\\') => {
                    out.push('\\');
                    out.push(c);
                }
                // PowerShell は全角のシングルクォートも引用符として扱う
                (Shell::PowerShell, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') => {
                    out.push(c);
                    out.push(c);
                }
                _ => out.push(c),
            }
        }
        out.push('\'');
        out
    }
}

/// curl に渡すべき引数を、行ごとのグループにして返す (クォート前の生の値)。
pub fn arg_groups(req: &Request) -> Vec<Vec<String>> {
    let url = req
        .full_url()
        .map(|u| u.to_string())
        .unwrap_or_else(|_| req.url.clone());
    let has_body = !req.body.is_empty();
    let mut groups = Vec::new();
    match req.method {
        // -X HEAD だと curl がボディを待ち続けるので --head を使う
        HttpMethod::Head => groups.push(vec!["--head".to_string()]),
        // ボディがあると curl は POST にするので、GET でも明示する
        HttpMethod::Get if !has_body => {}
        m => groups.push(vec!["-X".to_string(), m.as_str().to_string()]),
    }
    groups.push(vec![url]);
    for h in req
        .headers
        .iter()
        .filter(|h| h.enabled && !h.key.is_empty())
    {
        groups.push(vec!["-H".to_string(), format!("{}: {}", h.key, h.value)]);
    }
    if has_body {
        // --data-raw は先頭の @ をファイル指定として解釈しない
        groups.push(vec!["--data-raw".to_string(), req.body.clone()]);
    }
    groups
}

/// curl 本体を除いた引数列 (シェルが解釈した後に curl が受け取るべきもの)。
pub fn args(req: &Request) -> Vec<String> {
    arg_groups(req).into_iter().flatten().collect()
}

pub fn to_curl(req: &Request, shell: Shell) -> String {
    let mut lines = vec![shell.program().to_string()];
    for group in arg_groups(req) {
        let rendered: Vec<String> = group
            .iter()
            .map(|a| {
                if is_plain_option(a) {
                    a.clone()
                } else {
                    shell.quote(a)
                }
            })
            .collect();
        lines.push(rendered.join(" "));
    }
    lines.join(shell.continuation())
}

/// クォート不要な引数 (`-X` や `POST` など)。
fn is_plain_option(a: &str) -> bool {
    !a.is_empty()
        && a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && (a.starts_with('-') || a.chars().all(|c| c.is_ascii_uppercase()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::KeyValue;

    fn post(body: &str) -> Request {
        Request {
            method: HttpMethod::Post,
            url: "https://example.com/api".into(),
            headers: vec![
                KeyValue::new("Content-Type", "application/json"),
                KeyValue::disabled("X-Off", "1"),
            ],
            body: body.into(),
            ..Default::default()
        }
    }

    #[test]
    fn get_is_minimal() {
        let req = Request {
            url: "https://example.com".into(),
            ..Default::default()
        };
        assert_eq!(
            to_curl(&req, Shell::Posix),
            "curl \\\n  'https://example.com/'"
        );
        assert_eq!(args(&req), vec!["https://example.com/"]);
    }

    #[test]
    fn get_with_body_keeps_explicit_method() {
        let req = Request {
            body: "x".into(),
            ..Default::default()
        };
        assert_eq!(&args(&req)[..2], ["-X", "GET"]);
    }

    #[test]
    fn head_uses_head_flag() {
        let req = Request {
            method: HttpMethod::Head,
            ..Default::default()
        };
        let a = args(&req);
        assert_eq!(a[0], "--head");
        assert!(!a.contains(&"-X".to_string()));
    }

    #[test]
    fn args_skip_disabled_headers() {
        let a = args(&post("{}"));
        assert_eq!(
            a,
            vec![
                "-X",
                "POST",
                "https://example.com/api",
                "-H",
                "Content-Type: application/json",
                "--data-raw",
                "{}"
            ]
        );
    }

    #[test]
    fn posix_output() {
        let curl = to_curl(&post(r#"{"s":"it's","p":"C:\\d"}"#), Shell::Posix);
        assert_eq!(
            curl,
            "curl \\\n  -X POST \\\n  'https://example.com/api' \\\n  -H 'Content-Type: application/json' \\\n  --data-raw '{\"s\":\"it'\\''s\",\"p\":\"C:\\\\d\"}'"
        );
    }

    #[test]
    fn fish_escapes_backslash_and_quote() {
        assert_eq!(Shell::Fish.quote(r"a\b'c"), r"'a\\b\'c'");
        assert_eq!(Shell::Fish.quote("$HOME"), "'$HOME'");
        assert!(to_curl(&post("{}"), Shell::Fish).starts_with("curl \\\n  -X POST"));
    }

    #[test]
    fn powershell_doubles_quotes_and_uses_backtick() {
        assert_eq!(Shell::PowerShell.quote("it's"), "'it''s'");
        assert_eq!(Shell::PowerShell.quote("‘x’"), "'‘‘x’’'");
        assert_eq!(Shell::PowerShell.quote(r"C:\d $env:X"), r"'C:\d $env:X'");
        let curl = to_curl(&post("{}"), Shell::PowerShell);
        assert!(curl.starts_with("curl.exe `\n  -X POST `\n  'https://example.com/api'"));
        assert!(curl.ends_with("--data-raw '{}'"));
    }

    #[test]
    fn detect_from_env() {
        assert_eq!(Shell::from_env(Some("/usr/bin/fish"), false), Shell::Fish);
        assert_eq!(Shell::from_env(Some("/bin/zsh"), false), Shell::Posix);
        assert_eq!(
            Shell::from_env(Some("/usr/local/bin/pwsh"), false),
            Shell::PowerShell
        );
        assert_eq!(Shell::from_env(None, true), Shell::PowerShell);
        assert_eq!(Shell::from_env(None, false), Shell::Posix);
    }

    #[test]
    fn shell_cycles() {
        assert_eq!(Shell::Posix.next(), Shell::Fish);
        assert_eq!(Shell::PowerShell.next(), Shell::Posix);
    }
}
