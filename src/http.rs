//! HTTP 送信処理 (reqwest + tokio)。

use std::time::{Duration, Instant};

use anyhow::Context;

use crate::model::{Request, Response};

/// TUI で使う HTTP クライアントを作成する。
pub fn build_client(timeout: Duration) -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(concat!("curl_tui/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("HTTP クライアントの作成に失敗しました")
}

/// リクエストを送信し、レスポンスを返す。
pub async fn send(client: &reqwest::Client, req: &Request) -> anyhow::Result<Response> {
    let url = req.full_url()?;
    let method = reqwest::Method::from_bytes(req.method.as_str().as_bytes())?;
    let mut builder = client.request(method, url);
    for h in req
        .headers
        .iter()
        .filter(|h| h.enabled && !h.key.is_empty())
    {
        builder = builder.header(h.key.as_str(), h.value.as_str());
    }
    if !req.body.is_empty() {
        builder = builder.body(req.body.clone());
    }

    let start = Instant::now();
    let resp = builder.send().await.map_err(describe_error)?;
    let status = resp.status();
    let headers: Vec<(String, String)> = resp
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.to_string(),
                String::from_utf8_lossy(v.as_bytes()).into_owned(),
            )
        })
        .collect();
    let bytes = resp.bytes().await.map_err(describe_error)?;
    let elapsed_ms = start.elapsed().as_millis();

    let content_type = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.as_str());
    let body = format_body(content_type, &String::from_utf8_lossy(&bytes));

    Ok(Response {
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or("").to_string(),
        elapsed_ms,
        headers,
        body,
        size: bytes.len(),
    })
}

fn describe_error(e: reqwest::Error) -> anyhow::Error {
    let kind = if e.is_timeout() {
        "タイムアウト"
    } else if e.is_connect() {
        "接続エラー"
    } else if e.is_builder() {
        "リクエスト構築エラー"
    } else {
        "通信エラー"
    };
    // reqwest のエラーは source チェーンに詳細があるので連結する
    let mut msg = e.to_string();
    let mut src = std::error::Error::source(&e);
    while let Some(s) = src {
        msg.push_str(": ");
        msg.push_str(&s.to_string());
        src = s.source();
    }
    anyhow::anyhow!("{kind}: {msg}")
}

/// JSON と判断できればインデント整形し、それ以外はそのまま返す。
pub fn format_body(content_type: Option<&str>, body: &str) -> String {
    let looks_json = content_type.is_some_and(|c| c.to_ascii_lowercase().contains("json"))
        || matches!(body.trim_start().chars().next(), Some('{' | '['));
    if looks_json
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(body)
        && let Ok(pretty) = serde_json::to_string_pretty(&v)
    {
        return pretty;
    }
    body.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_body_pretty_prints_json() {
        let out = format_body(
            Some("application/json; charset=utf-8"),
            r#"{"a":1,"b":[1,2]}"#,
        );
        assert_eq!(out, "{\n  \"a\": 1,\n  \"b\": [\n    1,\n    2\n  ]\n}");
    }

    #[test]
    fn format_body_detects_json_without_content_type() {
        assert_eq!(format_body(None, "[1]"), "[\n  1\n]");
    }

    #[test]
    fn format_body_keeps_invalid_json_and_plain_text() {
        assert_eq!(format_body(Some("application/json"), "{oops"), "{oops");
        assert_eq!(format_body(Some("text/html"), "<p>hi</p>"), "<p>hi</p>");
    }
}
