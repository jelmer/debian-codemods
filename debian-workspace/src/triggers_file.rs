//! Minimal parser for `debian/triggers` / `debian/<pkg>.triggers`.
//!
//! The format is line-based: blank lines and lines starting with `#` are
//! ignored; every other line is `<type> <name>` (whitespace-delimited,
//! with optional leading whitespace). This module reads the format into
//! structured pieces so a caller can inspect or rewrite the type token
//! without doing its own whitespace surgery.

/// One non-comment, non-blank line of a triggers file, split into its
/// pieces so the type token can be swapped out while preserving the
/// surrounding whitespace exactly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TriggerLine {
    /// 1-based line number in the source file.
    pub line_no: usize,
    /// Leading whitespace before the type token (may be empty).
    pub leading_ws: String,
    /// Trigger type - the first whitespace-delimited token.
    pub trigger_type: String,
    /// Everything after the type token, including the whitespace that
    /// separates it from the trigger name. Empty if the line has no
    /// trigger name (which is syntactically invalid but we preserve it).
    pub rest: String,
}

impl TriggerLine {
    /// Produce the raw line text (without trailing newline) with the
    /// trigger type replaced by `new_type`.
    pub fn render_with_type(&self, new_type: &str) -> String {
        let mut out =
            String::with_capacity(self.leading_ws.len() + new_type.len() + self.rest.len());
        out.push_str(&self.leading_ws);
        out.push_str(new_type);
        out.push_str(&self.rest);
        out
    }
}

/// Parse `content` as a triggers file, yielding one `TriggerLine` per
/// non-comment, non-blank line.
pub fn parse(content: &str) -> impl Iterator<Item = TriggerLine> + '_ {
    content.lines().enumerate().filter_map(|(idx, line)| {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return None;
        }
        let leading_ws_len = line.len() - line.trim_start().len();
        let (leading_ws, after_ws) = line.split_at(leading_ws_len);
        let (trigger_type, rest) = match after_ws.find(char::is_whitespace) {
            Some(pos) => (&after_ws[..pos], &after_ws[pos..]),
            None => (after_ws, ""),
        };
        if trigger_type.is_empty() {
            return None;
        }
        Some(TriggerLine {
            line_no: idx + 1,
            leading_ws: leading_ws.to_string(),
            trigger_type: trigger_type.to_string(),
            rest: rest.to_string(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_skips_comments_and_blank_lines() {
        let content = "# comment\n\n  interest /usr/share/foo\nactivate /usr/share/bar\n";
        let lines: Vec<_> = parse(content).collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].line_no, 3);
        assert_eq!(lines[0].leading_ws, "  ");
        assert_eq!(lines[0].trigger_type, "interest");
        assert_eq!(lines[0].rest, " /usr/share/foo");
        assert_eq!(lines[1].line_no, 4);
        assert_eq!(lines[1].leading_ws, "");
        assert_eq!(lines[1].trigger_type, "activate");
    }

    #[test]
    fn parse_preserves_tab_separated_rest() {
        let content = "interest\t/etc/foo\n";
        let lines: Vec<_> = parse(content).collect();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].rest, "\t/etc/foo");
    }

    #[test]
    fn render_with_type_preserves_whitespace() {
        let content = "   interest\t/etc/foo\n";
        let line = parse(content).next().unwrap();
        assert_eq!(
            line.render_with_type("interest-await"),
            "   interest-await\t/etc/foo"
        );
    }

    #[test]
    fn render_with_type_handles_no_rest() {
        let content = "activate\n";
        let line = parse(content).next().unwrap();
        assert_eq!(line.render_with_type("activate-await"), "activate-await");
    }
}
