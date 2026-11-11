//! シンプルなテキストエディタ (単一行 / 複数行)。
//!
//! カーソル位置は「文字 (char) 単位」で保持し、表示幅は `unicode-width` で計算する。

use unicode_width::UnicodeWidthStr;

/// undo 用のスナップショット。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    lines: Vec<String>,
    row: usize,
    col: usize,
}

const UNDO_LIMIT: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEditor {
    lines: Vec<String>,
    row: usize,
    col: usize,
    multiline: bool,
    undo_stack: Vec<Snapshot>,
}

/// Vim の単語境界判定に使う文字種。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Space,
    Word,
    Punct,
}

fn char_class(c: Option<char>) -> CharClass {
    match c {
        None => CharClass::Space,
        Some(c) if c.is_whitespace() => CharClass::Space,
        Some(c) if c.is_alphanumeric() || c == '_' || !c.is_ascii() => CharClass::Word,
        Some(_) => CharClass::Punct,
    }
}

impl TextEditor {
    pub fn single_line(text: &str) -> Self {
        let mut e = Self {
            lines: vec![String::new()],
            row: 0,
            col: 0,
            multiline: false,
            undo_stack: Vec::new(),
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
            undo_stack: Vec::new(),
        };
        e.set_text(text);
        e
    }

    pub fn is_multiline(&self) -> bool {
        self.multiline
    }

    /// テキスト全体を置き換え、カーソルを末尾に移動する。undo 履歴は破棄する。
    pub fn set_text(&mut self, text: &str) {
        self.undo_stack.clear();
        self.replace_text(text);
    }

