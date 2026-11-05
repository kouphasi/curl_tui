//! ローカルに立てた最小 HTTP サーバに対して実際に送信するテスト。

use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use curl_tui::{
    app::{Action, App, ResponseState},
    http,
    model::{HttpMethod, KeyValue, Request},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

/// 受け取った生リクエストを JSON で返すエコーサーバ。`/slow` は応答を遅らせ、`/text` は text/plain を返す。
async fn spawn_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let raw = read_request(&mut sock).await;
                let first = raw.lines().next().unwrap_or("").to_string();
                let path = first.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (status, ctype, body) = if path.starts_with("/slow") {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    ("200 OK", "text/plain", "late".to_string())
                } else if path.starts_with("/text") {
                    ("200 OK", "text/plain", "hello".to_string())
                } else if path.starts_with("/missing") {
                    (
                        "404 Not Found",
                        "application/json",
                        r#"{"error":"nope"}"#.to_string(),
                    )
                } else {
                    let (head, req_body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
                    let headers: Vec<String> = head
                        .lines()
                        .skip(1)
                        .map(|l| l.to_ascii_lowercase())
                        .collect();
                    let echo = serde_json::json!({
                        "request_line": first,
                        "headers": headers,
                        "body": req_body,
                    });
                    ("200 OK", "application/json", echo.to_string())
                };
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nX-Test: yes\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    format!("http://{addr}")
}

/// ヘッダと Content-Length 分のボディを読み込む。
async fn read_request(sock: &mut tokio::net::TcpStream) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = sock.read(&mut chunk).await.unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        let text = String::from_utf8_lossy(&buf);
        if let Some(pos) = text.find("\r\n\r\n") {
            let len = text[..pos]
                .lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case("content-length")
                        .then(|| v.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            if buf.len() >= pos + 4 + len {
                break;
            }
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn client() -> reqwest::Client {
    http::build_client(Duration::from_secs(10)).unwrap()
}

#[tokio::test]
async fn get_with_params_and_headers() {
    let base = spawn_server().await;
    let req = Request {
        url: format!("{base}/echo"),
        params: vec![KeyValue::new("q", "a b"), KeyValue::disabled("off", "1")],
        headers: vec![
            KeyValue::new("X-Token", "secret"),
            KeyValue::disabled("X-Off", "1"),
        ],
        ..Default::default()
    };
    let resp = http::send(&client(), &req).await.unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.status_text, "OK");
    assert!(resp.is_success());
    assert!(
        resp.headers
            .iter()
            .any(|(k, v)| k == "x-test" && v == "yes")
    );

    let echo: serde_json::Value = serde_json::from_str(&resp.body).unwrap();
    assert_eq!(echo["request_line"], "GET /echo?q=a+b HTTP/1.1");
    let headers = echo["headers"].as_array().unwrap();
    assert!(headers.iter().any(|h| h == "x-token: secret"));
    assert!(
        !headers
            .iter()
            .any(|h| h.as_str().unwrap().starts_with("x-off"))
    );
    // JSON は整形されている
    assert!(resp.body.contains("\n  \"body\""));
}

#[tokio::test]
async fn post_sends_body() {
    let base = spawn_server().await;
    let req = Request {
        method: HttpMethod::Post,
        url: format!("{base}/items"),
        headers: vec![KeyValue::new("Content-Type", "application/json")],
        body: r#"{"name":"日本語"}"#.into(),
        ..Default::default()
    };
    let resp = http::send(&client(), &req).await.unwrap();
    let echo: serde_json::Value = serde_json::from_str(&resp.body).unwrap();
    assert_eq!(echo["request_line"], "POST /items HTTP/1.1");
    assert_eq!(echo["body"], r#"{"name":"日本語"}"#);
}

#[tokio::test]
async fn every_method_is_sent() {
    let base = spawn_server().await;
    for method in [
        HttpMethod::Put,
        HttpMethod::Patch,
        HttpMethod::Delete,
        HttpMethod::Options,
    ] {
        let req = Request {
            method,
            url: format!("{base}/m"),
            ..Default::default()
        };
        let resp = http::send(&client(), &req).await.unwrap();
        assert!(
            resp.body.contains(&format!("{method} /m HTTP/1.1")),
            "{method}: {}",
            resp.body
        );
    }
    let head = Request {
        method: HttpMethod::Head,
        url: format!("{base}/m"),
        ..Default::default()
    };
    let resp = http::send(&client(), &head).await.unwrap();
    assert_eq!(resp.status, 200);
    assert!(resp.body.is_empty());
}

#[tokio::test]
async fn non_json_and_error_status() {
    let base = spawn_server().await;
    let text = http::send(
        &client(),
        &Request {
            url: format!("{base}/text"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(text.body, "hello");
    assert_eq!(text.size, 5);

    let missing = http::send(
        &client(),
        &Request {
            url: format!("{base}/missing"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(missing.status, 404);
    assert!(!missing.is_success());
    assert!(missing.body.contains("\"error\": \"nope\""));
}

#[tokio::test]
async fn connection_refused_is_reported() {
    // 一度 bind して即座に閉じたポートは (ほぼ確実に) 誰も listen していない
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap().port()
    };
    let req = Request {
        url: format!("http://127.0.0.1:{port}/"),
        ..Default::default()
    };
    let err = http::send(&client(), &req).await.unwrap_err();
    assert!(err.to_string().contains("接続エラー"), "{err}");
}

#[tokio::test]
async fn timeout_is_reported() {
    let base = spawn_server().await;
    let client = http::build_client(Duration::from_millis(200)).unwrap();
    let err = http::send(
        &client,
        &Request {
            url: format!("{base}/slow"),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("タイムアウト"), "{err}");
}

/// キー入力 → Action::Send → 実送信 → set_response までの一連の流れ。
#[tokio::test]
async fn end_to_end_app_flow() {
    let base = spawn_server().await;
    let mut app = App::new(vec![Request {
        url: format!("{base}/flow"),
        ..Default::default()
    }]);
    let action = app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    let Action::Send(req) = action else {
        panic!("expected Send")
    };
    assert!(app.is_loading());

    let res = http::send(&client(), &req).await.map_err(|e| e.to_string());
    app.set_response(res);
    let ResponseState::Done(resp) = &app.response else {
        panic!("expected Done")
    };
    assert_eq!(resp.status, 200);
    assert!(resp.body.contains("GET /flow"));
}
