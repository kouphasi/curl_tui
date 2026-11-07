//! HTTP リクエスト / レスポンスのデータモデル。

use serde::{Deserialize, Serialize};

/// サポートする HTTP メソッド。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl HttpMethod {
    pub const ALL: [HttpMethod; 7] = [
        HttpMethod::Get,
        HttpMethod::Post,
        HttpMethod::Put,
        HttpMethod::Patch,
        HttpMethod::Delete,
        HttpMethod::Head,
        HttpMethod::Options,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Head => "HEAD",
            HttpMethod::Options => "OPTIONS",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|m| *m == self).unwrap_or(0)
    }

    /// 次のメソッド (末尾の次は先頭)。
    pub fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    /// 前のメソッド (先頭の前は末尾)。
    pub fn prev(self) -> Self {
        let len = Self::ALL.len();
        Self::ALL[(self.index() + len - 1) % len]
    }
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// ヘッダやクエリパラメータの 1 行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyValue {
    pub key: String,
    pub value: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl KeyValue {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            enabled: true,
        }
    }

    pub fn disabled(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            enabled: false,
            ..Self::new(key, value)
        }
    }
}

/// 保存・送信の単位となるリクエスト。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub name: String,
    #[serde(default)]
    pub method: HttpMethod,
    pub url: String,
    #[serde(default)]
    pub params: Vec<KeyValue>,
    #[serde(default)]
    pub headers: Vec<KeyValue>,
    #[serde(default)]
    pub body: String,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            name: "New Request".to_string(),
            method: HttpMethod::Get,
            url: "https://httpbin.org/get".to_string(),
            params: Vec::new(),
            headers: Vec::new(),
            body: String::new(),
        }
    }
}

impl Request {
    /// スキームを補完し、有効なクエリパラメータを付与した URL を返す。
    pub fn full_url(&self) -> anyhow::Result<reqwest::Url> {
        let raw = self.url.trim();
        if raw.is_empty() {
            anyhow::bail!("URL が空です");
        }
        let with_scheme = if raw.contains("://") {
            raw.to_string()
        } else {
            format!("http://{raw}")
        };
        let mut url = reqwest::Url::parse(&with_scheme)
            .map_err(|e| anyhow::anyhow!("URL が不正です ({e}): {raw}"))?;
        let enabled: Vec<_> = self
            .params
            .iter()
            .filter(|p| p.enabled && !p.key.is_empty())
            .collect();
        if !enabled.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for p in enabled {
                pairs.append_pair(&p.key, &p.value);
            }
        }
        Ok(url)
    }
}

/// `Key: Value` 形式のテキストをヘッダ一覧に変換する。`#` 始まりの行は無効扱い。
pub fn parse_headers(text: &str) -> Vec<KeyValue> {
    parse_lines(text, ':')
}

/// `key=value` 形式のテキストをパラメータ一覧に変換する。`#` 始まりの行は無効扱い。
pub fn parse_params(text: &str) -> Vec<KeyValue> {
    parse_lines(text, '=')
}

fn parse_lines(text: &str, sep: char) -> Vec<KeyValue> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let (enabled, line) = match line.strip_prefix('#') {
                Some(rest) => (false, rest.trim_start()),
                None => (true, line),
            };
            let (key, value) = match line.split_once(sep) {
                Some((k, v)) => (k.trim(), v.trim()),
                None => (line, ""),
            };
            if key.is_empty() {
                return None;
            }
            Some(KeyValue {
                key: key.to_string(),
                value: value.to_string(),
                enabled,
            })
        })
        .collect()
}

pub fn headers_to_text(headers: &[KeyValue]) -> String {
    to_lines(headers, ": ")
}

pub fn params_to_text(params: &[KeyValue]) -> String {
    to_lines(params, "=")
}

