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
            return Some(if let Some(stripped) = before.strip_suffix(' ') {
                format!("{}{}", stripped, after)
            } else if let Some(stripped) = after.strip_prefix(' ') {
                format!("{}{}", before, stripped)
            } else {
                format!("{}{}", before, after)
            });
        }

        search_from = pos + 1;
    }
    None
}

/// Find the position and end of a `--key=value` or `--key value` option in a line.
/// Returns `Some((start, end))` where start..end covers the key and value portion.
/// Returns `None` if the key is not found as a whole token.
fn find_option_argument(line: &str, key: &str) -> Option<(usize, usize)> {
    let bytes = line.as_bytes();
    let mut search_from = 0;

    while search_from + key.len() <= line.len() {
        let Some(rel) = line[search_from..].find(key) else {
            break;
        };
        let pos = search_from + rel;
        let after_key = pos + key.len();

        // Must be preceded by whitespace (or start of line)
        let before_ok = pos == 0 || bytes[pos - 1] == b' ' || bytes[pos - 1] == b'\t';
        if !before_ok {
            search_from = pos + 1;
            continue;
        }

        if after_key < line.len() && bytes[after_key] == b'=' {
            // --key=value form: find end of value
            let value_start = after_key + 1;
            let value_end = line[value_start..]
                .find(|c: char| c == ' ' || c == '\t')
                .map_or(line.len(), |i| value_start + i);
            return Some((pos, value_end));
        } else if after_key < line.len() && (bytes[after_key] == b' ' || bytes[after_key] == b'\t')
        {
            // --key value form: find end of value
            let value_start = after_key + 1;
            let value_end = line[value_start..]
                .find(|c: char| c == ' ' || c == '\t')
                .map_or(line.len(), |i| value_start + i);
            return Some((pos, value_end));
        }

        search_from = pos + 1;
    }

    None
}

/// Replace or add a `--key=value` style option argument in a dh invocation line.
///
/// If `--key=<old_value>` or `--key <old_value>` is present, replace the value.
/// If the key is not present, append `--key=value` to the line.
pub fn dh_invoke_set_option_argument(line: &str, key: &str, value: &str) -> String {
    if let Some((start, end)) = find_option_argument(line, key) {
        format!("{}{}={}{}", &line[..start], key, value, &line[end..])
    } else {
        format!("{} {}={}", line.trim_end(), key, value)
    }
}

/// Like [`dh_invoke_set_option_argument`], but only adds the argument if the key
/// is not already present. Existing values are left untouched.
pub fn dh_invoke_set_option_argument_soft(line: &str, key: &str, value: &str) -> String {
    if find_option_argument(line, key).is_some() {
        return line.to_owned();
    }
    format!("{} {}={}", line.trim_end(), key, value)
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

    #[test]
    fn test_set_option_argument_add() {
        assert_eq!(
            dh_invoke_set_option_argument(
                "\tdh_auto_configure -- --prefix=/usr",
                "--libexecdir",
                "/usr/libexec"
            ),
            "\tdh_auto_configure -- --prefix=/usr --libexecdir=/usr/libexec"
        );
    }

    #[test]
    fn test_set_option_argument_replace_eq() {
        assert_eq!(
            dh_invoke_set_option_argument(
                "\tdh_auto_configure -- --libexecdir=/old/path",
                "--libexecdir",
                "/usr/libexec"
            ),
            "\tdh_auto_configure -- --libexecdir=/usr/libexec"
        );
    }

    #[test]
    fn test_set_option_argument_replace_space() {
        assert_eq!(
            dh_invoke_set_option_argument(
                "\tdh_auto_configure -- --libexecdir /old/path",
                "--libexecdir",
                "/usr/libexec"
            ),
            "\tdh_auto_configure -- --libexecdir=/usr/libexec"
        );
    }

    #[test]
    fn test_set_option_argument_replace_middle() {
        assert_eq!(
            dh_invoke_set_option_argument(
                "\tdh_auto_configure -- --libexecdir=/old --other",
                "--libexecdir",
                "/usr/libexec"
            ),
            "\tdh_auto_configure -- --libexecdir=/usr/libexec --other"
        );
    }

    #[test]
    fn test_set_option_argument_soft_add() {
        assert_eq!(
            dh_invoke_set_option_argument_soft(
                "\tdh_auto_configure -- --prefix=/usr",
                "--libexecdir",
                "/usr/libexec"
            ),
            "\tdh_auto_configure -- --prefix=/usr --libexecdir=/usr/libexec"
        );
    }

    #[test]
    fn test_set_option_argument_soft_noop() {
        let line = "\tdh_auto_configure -- --libexecdir=/custom/path";
        assert_eq!(
            dh_invoke_set_option_argument_soft(line, "--libexecdir", "/usr/libexec"),
            line
        );
    }

    #[test]
    fn test_set_option_argument_no_partial_match() {
        // --libexecdir-foo should not match --libexecdir
        assert_eq!(
            dh_invoke_set_option_argument_soft(
                "\tdh_auto_configure -- --libexecdir-foo=/bar",
                "--libexecdir",
                "/usr/libexec"
            ),
            "\tdh_auto_configure -- --libexecdir-foo=/bar --libexecdir=/usr/libexec"
        );
    }
}
