use deb822_lossless::{Deb822, Paragraph};
use debian_analyzer::benfile::{Comparison, Expr};
use debian_analyzer::transition::Transition;
use debian_control::lossless::Control;
use regex::Regex;

fn find_expr_by_field_name<'a>(expr: &'a Expr, field_name: &'a str) -> Option<&'a Expr> {
    let exprs = match expr {
        Expr::Or(exprs) => exprs,
        _ => return None,
    };
    exprs
        .iter()
        .find(|expr| match expr.as_ref() {
            Expr::FieldRegex(f, _) => f == field_name,
            Expr::FieldString(f, _n) => f == field_name,
            Expr::FieldComparison(f, _, _) => f == field_name,
            _ => false,
        })
        .map(|e| e.as_ref())
}

#[derive(Debug)]
enum Match {
    Regex(Regex),
    String(String),
    Comparison(Comparison, String),
}

fn compare(_operator: &Comparison, _value: &str, _other: &str) -> bool {
    todo!()
}

impl Match {
    fn applies(&self, value: &str) -> bool {
        match self {
            Match::Regex(re) => re.is_match(value),
            Match::String(s) => s == value,
            Match::Comparison(c, s) => compare(c, value, s),
        }
    }
}

fn map_bad_to_good(bad: &Expr, good: &Expr) -> Result<Vec<(String, Match, Match)>, String> {
    let mut used = vec![];
    let entries = match bad {
        Expr::And(entries) => entries,
        _ => return Err("bad must be an And".to_string()),
    };
    let ret = entries
        .iter()
        .map(|entry| {
            let (f, o) = match entry.as_ref() {
                Expr::FieldRegex(f, regex) => {
                    (f.to_string(), Match::Regex(Regex::new(regex).unwrap()))
                }
                Expr::FieldString(f, s) => (f.to_string(), Match::String(s.to_string())),
                Expr::FieldComparison(f, c, s) => {
                    (f.to_string(), Match::Comparison(c.clone(), s.to_string()))
                }
                _ => return Err(format!("unable to find replacement value for {:?}", entry)),
            };

            let replacement = if let Some(good) = find_expr_by_field_name(good, &f) {
                used.push(f.clone());
                match good {
                    Expr::FieldString(_, s) => Match::String(s.to_string()),
                    Expr::FieldRegex(_, r) => Match::Regex(Regex::new(r).unwrap()),
                    Expr::FieldComparison(_, c, s) => Match::Comparison(c.clone(), s.to_string()),
                    _ => return Err(format!("unable to find replacement value for {}", f)),
                }
            } else {
                return Err(format!("unable to find replacement value for {}", f));
            };
            Ok((f, o, replacement))
        })
        .collect();

    let exprs = match good {
        Expr::Or(exprs) => exprs,
        _ => return Err("good must be an Or".to_string()),
    };

    // check that all fields in good were used
    for expr in exprs {
        match expr.as_ref() {
            Expr::FieldRegex(f, _) | Expr::FieldString(f, _) | Expr::FieldComparison(f, _, _) => {
                if !used.contains(f) {
                    return Err(format!("extra field in good: {}", f));
                }
            }
            _ => {
                return Err(format!("unsupported expr in good: {:?}", expr));
            }
        }
    }
    ret
}

fn para_matches(para: &Paragraph, expr: &Expr) -> bool {
    match expr {
        Expr::FieldRegex(f, regex) => {
            if let Some(value) = para.get(f) {
                Regex::new(regex).unwrap().is_match(&value)
            } else {
                false
            }
        }
        Expr::FieldString(f, s) => {
            if let Some(value) = para.get(f) {
                value == *s
            } else {
                false
            }
        }
        Expr::FieldComparison(f, c, s) => {
            if let Some(value) = para.get(f) {
                compare(c, &value, s)
            } else {
                false
            }
        }
        Expr::Not(e) => !para_matches(para, e),
        _ => unreachable!(),
    }
}

fn control_matches(control: &Deb822, expr: &Expr) -> bool {
    match expr {
        Expr::Bool(b) => *b,
        Expr::And(exprs) => exprs.iter().all(|expr| control_matches(control, expr)),
        Expr::Or(exprs) => exprs.iter().any(|expr| control_matches(control, expr)),
        o => control.paragraphs().any(|para| para_matches(&para, o)),
    }
}