fn to_lines(items: &[KeyValue], sep: &str) -> String {
    items
        .iter()
        .map(|kv| {
            let prefix = if kv.enabled { "" } else { "# " };
            format!("{prefix}{}{sep}{}", kv.key, kv.value)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 受信したレスポンス。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub status_text: String,
    pub elapsed_ms: u128,
    pub headers: Vec<(String, String)>,
    /// 表示用に整形済みのボディ (JSON なら pretty print 済み)。
    pub body: String,
    /// 受信したボディのバイト数。
    pub size: usize,
}

impl Response {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// バイト数を人間に読みやすい表記にする。
pub fn format_size(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{:.1} MB", b / KB / KB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_cycles_forward_and_backward() {
        assert_eq!(HttpMethod::Get.next(), HttpMethod::Post);
        assert_eq!(HttpMethod::Options.next(), HttpMethod::Get);
        assert_eq!(HttpMethod::Get.prev(), HttpMethod::Options);
        let mut m = HttpMethod::Get;
        for _ in 0..HttpMethod::ALL.len() {
            m = m.next();
        }
        assert_eq!(m, HttpMethod::Get);
    }

    #[test]
    fn method_serializes_uppercase() {
        assert_eq!(
            serde_json::to_string(&HttpMethod::Patch).unwrap(),
            "\"PATCH\""
        );
        let m: HttpMethod = serde_json::from_str("\"DELETE\"").unwrap();
        assert_eq!(m, HttpMethod::Delete);
    }

    #[test]
    fn parse_headers_handles_disabled_blank_and_colons() {
        let text = "Content-Type: application/json\n\n# X-Debug: 1\nX-Time: 12:30:00\nNoValue";
        let h = parse_headers(text);
        assert_eq!(
            h,
            vec![
                KeyValue::new("Content-Type", "application/json"),
                KeyValue::disabled("X-Debug", "1"),
                KeyValue::new("X-Time", "12:30:00"),
                KeyValue::new("NoValue", ""),
            ]
        );
    }

    #[test]
    fn parse_params_ignores_empty_keys() {
        let p = parse_params("a=1\n=oops\n  b = two words \n#c=3");
        assert_eq!(
            p,
            vec![
                KeyValue::new("a", "1"),
                KeyValue::new("b", "two words"),
                KeyValue::disabled("c", "3"),
            ]
        );
    }

    #[test]
    fn headers_text_round_trip() {
        let h = vec![KeyValue::new("A", "1"), KeyValue::disabled("B", "2")];
        let text = headers_to_text(&h);
        assert_eq!(text, "A: 1\n# B: 2");
        assert_eq!(parse_headers(&text), h);
    }

    #[test]
    fn params_text_round_trip() {
        let p = vec![KeyValue::new("q", "rust"), KeyValue::disabled("page", "2")];
        let text = params_to_text(&p);
        assert_eq!(text, "q=rust\n# page=2");
        assert_eq!(parse_params(&text), p);
    }

    #[test]
    fn full_url_adds_scheme_and_encoded_params() {
        let req = Request {
            url: "example.com/search?x=1".into(),
            params: vec![
                KeyValue::new("q", "hello world"),
                KeyValue::new("lang", "日本"),
                KeyValue::disabled("skip", "me"),
            ],
            ..Default::default()
        };
        let url = req.full_url().unwrap();
        assert_eq!(
            url.as_str(),
            "http://example.com/search?x=1&q=hello+world&lang=%E6%97%A5%E6%9C%AC"
        );
    }

    #[test]
    fn full_url_rejects_empty_and_invalid() {
        let empty = Request {
            url: "   ".into(),
            ..Default::default()
        };
        assert!(empty.full_url().is_err());
        let bad = Request {
            url: "http://exa mple.com".into(),
            ..Default::default()
        };
        assert!(bad.full_url().is_err());
    }

    #[test]
    fn request_deserializes_with_defaults() {
        let req: Request = serde_json::from_str(r#"{"name":"n","url":"http://a"}"#).unwrap();
        assert_eq!(req.method, HttpMethod::Get);
        assert!(req.headers.is_empty());
        assert!(req.body.is_empty());
    }

    #[test]
    fn format_size_units() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(3 * 1024 * 1024), "3.0 MB");
    }

    #[test]
    fn response_success_range() {
        let mut r = Response {
            status: 204,
            status_text: String::new(),
            elapsed_ms: 0,
            headers: vec![],
            body: String::new(),
            size: 0,
        };
        assert!(r.is_success());
        r.status = 404;
        assert!(!r.is_success());
    }
}
