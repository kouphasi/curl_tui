//! リクエストコレクションの保存 / 読み込み (JSON)。

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::model::Request;

/// 既定の保存先: `$XDG_CONFIG_HOME/curl_tui/collection.json` など。
pub fn default_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("curl_tui")
        .join("collection.json")
}

/// ファイルが無ければデフォルトのリクエスト 1 件を返す。
pub fn load(path: &Path) -> anyhow::Result<Vec<Request>> {
    if !path.exists() {
        return Ok(vec![Request::default()]);
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("{} を読み込めません", path.display()))?;
    let requests: Vec<Request> = serde_json::from_str(&text)
        .with_context(|| format!("{} の JSON が不正です", path.display()))?;
    if requests.is_empty() {
        Ok(vec![Request::default()])
    } else {
        Ok(requests)
    }
}

pub fn save(path: &Path, requests: &[Request]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("{} を作成できません", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(requests)?;
    std::fs::write(path, json).with_context(|| format!("{} に書き込めません", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{HttpMethod, KeyValue};

    #[test]
    fn missing_file_yields_default() {
        let dir = tempfile::tempdir().unwrap();
        let reqs = load(&dir.path().join("none.json")).unwrap();
        assert_eq!(reqs, vec![Request::default()]);
    }

    #[test]
    fn save_and_load_round_trip_creates_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/dir/c.json");
        let reqs = vec![
            Request::default(),
            Request {
                name: "Create".into(),
                method: HttpMethod::Post,
                url: "http://localhost/items".into(),
                params: vec![KeyValue::new("a", "1")],
                headers: vec![KeyValue::disabled("X", "y")],
                body: "{}".into(),
            },
        ];
        save(&path, &reqs).unwrap();
        assert_eq!(load(&path).unwrap(), reqs);
    }

    #[test]
    fn empty_array_yields_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.json");
        std::fs::write(&path, "[]").unwrap();
        assert_eq!(load(&path).unwrap().len(), 1);
    }

    #[test]
    fn invalid_json_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.json");
        std::fs::write(&path, "not json").unwrap();
        assert!(load(&path).is_err());
    }
}
