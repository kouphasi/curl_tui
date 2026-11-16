//! 表示言語 (英語 / 日本語) の切り替え。
//!
//! 言語はプロセス全体で 1 つ。設定ファイルの `"lang"` を読んだ時点で [`set`] し、
//! 文言を出す側は [`t!`](crate::t) / [`tf!`](crate::tf) で両方の言語を並べて書く。

use std::sync::atomic::{AtomicU8, Ordering};

/// 表示言語。既定は英語。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    En,
    Ja,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Ja];

    /// 設定ファイルに書く名前。
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ja => "ja",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|l| l.code().eq_ignore_ascii_case(code.trim()))
    }

    /// この言語の文言を選ぶ。
    pub fn pick<'a>(self, en: &'a str, ja: &'a str) -> &'a str {
        match self {
            Lang::En => en,
            Lang::Ja => ja,
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// 現在の表示言語。
pub fn current() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Lang::Ja,
        _ => Lang::En,
    }
}

/// 表示言語を切り替える。
pub fn set(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

/// 現在の言語の文言を選ぶ: `t!("Saved", "保存しました")`。
#[macro_export]
macro_rules! t {
    ($en:expr, $ja:expr $(,)?) => {
        $crate::i18n::current().pick($en, $ja)
    };
}

/// [`t!`](crate::t) の `format!` 版: `tf!("Saved: {p}", "保存しました: {p}")`。
/// 引数は両方の言語で同じものを使う。
#[macro_export]
macro_rules! tf {
    ($en:literal, $ja:literal $(, $($args:tt)*)?) => {
        match $crate::i18n::current() {
            $crate::i18n::Lang::En => format!($en $(, $($args)*)?),
            $crate::i18n::Lang::Ja => format!($ja $(, $($args)*)?),
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        for lang in Lang::ALL {
            assert_eq!(Lang::from_code(lang.code()), Some(lang));
        }
        assert_eq!(Lang::from_code(" JA "), Some(Lang::Ja));
        assert_eq!(Lang::from_code("fr"), None);
        assert_eq!(Lang::default(), Lang::En);
    }

    #[test]
    fn pick_selects_language() {
        assert_eq!(Lang::En.pick("a", "あ"), "a");
        assert_eq!(Lang::Ja.pick("a", "あ"), "あ");
    }
}