    fn replace_text(&mut self, text: &str) {
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

    /// 全テキストを消去 (Ctrl+U)。undo 履歴は残す。
    pub fn clear(&mut self) {
        self.replace_text("");
    }

    // ---- Vim 向けの操作 ----

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line_len(&self, row: usize) -> usize {
        char_len(&self.lines[row])
    }

    /// カーソルを移動する (範囲外は丸める)。
    pub fn set_cursor(&mut self, row: usize, col: usize) {
        self.row = row.min(self.lines.len() - 1);
        self.col = col.min(self.line_len(self.row));
    }

    /// 表示上の列 (全角文字は 2 カラム) に対応する文字単位の列。行末より右は行末に丸める。
    pub fn col_at_display(&self, row: usize, display_col: usize) -> usize {
        let mut width = 0;
        for (i, c) in self.lines[row].chars().enumerate() {
            width += unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
            if display_col < width {
                return i;
            }
        }
        self.line_len(row)
    }

    /// Normal モード用にカーソルを丸める (行末の 1 つ先には置かない)。
    pub fn clamp_normal(&mut self) {
        self.col = self.col.min(self.line_len(self.row).saturating_sub(1));
    }

    /// 現在の状態を undo 履歴に積む。変更操作の直前に呼ぶ。
    pub fn checkpoint(&mut self) {
        let snap = Snapshot {
            lines: self.lines.clone(),
            row: self.row,
            col: self.col,
        };
        if self.undo_stack.last() == Some(&snap) {
            return;
        }
        self.undo_stack.push(snap);
        if self.undo_stack.len() > UNDO_LIMIT {
            self.undo_stack.remove(0);
        }
    }

    /// 直前の checkpoint に戻す。戻せたら true。
    pub fn undo(&mut self) -> bool {
        // 変更が無かった checkpoint は飛ばす
        while let Some(snap) = self.undo_stack.pop() {
            if snap.lines != self.lines {
                self.lines = snap.lines;
                self.row = snap.row;
                self.col = snap.col;
                self.clamp_normal();
                return true;
            }
        }
        false
    }

    /// 行内で左へ (行をまたがない)。
    pub fn move_left_in_line(&mut self) {
        self.col = self.col.saturating_sub(1);
    }

    /// 行内で右へ (Normal モード: 最後の文字で止まる)。
    pub fn move_right_normal(&mut self) {
        if self.col + 1 < self.line_len(self.row) {
            self.col += 1;
        }
    }

    pub fn first_non_blank_col(&self, row: usize) -> usize {
        self.lines[row]
            .chars()
            .position(|c| !c.is_whitespace())
            .unwrap_or(0)
    }

    /// (row, col) の文字。行末 (改行位置) は None。
    fn char_at(&self, (row, col): (usize, usize)) -> Option<char> {
        self.lines[row].chars().nth(col)
    }

    /// 改行も 1 文字として数えた次の位置。
    fn next_pos(&self, (row, col): (usize, usize)) -> Option<(usize, usize)> {
        if col < self.line_len(row) {
            Some((row, col + 1))
        } else if row + 1 < self.lines.len() {
            Some((row + 1, 0))
        } else {
            None
        }
    }

    fn prev_pos(&self, (row, col): (usize, usize)) -> Option<(usize, usize)> {
        if col > 0 {
            Some((row, col - 1))
        } else if row > 0 {
            Some((row - 1, self.line_len(row - 1)))
        } else {
            None
        }
    }

    fn class_at(&self, pos: (usize, usize)) -> CharClass {
        char_class(self.char_at(pos))
    }

    /// `w`: 次の単語の先頭。末尾に達したらテキスト末尾。
    pub fn word_forward_pos(&self) -> (usize, usize) {
        let mut pos = (self.row, self.col);
        let start = self.class_at(pos);
        if start != CharClass::Space {
            while self.class_at(pos) == start {
                match self.next_pos(pos) {
                    Some(p) => pos = p,
                    None => return pos,
                }
            }
        }
        while self.class_at(pos) == CharClass::Space {
            match self.next_pos(pos) {
                Some(p) => pos = p,
                None => return pos,
            }
        }
        pos
    }

    /// `e`: 単語の末尾 (その文字自身を指す)。
    pub fn word_end_pos(&self) -> (usize, usize) {
        let origin = (self.row, self.col);
        let Some(mut pos) = self.next_pos(origin) else {
            return origin;
        };
        while self.class_at(pos) == CharClass::Space {
            match self.next_pos(pos) {
                Some(p) => pos = p,
                None => return origin,
            }
        }
        let class = self.class_at(pos);
        while let Some(next) = self.next_pos(pos) {
            if self.class_at(next) != class {
                break;
            }
            pos = next;
        }
        pos
    }

    /// `b`: 前の単語の先頭。
    pub fn word_backward_pos(&self) -> (usize, usize) {
        let Some(mut pos) = self.prev_pos((self.row, self.col)) else {
            return (self.row, self.col);
        };
        while self.class_at(pos) == CharClass::Space {
            match self.prev_pos(pos) {
                Some(p) => pos = p,
                None => return pos,
            }
        }
        let class = self.class_at(pos);
        while let Some(prev) = self.prev_pos(pos) {
            if self.class_at(prev) != class {
                break;
            }
            pos = prev;
        }
        pos
    }

    /// [start, end) の文字列 (改行を含む)。
    pub fn text_range(&self, start: (usize, usize), end: (usize, usize)) -> String {
        let mut out = String::new();
        let mut pos = start;
        while pos < end {
            match self.char_at(pos) {
                Some(c) => out.push(c),
                None => out.push('\n'),
            }
            match self.next_pos(pos) {
                Some(p) => pos = p,
                None => break,
            }
        }
        out
    }

    /// [start, end) を削除し、カーソルを start に置く。削除した文字列を返す。
    pub fn delete_range(&mut self, start: (usize, usize), end: (usize, usize)) -> String {
        let removed = self.text_range(start, end);
        let (sr, sc) = start;
        let (er, ec) = end;
        let er = er.min(self.lines.len() - 1);
        let ec = ec.min(self.line_len(er));
        let tail = self.lines[er][byte_index(&self.lines[er], ec)..].to_string();
        let head_end = byte_index(&self.lines[sr], sc);
        self.lines[sr].truncate(head_end);
        self.lines[sr].push_str(&tail);
        self.lines.drain(sr + 1..=er.max(sr));
        self.row = sr;
        self.col = sc;
        removed
    }

    /// first..=last 行の文字列 (改行区切り)。
    pub fn lines_text(&self, first: usize, last: usize) -> String {
        self.lines[first..=last].join("\n")
    }

    /// first..=last 行を削除する。削除した文字列を返す。
    pub fn delete_lines(&mut self, first: usize, last: usize) -> String {
        let removed = self.lines_text(first, last);
        self.lines.drain(first..=last);
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.row = first.min(self.lines.len() - 1);
        self.col = self.first_non_blank_col(self.row);
        removed
    }

    /// row 行の後ろ (above なら前) に行を挿入し、先頭行にカーソルを置く。
    pub fn insert_lines(&mut self, row: usize, above: bool, text: &str) {
        if !self.multiline {
            self.lines[0].push_str(&text.replace(['\n', '\r'], ""));
            self.col = self.line_len(0).saturating_sub(1);
            return;
        }
        let at = if above { row } else { row + 1 };
        let new: Vec<String> = text.split('\n').map(str::to_string).collect();
        self.lines.splice(at..at, new);
        self.row = at;
        self.col = self.first_non_blank_col(at);
    }

    /// 現在行の内容を空にする (`cc` / `S`)。
    pub fn clear_line(&mut self) -> String {
        let removed = std::mem::take(&mut self.lines[self.row]);
        self.col = 0;
        removed
    }

    /// 直前の単語を削除 (Insert モードの Ctrl+W)。
    pub fn delete_word_before(&mut self) {
        if self.col == 0 {
            self.backspace();
            return;
        }
        let chars: Vec<char> = self.lines[self.row].chars().collect();
        let mut c = self.col;
        while c > 0 && chars[c - 1].is_whitespace() {
            c -= 1;
        }
        if c > 0 {
            let class = char_class(Some(chars[c - 1]));
            while c > 0 && char_class(Some(chars[c - 1])) == class {
                c -= 1;
            }
        }
        let end = (self.row, self.col);
        self.delete_range((self.row, c), end);
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

    #[test]
    fn word_motions() {
        let mut e = TextEditor::multi_line("foo.bar  baz\nqux");
        e.set_cursor(0, 0);
        e.set_cursor(e.word_forward_pos().0, e.word_forward_pos().1);
        assert_eq!(e.cursor(), (0, 3)); // "."
        let p = e.word_forward_pos();
        e.set_cursor(p.0, p.1);
        assert_eq!(e.cursor(), (0, 4)); // "bar"
        let p = e.word_forward_pos();
        e.set_cursor(p.0, p.1);
        assert_eq!(e.cursor(), (0, 9)); // "baz"
        let p = e.word_forward_pos();
        e.set_cursor(p.0, p.1);
        assert_eq!(e.cursor(), (1, 0)); // 次の行
        let p = e.word_backward_pos();
        e.set_cursor(p.0, p.1);
        assert_eq!(e.cursor(), (0, 9));
        e.set_cursor(0, 4);
        assert_eq!(e.word_end_pos(), (0, 6));
        e.set_cursor(0, 6);
        assert_eq!(e.word_end_pos(), (0, 11));
    }

    #[test]
    fn delete_range_across_lines() {
        let mut e = TextEditor::multi_line("abc\ndef\nghi");
        let removed = e.delete_range((0, 1), (2, 1));
        assert_eq!(removed, "bc\ndef\ng");
        assert_eq!(e.text(), "ahi");
        assert_eq!(e.cursor(), (0, 1));
    }

    #[test]
    fn delete_and_insert_lines() {
        let mut e = TextEditor::multi_line("a\nb\nc");
        assert_eq!(e.delete_lines(0, 1), "a\nb");
        assert_eq!(e.text(), "c");
        e.insert_lines(0, false, "x\ny");
        assert_eq!(e.text(), "c\nx\ny");
        assert_eq!(e.cursor(), (1, 0));
        e.insert_lines(0, true, "z");
        assert_eq!(e.text(), "z\nc\nx\ny");
        e.delete_lines(0, 3);
        assert!(e.is_empty());
    }

    #[test]
    fn undo_restores_checkpoint() {
        let mut e = TextEditor::multi_line("hello");
        e.checkpoint();
        e.insert_str(" world");
        e.checkpoint();
        e.clear();
        assert!(e.undo());
        assert_eq!(e.text(), "hello world");
        assert!(e.undo());
        assert_eq!(e.text(), "hello");
        assert!(!e.undo());
        // set_text は履歴を消す
        e.checkpoint();
        e.set_text("x");
        assert!(!e.undo());
    }

    #[test]
    fn delete_word_before_cursor() {
        let mut e = TextEditor::single_line("curl http://a");
        e.delete_word_before();
        assert_eq!(e.text(), "curl http://");
        e.delete_word_before();
        assert_eq!(e.text(), "curl http");
        e.delete_word_before();
        e.delete_word_before();
        assert_eq!(e.text(), "");
    }
}
