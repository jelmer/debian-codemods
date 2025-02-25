//! This module provides functions to manipulate debian/rules file.

/// Add a particular value to a with argument.
pub fn dh_invoke_add_with(line: &str, with_argument: &str) -> String {
    if line.contains(with_argument) {
        return line.to_owned();
    }
    if !line.contains(" --with") {
        return format!("{} --with={}", line, with_argument);
    }

    lazy_regex::regex_replace!(
        r"([ \t])--with([ =])([^ \t]+)",
        line,
        |_, head, with, tail| format!("{}--with={},{}{}", head, with_argument, with, tail)
    )
    .to_string()
}

/// Obtain the value of a with argument.
pub fn dh_invoke_get_with(line: &str) -> Vec<String> {
    let mut ret = Vec::new();
    for m in lazy_regex::regex!("[ \t]--with[ =]([^ \t]+)").find_iter(line) {
        ret.extend(m.as_str().split(',').map(|s| s.to_owned()));
    }
    ret
}
