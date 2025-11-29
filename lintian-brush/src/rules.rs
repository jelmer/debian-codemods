/// Utilities for working with debian/rules files
use regex::bytes::Regex;

/// Drop a particular value from a --with argument in a dh command line.
/// This is a port of debmutate._rules.dh_invoke_drop_with from Python.
///
/// # Examples
///
/// ```
/// use lintian_brush::rules::dh_invoke_drop_with;
///
/// assert_eq!(
///     dh_invoke_drop_with(b"\tdh $@ --with=quilt", b"quilt"),
///     b"\tdh $@"
/// );
/// assert_eq!(
///     dh_invoke_drop_with(b"\tdh $@ --with=quilt,autoreconf", b"quilt"),
///     b"\tdh $@ --with=autoreconf"
/// );
/// ```
#[deprecated = "Use `debian_analyzer::rules::dh_invoke_drop_with` instead"]
pub fn dh_invoke_drop_with(line: &[u8], with_argument: &[u8]) -> Vec<u8> {
    // Check if the with_argument is even in the line
    if !line
        .windows(with_argument.len())
        .any(|w| w == with_argument)
    {
        return line.to_vec();
    }

    let mut result = line.to_vec();

    // It's the only with argument: --with quilt or --with=quilt at end
    let re1 = Regex::new(&format!(
        r"[ \t]--with[ =]{}( .+|)$",
        regex::escape(std::str::from_utf8(with_argument).unwrap())
    ))
    .unwrap();
    result = re1.replace_all(&result, &b"$1"[..]).to_vec();

    // It's at the beginning of a comma-separated list: --with=quilt,foo
    let re2 = Regex::new(&format!(
        r"([ \t])--with([ =]){},",
        regex::escape(std::str::from_utf8(with_argument).unwrap())
    ))
    .unwrap();
    result = re2.replace_all(&result, &b"$1--with$2"[..]).to_vec();

    // It's somewhere in the middle or the end: --with=foo,quilt,bar or --with=foo,quilt
    let re3 = Regex::new(&format!(
        r"([ \t])--with([ =])(.+),{}([ ,])",
        regex::escape(std::str::from_utf8(with_argument).unwrap())
    ))
    .unwrap();
    result = re3.replace_all(&result, &b"$1--with$2$3$4"[..]).to_vec();

    // It's at the end: --with=foo,quilt$
    let re4 = Regex::new(&format!(
        r"([ \t])--with([ =])(.+),{}$",
        regex::escape(std::str::from_utf8(with_argument).unwrap())
    ))
    .unwrap();
    result = re4.replace_all(&result, &b"$1--with$2$3"[..]).to_vec();

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dh_invoke_drop_with_only_argument() {
        assert_eq!(
            dh_invoke_drop_with(b"\tdh $@ --with=quilt", b"quilt"),
            b"\tdh $@"
        );
        assert_eq!(
            dh_invoke_drop_with(b"\tdh $@ --with quilt", b"quilt"),
            b"\tdh $@"
        );
    }

    #[test]
    fn test_dh_invoke_drop_with_first_in_list() {
        assert_eq!(
            dh_invoke_drop_with(b"\tdh $@ --with=quilt,autoreconf", b"quilt"),
            b"\tdh $@ --with=autoreconf"
        );
    }

    #[test]
    fn test_dh_invoke_drop_with_last_in_list() {
        assert_eq!(
            dh_invoke_drop_with(b"\tdh $@ --with=autoreconf,quilt", b"quilt"),
            b"\tdh $@ --with=autoreconf"
        );
    }

    #[test]
    fn test_dh_invoke_drop_with_middle_of_list() {
        assert_eq!(
            dh_invoke_drop_with(b"\tdh $@ --with=foo,quilt,bar", b"quilt"),
            b"\tdh $@ --with=foo,bar"
        );
    }

    #[test]
    fn test_dh_invoke_drop_with_not_present() {
        assert_eq!(
            dh_invoke_drop_with(b"\tdh $@ --with=autoreconf", b"quilt"),
            b"\tdh $@ --with=autoreconf"
        );
    }
}
