//! シンプルなテキストエディタ (単一行 / 複数行)。
//!
//! カーソル位置は「文字 (char) 単位」で保持し、表示幅は `unicode-width` で計算する。

use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEditor {
    lines: Vec<String>,
    row: usize,
    col: usize,
    multiline: bool,
}

impl TextEditor {
    pub fn single_line(text: &str) -> Self {
        let mut e = Self {
            lines: vec![String::new()],
            row: 0,
            col: 0,
            multiline: false,
        };
        e.set_text(text);
        e
    }

    pub fn multi_line(text: &str) -> Self {
        let mut e = Self {
            lines: vec![String::new()],
            row: 0,
            col: 0,
            multiline: true,
        };
        e.set_text(text);
        e
    }

    pub fn is_multiline(&self) -> bool {
        self.multiline
    }

    /// テキスト全体を置き換え、カーソルを末尾に移動する。
    pub fn set_text(&mut self, text: &str) {
        self.lines = if self.multiline {
            text.split('\n')
                .map(|l| l.trim_end_matches('\r').to_string())
                .collect()
        } else {
            vec![text.replace(['\n', '\r'], "")]
        };
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.row = self.lines.len() - 1;
        self.col = char_len(&self.lines[self.row]);
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }

    /// (行, 文字単位の列)
    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    /// カーソルの表示上の列 (全角文字は 2 カラム)。
    pub fn cursor_display_col(&self) -> usize {
        let line = &self.lines[self.row];
        line[..byte_index(line, self.col)].width()
    }

    pub fn insert_char(&mut self, c: char) {
        if c == '\n' {
            self.insert_newline();
            return;
        }
        if c == '\r' {
            return;
        }
        let line = &mut self.lines[self.row];
        let idx = byte_index(line, self.col);
        line.insert(idx, c);
        self.col += 1;
    }

    pub fn insert_str(&mut self, s: &str) {
        for c in s.chars() {
            if c == '\n' && !self.multiline {
                continue;
            }
            self.insert_char(c);
        }
    }

    pub fn insert_newline(&mut self) {
        if !self.multiline {
            return;
        }
        let line = &mut self.lines[self.row];
        let idx = byte_index(line, self.col);
        let rest = line.split_off(idx);
        self.row += 1;
        self.lines.insert(self.row, rest);
        self.col = 0;
    }

    pub fn backspace(&mut self) {
        if self.col > 0 {
            let line = &mut self.lines[self.row];
            let idx = byte_index(line, self.col - 1);
            line.remove(idx);
            self.col -= 1;
        } else if self.row > 0 {
            let current = self.lines.remove(self.row);
            self.row -= 1;
            self.col = char_len(&self.lines[self.row]);
            self.lines[self.row].push_str(&current);
        }
    }

    pub fn delete(&mut self) {
        let len = char_len(&self.lines[self.row]);
        if self.col < len {
            let line = &mut self.lines[self.row];
            let idx = byte_index(line, self.col);
            line.remove(idx);
        } else if self.row + 1 < self.lines.len() {
            let next = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&next);
        }
    }

    pub fn move_left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = char_len(&self.lines[self.row]);
        }
    }

    pub fn move_right(&mut self) {
        if self.col < char_len(&self.lines[self.row]) {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    pub fn move_up(&mut self) {
        if self.row > 0 {
            self.row -= 1;
            self.clamp_col();
        }
    }

    pub fn move_down(&mut self) {
        if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.clamp_col();
        }
    }

    pub fn move_home(&mut self) {
        self.col = 0;
    }

    pub fn move_end(&mut self) {
        self.col = char_len(&self.lines[self.row]);
    }

    /// カーソル位置から行末までを削除 (Ctrl+K)。
    pub fn kill_to_end(&mut self) {
        let line = &mut self.lines[self.row];
        let idx = byte_index(line, self.col);
        line.truncate(idx);
    }

    /// 全テキストを消去 (Ctrl+U)。
    pub fn clear(&mut self) {
        self.set_text("");
    }

    fn clamp_col(&mut self) {
        self.col = self.col.min(char_len(&self.lines[self.row]));
    }
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

fn byte_index(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_strips_newlines() {
        let mut e = TextEditor::single_line("a\nb");
        assert_eq!(e.text(), "ab");
        e.insert_newline();
        e.insert_str("c\nd");
        assert_eq!(e.text(), "abcd");
        assert_eq!(e.lines().len(), 1);
    }

    #[test]
    fn insert_and_backspace_in_middle() {
        let mut e = TextEditor::single_line("helo");
        e.move_left();
        e.insert_char('l');
        assert_eq!(e.text(), "hello");
        assert_eq!(e.cursor(), (0, 4));
        e.backspace();
        e.backspace();
        assert_eq!(e.text(), "heo");
        assert_eq!(e.cursor(), (0, 2));
    }

    #[test]
    fn handles_multibyte_characters() {
        let mut e = TextEditor::single_line("日本");
        assert_eq!(e.cursor(), (0, 2));
        assert_eq!(e.cursor_display_col(), 4);
        e.move_left();
        e.insert_char('の');
        assert_eq!(e.text(), "日の本");
        e.delete();
        assert_eq!(e.text(), "日の");
        e.move_home();
        e.delete();
        assert_eq!(e.text(), "の");
    }

    #[test]
    fn multiline_newline_splits_and_backspace_joins() {
        let mut e = TextEditor::multi_line("abcd");
        e.move_left();
        e.move_left();
        e.insert_newline();
        assert_eq!(e.lines(), &["ab".to_string(), "cd".to_string()]);
        assert_eq!(e.cursor(), (1, 0));
        e.backspace();
        assert_eq!(e.text(), "abcd");
        assert_eq!(e.cursor(), (0, 2));
    }

    #[test]
    fn delete_at_line_end_joins_next_line() {
        let mut e = TextEditor::multi_line("ab\ncd");
        e.move_up();
        e.move_end();
        e.delete();
        assert_eq!(e.text(), "abcd");
    }

    #[test]
    fn vertical_movement_clamps_column() {
        let mut e = TextEditor::multi_line("long line\nab");
        assert_eq!(e.cursor(), (1, 2));
        e.move_up();
        e.move_end();
        assert_eq!(e.cursor(), (0, 9));
        e.move_down();
        assert_eq!(e.cursor(), (1, 2));
        e.move_down();
        assert_eq!(e.cursor(), (1, 2));
    }

    #[test]
    fn left_right_wrap_across_lines() {
        let mut e = TextEditor::multi_line("a\nb");
        e.move_home();
        e.move_left();
        assert_eq!(e.cursor(), (0, 1));
        e.move_right();
        assert_eq!(e.cursor(), (1, 0));
    }

    #[test]
    fn boundaries_are_noops() {
        let mut e = TextEditor::multi_line("");
        e.backspace();
        e.delete();
        e.move_left();
        e.move_right();
        e.move_up();
        e.move_down();
        assert!(e.is_empty());
        assert_eq!(e.cursor(), (0, 0));
    }

    #[test]
    fn kill_and_clear() {
        let mut e = TextEditor::single_line("hello world");
        e.move_home();
        for _ in 0..5 {
            e.move_right();
        }
        e.kill_to_end();
        assert_eq!(e.text(), "hello");
        e.clear();
        assert!(e.is_empty());
    }

    #[test]
    fn paste_multiline_text() {
        let mut e = TextEditor::multi_line("");
        e.insert_str("{\r\n  \"a\": 1\r\n}");
        assert_eq!(e.text(), "{\n  \"a\": 1\n}");
        assert_eq!(e.cursor(), (2, 1));
    }
}