fn transition_find_bugno(transition: &Transition) -> Vec<i32> {
    transition
        .notes
        .as_ref()
        .map(|notes| {
            lazy_regex::regex!("#([0-9]+)")
                .find_iter(notes)
                .map(|m| m.as_str()[1..].parse().unwrap())
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Debug)]
pub enum TransitionResult {
    PackageNotAffected(String),
    PackageAlreadyGood(String),
    PackageNotBad(String),
    TransitionSuccess(String, Vec<i32>),
    Unsupported(String),
}

impl TransitionResult {
    pub fn is_success(&self) -> bool {
        matches!(self, TransitionResult::TransitionSuccess(_, _))
    }

    pub fn is_noop(&self) -> bool {
        !matches!(self, TransitionResult::TransitionSuccess(_, _))
    }
}

pub fn apply_transition(control: &mut Control, transition: &Transition) -> TransitionResult {
    if let Some(is_affected) = &transition.is_affected {
        if !control_matches(control.as_deb822(), is_affected) {
            return TransitionResult::PackageNotAffected(control.source().unwrap().to_string());
        }
    }
    if let Some(is_good) = &transition.is_good {
        if control_matches(control.as_deb822(), is_good) {
            return TransitionResult::PackageAlreadyGood(control.source().unwrap().to_string());
        }
    }
    if let Some(is_bad) = &transition.is_bad {
        if !control_matches(control.as_deb822(), is_bad) {
            return TransitionResult::PackageNotBad(control.source().unwrap().to_string());
        }
    }

    if transition.is_bad.is_none() || transition.is_good.is_none() {
        return TransitionResult::PackageNotBad(control.source().unwrap().to_string());
    }

    let map = map_bad_to_good(
        transition.is_bad.as_ref().unwrap(),
        transition.is_good.as_ref().unwrap(),
    )
    .unwrap();

    let deb822 = control.as_mut_deb822();

    for (field, bad, good) in map {
        for mut para in deb822.paragraphs() {
            if let Some(old_value) = para.get(&field) {
                if bad.applies(&old_value) {
                    let new_value = match (&bad, &good) {
                        (Match::String(o), Match::String(n)) => old_value.replace(o, n),
                        (Match::Regex(o), Match::String(n)) => o.replace(&old_value, n).to_string(),
                        (_, _) => {
                            return TransitionResult::Unsupported(format!(
                                "unsupported bad/good combination for field {}: {:?} -> {:?}",
                                field, bad, good
                            ));
                        }
                    };
                    para.insert(&field, &new_value);
                }
            }
        }
    }

    let bugnos = transition_find_bugno(transition);

    TransitionResult::TransitionSuccess(control.source().unwrap().to_string(), bugnos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_find_expr_by_field_name_returns_none_for_non_or() {
        let expr = Expr::Bool(true);
        let result = find_expr_by_field_name(&expr, "Package");
        assert_eq!(result, None);
    }

    #[test]
    fn test_find_expr_by_field_name_finds_field_regex() {
        let expr = Expr::Or(vec![
            Box::new(Expr::FieldRegex("Package".to_string(), "foo.*".to_string())),
            Box::new(Expr::FieldString("Source".to_string(), "bar".to_string())),
        ]);
        let result = find_expr_by_field_name(&expr, "Package");
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), Expr::FieldRegex(_, _)));
    }

    #[test]
    fn test_find_expr_by_field_name_finds_field_string() {
        let expr = Expr::Or(vec![
            Box::new(Expr::FieldString("Package".to_string(), "foo".to_string())),
            Box::new(Expr::FieldString("Source".to_string(), "bar".to_string())),
        ]);
        let result = find_expr_by_field_name(&expr, "Package");
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), Expr::FieldString(_, _)));
    }

    #[test]
    fn test_find_expr_by_field_name_finds_field_comparison() {
        let expr = Expr::Or(vec![
            Box::new(Expr::FieldComparison(
                "Version".to_string(),
                Comparison::GreaterThan,
                "1.0".to_string(),
            )),
            Box::new(Expr::FieldString("Source".to_string(), "bar".to_string())),
        ]);
        let result = find_expr_by_field_name(&expr, "Version");
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), Expr::FieldComparison(_, _, _)));
    }

    #[test]
    fn test_find_expr_by_field_name_returns_none_for_missing_field() {
        let expr = Expr::Or(vec![Box::new(Expr::FieldString(
            "Package".to_string(),
            "foo".to_string(),
        ))]);
        let result = find_expr_by_field_name(&expr, "Nonexistent");
        assert_eq!(result, None);
    }

    #[test]
    fn test_match_regex_applies() {
        let m = Match::Regex(Regex::new("^foo.*").unwrap());
        assert!(m.applies("foobar"));
        assert!(!m.applies("barfoo"));
    }

    #[test]
    fn test_match_string_applies() {
        let m = Match::String("exact".to_string());
        assert!(m.applies("exact"));
        assert!(!m.applies("notexact"));
    }

    #[test]
    fn test_map_bad_to_good_error_on_non_and() {
        let bad = Expr::Bool(true);
        let good = Expr::Or(vec![]);
        let result = map_bad_to_good(&bad, &good);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "bad must be an And");
    }

    #[test]
    fn test_map_bad_to_good_error_on_non_or() {
        let bad = Expr::And(vec![]);
        let good = Expr::Bool(true);
        let result = map_bad_to_good(&bad, &good);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "good must be an Or");
    }

    #[test]
    fn test_map_bad_to_good_error_on_missing_field_in_good() {
        let bad = Expr::And(vec![Box::new(Expr::FieldString(
            "Package".to_string(),
            "oldpkg".to_string(),
        ))]);
        let good = Expr::Or(vec![Box::new(Expr::FieldString(
            "Source".to_string(),
            "newpkg".to_string(),
        ))]);
        let result = map_bad_to_good(&bad, &good);
        // The function checks extra fields first, so this returns "extra field" error
        assert_eq!(result.unwrap_err(), "extra field in good: Source");
    }

    #[test]
    fn test_map_bad_to_good_error_on_extra_field_in_good() {
        let bad = Expr::And(vec![Box::new(Expr::FieldString(
            "Package".to_string(),
            "oldpkg".to_string(),
        ))]);
        let good = Expr::Or(vec![
            Box::new(Expr::FieldString(
                "Package".to_string(),
                "newpkg".to_string(),
            )),
            Box::new(Expr::FieldString("Source".to_string(), "src".to_string())),
        ]);
        let result = map_bad_to_good(&bad, &good);
        assert_eq!(result.unwrap_err(), "extra field in good: Source");
    }

    #[test]
    fn test_map_bad_to_good_success() {
        let bad = Expr::And(vec![Box::new(Expr::FieldString(
            "Package".to_string(),
            "oldpkg".to_string(),
        ))]);
        let good = Expr::Or(vec![Box::new(Expr::FieldString(
            "Package".to_string(),
            "newpkg".to_string(),
        ))]);
        let result = map_bad_to_good(&bad, &good);
        assert!(result.is_ok());
        let map = result.unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map[0].0, "Package");
    }

    #[test]
    fn test_para_matches_field_regex_match() {
        let mut para = Paragraph::new();
        para.insert("Package", "foo-bar");
        let expr = Expr::FieldRegex("Package".to_string(), "foo.*".to_string());
        assert!(para_matches(&para, &expr));
    }

    #[test]
    fn test_para_matches_field_regex_no_match() {
        let mut para = Paragraph::new();
        para.insert("Package", "bar-foo");
        let expr = Expr::FieldRegex("Package".to_string(), "^foo".to_string());
        assert!(!para_matches(&para, &expr));
    }

    #[test]
    fn test_para_matches_field_regex_missing_field() {
        let para = Paragraph::new();
        let expr = Expr::FieldRegex("Package".to_string(), "foo.*".to_string());
        assert!(!para_matches(&para, &expr));
    }

    #[test]
    fn test_para_matches_field_string_match() {
        let mut para = Paragraph::new();
        para.insert("Package", "foo");
        let expr = Expr::FieldString("Package".to_string(), "foo".to_string());
        assert!(para_matches(&para, &expr));
    }

    #[test]
    fn test_para_matches_field_string_no_match() {
        let mut para = Paragraph::new();
        para.insert("Package", "bar");
        let expr = Expr::FieldString("Package".to_string(), "foo".to_string());
        assert!(!para_matches(&para, &expr));
    }

    #[test]
    fn test_para_matches_field_string_missing_field() {
        let para = Paragraph::new();
        let expr = Expr::FieldString("Package".to_string(), "foo".to_string());
        assert!(!para_matches(&para, &expr));
    }

    #[test]
    fn test_para_matches_not_expr() {
        let mut para = Paragraph::new();
        para.insert("Package", "foo");
        let expr = Expr::Not(Box::new(Expr::FieldString(
            "Package".to_string(),
            "bar".to_string(),
        )));
        assert!(para_matches(&para, &expr));
    }

    #[test]
    fn test_para_matches_not_expr_negated() {
        let mut para = Paragraph::new();
        para.insert("Package", "foo");
        let expr = Expr::Not(Box::new(Expr::FieldString(
            "Package".to_string(),
            "foo".to_string(),
        )));
        assert!(!para_matches(&para, &expr));
    }

    #[test]
    fn test_control_matches_bool_true() {
        let control = Deb822::new();
        let expr = Expr::Bool(true);
        assert!(control_matches(&control, &expr));
    }

    #[test]
    fn test_control_matches_bool_false() {
        let control = Deb822::new();
        let expr = Expr::Bool(false);
        assert!(!control_matches(&control, &expr));
    }

    #[test]
    fn test_control_matches_and_all_true() {
        let control_text = "Source: mypackage\nPackage: foo\n";
        let control = Deb822::from_str(control_text).unwrap();
        let expr = Expr::And(vec![
            Box::new(Expr::FieldString(
                "Source".to_string(),
                "mypackage".to_string(),
            )),
            Box::new(Expr::FieldString("Package".to_string(), "foo".to_string())),
        ]);
        assert!(control_matches(&control, &expr));
    }

    #[test]
    fn test_control_matches_and_some_false() {
        let control_text = "Source: mypackage\nPackage: foo\n";
        let control = Deb822::from_str(control_text).unwrap();
        let expr = Expr::And(vec![
            Box::new(Expr::FieldString(
                "Source".to_string(),
                "mypackage".to_string(),
            )),
            Box::new(Expr::FieldString("Package".to_string(), "bar".to_string())),
        ]);
        assert!(!control_matches(&control, &expr));
    }

    #[test]
    fn test_control_matches_or_any_true() {
        let control_text = "Source: mypackage\n";
        let control = Deb822::from_str(control_text).unwrap();
        let expr = Expr::Or(vec![
            Box::new(Expr::FieldString(
                "Source".to_string(),
                "mypackage".to_string(),
            )),
            Box::new(Expr::FieldString("Package".to_string(), "bar".to_string())),
        ]);
        assert!(control_matches(&control, &expr));
    }

    #[test]
    fn test_control_matches_or_all_false() {
        let control_text = "Source: mypackage\n";
        let control = Deb822::from_str(control_text).unwrap();
        let expr = Expr::Or(vec![
            Box::new(Expr::FieldString("Source".to_string(), "other".to_string())),
            Box::new(Expr::FieldString("Package".to_string(), "bar".to_string())),
        ]);
        assert!(!control_matches(&control, &expr));
    }

    #[test]
    fn test_transition_find_bugno_empty_notes() {
        let transition = Transition {
            title: Some("test".to_string()),
            notes: None,
            is_affected: None,
            is_good: None,
            is_bad: None,
            export: None,
        };
        let bugnos = transition_find_bugno(&transition);
        assert_eq!(bugnos, Vec::<i32>::new());
    }

    #[test]
    fn test_transition_find_bugno_single_bug() {
        let transition = Transition {
            title: Some("test".to_string()),
            notes: Some("See bug #12345 for details".to_string()),
            is_affected: None,
            is_good: None,
            is_bad: None,
            export: None,
        };
        let bugnos = transition_find_bugno(&transition);
        assert_eq!(bugnos, vec![12345]);
    }

    #[test]
    fn test_transition_find_bugno_multiple_bugs() {
        let transition = Transition {
            title: Some("test".to_string()),
            notes: Some("Fixes #123 and #456, see also #789".to_string()),
            is_affected: None,
            is_good: None,
            is_bad: None,
            export: None,
        };
        let bugnos = transition_find_bugno(&transition);
        assert_eq!(bugnos, vec![123, 456, 789]);
    }

    #[test]
    fn test_transition_find_bugno_no_bugs() {
        let transition = Transition {
            title: Some("test".to_string()),
            notes: Some("No bugs referenced here".to_string()),
            is_affected: None,
            is_good: None,
            is_bad: None,
            export: None,
        };
        let bugnos = transition_find_bugno(&transition);
        assert_eq!(bugnos, Vec::<i32>::new());
    }

    #[test]
    fn test_transition_result_is_success_true() {
        let result = TransitionResult::TransitionSuccess("pkg".to_string(), vec![]);
        assert!(result.is_success());
    }

    #[test]
    fn test_transition_result_is_success_false() {
        let result = TransitionResult::PackageNotAffected("pkg".to_string());
        assert!(!result.is_success());
    }

    #[test]
    fn test_transition_result_is_noop_true() {
        let result = TransitionResult::PackageNotAffected("pkg".to_string());
        assert!(result.is_noop());
    }

    #[test]
    fn test_transition_result_is_noop_false() {
        let result = TransitionResult::TransitionSuccess("pkg".to_string(), vec![]);
        assert!(!result.is_noop());
    }

    #[test]
    fn test_apply_transition_package_not_affected() {
        let control_text = "Source: mypackage\n";
        let mut control = Control::from_str(control_text).unwrap();
        let transition = Transition {
            title: Some("test".to_string()),
            notes: None,
            is_affected: Some(Expr::FieldString("Source".to_string(), "other".to_string())),
            is_good: None,
            is_bad: None,
            export: None,
        };
        let result = apply_transition(&mut control, &transition);
        assert!(matches!(result, TransitionResult::PackageNotAffected(_)));
    }

    #[test]
    fn test_apply_transition_package_already_good() {
        let control_text = "Source: mypackage\nPackage: newpkg\n";
        let mut control = Control::from_str(control_text).unwrap();
        let transition = Transition {
            title: Some("test".to_string()),
            notes: None,
            is_affected: None,
            is_good: Some(Expr::FieldString(
                "Package".to_string(),
                "newpkg".to_string(),
            )),
            is_bad: None,
            export: None,
        };
        let result = apply_transition(&mut control, &transition);
        assert!(matches!(result, TransitionResult::PackageAlreadyGood(_)));
    }

    #[test]
    fn test_apply_transition_package_not_bad() {
        let control_text = "Source: mypackage\nPackage: foo\n";
        let mut control = Control::from_str(control_text).unwrap();
        let transition = Transition {
            title: Some("test".to_string()),
            notes: None,
            is_affected: None,
            is_good: None,
            is_bad: Some(Expr::FieldString("Package".to_string(), "bar".to_string())),
            export: None,
        };
        let result = apply_transition(&mut control, &transition);
        assert!(matches!(result, TransitionResult::PackageNotBad(_)));
    }

    #[test]
    fn test_apply_transition_missing_is_bad() {
        let control_text = "Source: mypackage\n";
        let mut control = Control::from_str(control_text).unwrap();
        let transition = Transition {
            title: Some("test".to_string()),
            notes: None,
            is_affected: None,
            is_good: Some(Expr::Or(vec![])),
            is_bad: None,
            export: None,
        };
        let result = apply_transition(&mut control, &transition);
        assert!(matches!(result, TransitionResult::PackageNotBad(_)));
    }

    #[test]
    fn test_apply_transition_missing_is_good() {
        let control_text = "Source: mypackage\n";
        let mut control = Control::from_str(control_text).unwrap();
        let transition = Transition {
            title: Some("test".to_string()),
            notes: None,
            is_affected: None,
            is_good: None,
            is_bad: Some(Expr::And(vec![])),
            export: None,
        };
        let result = apply_transition(&mut control, &transition);
        assert!(matches!(result, TransitionResult::PackageNotBad(_)));
    }

    #[test]
    fn test_apply_transition_success() {
        let control_text = "Source: mypackage\nPackage: oldpkg\n";
        let mut control = Control::from_str(control_text).unwrap();
        let transition = Transition {
            title: Some("test".to_string()),
            notes: Some("Fixes #123".to_string()),
            is_affected: None,
            is_good: Some(Expr::Or(vec![Box::new(Expr::FieldString(
                "Package".to_string(),
                "newpkg".to_string(),
            ))])),
            is_bad: Some(Expr::And(vec![Box::new(Expr::FieldString(
                "Package".to_string(),
                "oldpkg".to_string(),
            ))])),
            export: None,
        };
        let result = apply_transition(&mut control, &transition);
        assert!(
            matches!(result, TransitionResult::TransitionSuccess(_, ref bugs) if bugs == &vec![123])
        );
    }
}
