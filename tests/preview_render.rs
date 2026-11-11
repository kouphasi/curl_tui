//! 実際のヘッドレス Chromium で HTML を描画する結合テスト。
//! 環境によってブラウザの挙動が違うため、`CURL_TUI_BROWSER` でブラウザを
//! 指定したときだけ実行する (例: CURL_TUI_BROWSER=$(which chromium) cargo test)。

use curl_tui::preview::{self, Job};

#[tokio::test]
async fn renders_html_to_png() {
    if std::env::var_os("CURL_TUI_BROWSER").is_none() {
        eprintln!("CURL_TUI_BROWSER が未設定なのでスキップ");
        return;
    }
    let browser = preview::find_browser().unwrap();
    let job = Job {
        html: r#"<!doctype html><html><head><style>
            body { margin: 0; background: #fff; }
            div { height: 300px; background: #c00; }
        </style></head><body><div></div></body></html>"#
            .into(),
        base_url: "http://example.invalid/".into(),
        width: 400,
        height: 1200,
    };
    let img = preview::render(&browser, &job).await.expect("render");
    assert_eq!(&img.png[1..4], b"PNG");
    assert_eq!(img.width, 400);
    // 下端の白い余白は切り詰められ、赤い 300px の部分だけが残る
    assert_eq!(img.height, 300);
}
