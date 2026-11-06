//! リクエストから curl コマンドを生成する。
//!
//! 出力は bash / zsh / sh (POSIX 系シェル) 向け。値はすべて `'…'` で囲み、
//! 中の `'` は `'\''` に置き換える。行の継続は `\`。

use crate::model::{HttpMethod, Request};

/// POSIX シェルで 1 つの引数として解釈されるようにクォートする。
pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
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

pub fn to_curl(req: &Request) -> String {
    let mut lines = vec!["curl".to_string()];
    for group in arg_groups(req) {
        let rendered: Vec<String> = group
            .iter()
            .map(|a| {
                if is_plain_option(a) {
                    a.clone()
                } else {
                    quote(a)
                }
            })
            .collect();
        lines.push(rendered.join(" "));
    }
    lines.join(" \\\n  ")
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
        assert_eq!(to_curl(&req), "curl \\\n  'https://example.com/'");
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
        assert_eq!(
            args(&post("{}")),
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
    fn output_quotes_values() {
        let curl = to_curl(&post(r#"{"s":"it's","p":"C:\\d"}"#));
        assert_eq!(
            curl,
            "curl \\\n  -X POST \\\n  'https://example.com/api' \\\n  -H 'Content-Type: application/json' \\\n  --data-raw '{\"s\":\"it'\\''s\",\"p\":\"C:\\\\d\"}'"
        );
    }

    #[test]
    fn quote_escapes_single_quote_only() {
        assert_eq!(quote("it's"), r"'it'\''s'");
        assert_eq!(quote(r"$HOME \ `x`"), r"'$HOME \ `x`'");
    }
}
