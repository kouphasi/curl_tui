//! 生成した curl コマンドを実際のシェルに解釈させ、curl が受け取る引数が
//! 意図どおりになるかを確かめる。
//!
//! 本物の curl の代わりに、受け取った引数を NUL 区切りで出力するだけの
//! 偽 `curl` を PATH の先頭に置いて実行する。インストールされていないシェルはスキップする。

use std::{os::unix::fs::PermissionsExt, path::Path, process::Command};

use curl_tui::{
    curl,
    model::{HttpMethod, KeyValue, Request},
};

/// 各シェルで壊れやすい文字をひととおり含むリクエスト。
fn tricky_requests() -> Vec<Request> {
    vec![
        Request {
            method: HttpMethod::Post,
            url: "https://example.com/a b?x=1&y=$HOME".into(),
            params: vec![KeyValue::new("q", "it's & 日本語")],
            headers: vec![
                KeyValue::new("Content-Type", "application/json"),
                KeyValue::new("X-Weird", r#"a"b'c\d `e` $(f) !g %PATH%"#),
            ],
            body: concat!(
                r#"{"path":"C:\\Users\\me","quote":"it's","dq":"say \"hi\"","#,
                "\n",
                r#""vars":"$HOME ${X} `cmd` $(cmd) %PATH% !!","glob":"*?[a]~","semi":"a;b|c&d>e<f","#,
                "\n",
                r#""esc":"\\' \n \t","jp":"日本語 ‘曲がった’ “引用”","end":"\"}"#,
                "\\"
            )
            .into(),
            ..Default::default()
        },
        Request {
            method: HttpMethod::Head,
            url: "localhost:8080".into(),
            ..Default::default()
        },
        Request {
            url: "http://x".into(),
            body: "'".into(),
            ..Default::default()
        },
    ]
}

fn installed(shell: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {shell}"))
        .output()
        .is_ok_and(|o| o.status.success())
}

fn write_fake_curl(dir: &Path) {
    let path = dir.join("curl");
    std::fs::write(
        &path,
        "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\0' \"$a\"; done\n",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn run_in_shell(shell_bin: &str, command: &str, bin_dir: &Path) -> Vec<String> {
    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = Command::new(shell_bin)
        .arg("-c")
        .arg(command)
        .env("PATH", path)
        .env("HOME", "/should/not/expand")
        .env("X", "should-not-expand")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{shell_bin} failed: {}\n{command}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    stdout
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

fn check(shell_bins: &[&str]) {
    let dir = tempfile::tempdir().unwrap();
    write_fake_curl(dir.path());
    let mut ran = false;
    for bin in shell_bins {
        if !installed(bin) {
            eprintln!("skip: {bin} がインストールされていません");
            continue;
        }
        ran = true;
        for req in tricky_requests() {
            let command = curl::to_curl(&req);
            let got = run_in_shell(bin, &command, dir.path());
            assert_eq!(got, curl::args(&req), "{bin} で引数が変わった:\n{command}");
        }
    }
    if !ran {
        eprintln!("skip: 対象のシェルが 1 つも見つかりません");
    }
}

#[test]
fn posix_shells_receive_exact_arguments() {
    check(&["bash", "sh", "dash", "zsh"]);
}
