/// Utilities for manipulating debian/rules files.

/// Remove a `--with <addon>` or `--with=<addon>` argument from a `dh` command line.
///
/// Only the exact token(s) are removed; no other whitespace in the line is touched.
/// Returns the line unchanged if the argument is not present.
pub fn drop_dh_with_argument(line: &str, addon: &str) -> String {
    for pattern in [format!("--with={}", addon), format!("--with {}", addon)] {
        if let Some(result) = remove_whole_token(line, &pattern) {
            return result;
        }
    }
    line.to_string()
}

/// Remove `pattern` from `line` if it appears as a whole space-delimited token.
/// Consumes exactly one adjacent space to avoid introducing a double space.
/// Returns None if the pattern is not found as a whole token.
fn remove_whole_token(line: &str, pattern: &str) -> Option<String> {
    let pat_len = pattern.len();
    let bytes = line.as_bytes();
    let mut search_from = 0;

    while search_from + pat_len <= line.len() {
        let rel = line[search_from..].find(pattern)?;
        let pos = search_from + rel;
        let end = pos + pat_len;

        let before_ok = pos == 0 || bytes[pos - 1] == b' ';
        let after_ok = end == line.len() || bytes[end] == b' ';

        if before_ok && after_ok {
            let before = &line[..pos];
            let after = &line[end..];
            // Consume one adjacent space so we don't leave a double space.
            return Some(if before.ends_with(' ') {
                format!("{}{}", &before[..before.len() - 1], after)
            } else if after.starts_with(' ') {
                format!("{}{}", before, &after[1..])
            } else {
                format!("{}{}", before, after)
            });
        }

        search_from = pos + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drop_with_space_form_trailing() {
        assert_eq!(
            drop_dh_with_argument("\tdh $@ --with autotools-dev", "autotools-dev"),
            "\tdh $@"
        );
    }

    #[test]
    fn test_drop_with_eq_form_trailing() {
        assert_eq!(
            drop_dh_with_argument("\tdh $@ --with=autotools-dev", "autotools-dev"),
            "\tdh $@"
        );
    }

    #[test]
    fn test_drop_with_space_form_leading() {
        assert_eq!(
            drop_dh_with_argument("\tdh --with autotools-dev $@", "autotools-dev"),
            "\tdh $@"
        );
    }

    #[test]
    fn test_drop_with_space_form_middle() {
        assert_eq!(
            drop_dh_with_argument("\tdh $@ --with autotools-dev --other", "autotools-dev"),
            "\tdh $@ --other"
        );
    }

    #[test]
    fn test_no_match_unchanged() {
        let line = "\tdh $@ --with other-addon";
        assert_eq!(drop_dh_with_argument(line, "autotools-dev"), line);
    }

    #[test]
    fn test_no_partial_match() {
        // --without should not match --with
        let line = "\tdh $@ --without autotools-dev";
        assert_eq!(drop_dh_with_argument(line, "autotools-dev"), line);
    }
}
