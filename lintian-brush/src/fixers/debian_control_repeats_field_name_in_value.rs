use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

/// Strip `prefix` from the start of `s`, comparing case-insensitively over
/// ASCII (deb822 field names are ASCII). Returns the remainder, or `None` if
/// `s` does not start with `prefix`.
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() < prefix.len() || !s.is_char_boundary(prefix.len()) {
        return None;
    }
    let (head, tail) = s.split_at(prefix.len());
    head.eq_ignore_ascii_case(prefix).then_some(tail)
}

/// If `value` starts with the field name followed by a colon (matching
/// lintian's `^ \Q$field\E \s* : `, case-insensitively), return the value
/// with that prefix stripped. Returns `None` if the prefix is not present, or
/// if stripping it would leave an empty value -- we will not replace a field
/// with nothing.
fn strip_repeated_field_name(field: &str, value: &str) -> Option<String> {
    let rest = strip_prefix_ci(value, field)?;
    let rest = rest.trim_start_matches([' ', '\t']);
    let rest = rest.strip_prefix(':')?;
    let rest = rest.trim_start_matches([' ', '\t']);
    if rest.is_empty() {
        return None;
    }
    Some(rest.to_string())
}

/// Collect diagnostics for every field in `paragraph` whose value repeats the
/// field name. `section` is the lintian "(in section for ...)" label and
/// `selector` targets the paragraph for the fixing action.
///
/// lintian reports this tag as a source-package issue even for binary
/// sections, distinguishing the section only via the info text.
fn collect_diagnostics(
    paragraph: &deb822_lossless::Paragraph,
    section: &str,
    selector: ParagraphSelector,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for entry in paragraph.entries() {
        let Some(key) = entry.key() else {
            continue;
        };
        let value = entry.value();
        let Some(stripped) = strip_repeated_field_name(&key, &value) else {
            continue;
        };
        let line_no = entry.line() + 1;

        let issue = LintianIssue::source_with_info(
            "debian-control-repeats-field-name-in-value",
            Visibility::Warning,
            vec![
                format!("(in section for {})", section),
                key.to_string(),
                format!("[debian/control:{}]", line_no),
            ],
        );

        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!(
                "Field {} in section for {} repeats its name in the value.",
                key, section
            ),
            format!("Drop repeated field name from {} value.", key),
            vec![Action::Deb822(Deb822Action::SetField {
                file: PathBuf::from("debian/control"),
                paragraph: selector.clone(),
                field: key.to_string(),
                value: stripped,
            })],
        ));
    }
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let mut diagnostics = Vec::new();

    if let Some(source) = control.source() {
        collect_diagnostics(
            source.as_deb822(),
            "source",
            ParagraphSelector::Source,
            &mut diagnostics,
        );
    }

    for binary in control.binaries() {
        let Some(package_name) = binary.name() else {
            continue;
        };
        collect_diagnostics(
            binary.as_deb822(),
            &package_name,
            ParagraphSelector::Binary {
                package: package_name.clone(),
            },
            &mut diagnostics,
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "debian-control-repeats-field-name-in-value",
    tags: ["debian-control-repeats-field-name-in-value"],
    triggers: [
        debian_workspace::Trigger::File("debian/control"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_exact() {
        assert_eq!(
            strip_repeated_field_name("Description", "Description: a tool"),
            Some("a tool".to_string())
        );
    }

    #[test]
    fn test_strip_case_insensitive() {
        assert_eq!(
            strip_repeated_field_name("Maintainer", "maintainer: Jane <j@example.com>"),
            Some("Jane <j@example.com>".to_string())
        );
    }

    #[test]
    fn test_strip_whitespace_before_colon() {
        assert_eq!(
            strip_repeated_field_name("Section", "Section : libs"),
            Some("libs".to_string())
        );
    }

    #[test]
    fn test_strip_no_space_after_colon() {
        assert_eq!(
            strip_repeated_field_name("Section", "Section:libs"),
            Some("libs".to_string())
        );
    }

    #[test]
    fn test_no_repeat() {
        assert_eq!(
            strip_repeated_field_name("Description", "a tool: for things"),
            None
        );
    }

    #[test]
    fn test_field_name_substring_not_followed_by_colon() {
        // "Section-Extra" is not the field name followed by a colon.
        assert_eq!(
            strip_repeated_field_name("Section", "Section-Extra: libs"),
            None
        );
    }

    #[test]
    fn test_value_shorter_than_field() {
        assert_eq!(strip_repeated_field_name("Description", "x"), None);
    }

    #[test]
    fn test_empty_after_strip_backs_off() {
        // "Description: Description:" has no real value after the repeated
        // name; we must not replace the field with an empty value.
        assert_eq!(
            strip_repeated_field_name("Description", "Description:"),
            None
        );
        assert_eq!(
            strip_repeated_field_name("Description", "Description:   "),
            None
        );
    }
}
