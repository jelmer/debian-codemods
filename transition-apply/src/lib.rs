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

fn compare(operator: &Comparison, value: &str, other: &str) -> bool {
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
                    (f.to_string(), Match::Regex(Regex::new(&regex).unwrap()))
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
                    Expr::FieldRegex(_, r) => Match::Regex(Regex::new(&r).unwrap()),
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
                if !used.contains(&f) {
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
            if let Some(value) = para.get(&f) {
                Regex::new(&regex).unwrap().is_match(&value)
            } else {
                return false;
            }
        }
        Expr::FieldString(f, s) => {
            if let Some(value) = para.get(&f) {
                value == *s
            } else {
                return false;
            }
        }
        Expr::FieldComparison(f, c, s) => {
            if let Some(value) = para.get(&f) {
                compare(c, &value, &s)
            } else {
                return false;
            }
        }
        Expr::Not(e) => !para_matches(para, &e),
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
    let notes = if let Some(notes) = &transition.notes {
        notes
    } else {
        return vec![];
    };
    let bugs_re = lazy_regex::regex!("#([0-9]+)");
    bugs_re
        .find_iter(&notes)
        .map(|m| m.as_str()[1..].parse().unwrap())
        .collect()
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
        match self {
            TransitionResult::TransitionSuccess(_, _) => true,
            _ => false,
        }
    }

    pub fn is_noop(&self) -> bool {
        match self {
            TransitionResult::PackageNotAffected(_) => true,
            TransitionResult::PackageAlreadyGood(_) => true,
            TransitionResult::PackageNotBad(_) => true,
            TransitionResult::TransitionSuccess(_, _) => false,
            TransitionResult::Unsupported(_) => true,
        }
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
            let old_value = if let Some(v) = para.get(&field) {
                v
            } else {
                continue;
            };
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

    let bugnos = transition_find_bugno(&transition);

    TransitionResult::TransitionSuccess(control.source().unwrap().to_string(), bugnos)
}
