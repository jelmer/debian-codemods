//! Handling of transitions
//!
//! See https://release.debian.org/transitions/ for more information about transitions.
use crate::benfile::{read_benfile, Assignment, Expr};

#[derive(Debug, Default)]
/// A transition
pub struct Transition {
    /// The title of the transition
    pub title: Option<String>,

    /// Expression to check if the transition has been applied
    pub is_good: Option<Expr>,

    /// Expression to check if the transition has not been applied
    pub is_bad: Option<Expr>,

    /// Expression to check if a package is involved in the transition
    pub is_affected: Option<Expr>,

    /// Notes about the transition
    pub notes: Option<String>,

    /// Whether to export the transition
    pub export: Option<bool>,
}

impl std::convert::TryFrom<Vec<Assignment>> for Transition {
    type Error = String;

    fn try_from(value: Vec<Assignment>) -> Result<Self, Self::Error> {
        let mut transition = Transition::default();
        for assignment in value.iter() {
            match assignment.field.as_str() {
                "title" => {
                    transition.title = if let Expr::String(s) = &assignment.expr {
                        Some(s.to_string())
                    } else {
                        return Err("title must be a string".to_string());
                    }
                }
                "is_good" => {
                    transition.is_good = Some(assignment.expr.clone());
                }
                "is_bad" => {
                    transition.is_bad = Some(assignment.expr.clone());
                }
                "is_affected" => {
                    transition.is_affected = Some(assignment.expr.clone());
                }
                "notes" => {
                    transition.notes = if let Expr::String(s) = &assignment.expr {
                        Some(s.to_string())
                    } else {
                        return Err("notes must be a string".to_string());
                    }
                }
                "export" => {
                    transition.export = if let Expr::Bool(b) = assignment.expr {
                        Some(b)
                    } else {
                        return Err("export must be a boolean".to_string());
                    }
                }
                n => {
                    log::warn!("Unknown field: {}", n);
                }
            }
        }
        Ok(transition)
    }
}

/// Read a transition from a reader
pub fn read_transition<R: std::io::Read>(reader: &mut R) -> Result<Transition, String> {
    let benfile = read_benfile(reader).map_err(|e| e.to_string())?;
    Transition::try_from(benfile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_transition() {
        let input = r###"title = "libsoup2.4 -> libsoup3";
is_affected = .build-depends ~ /libsoup2.4-dev|libsoup-gnome2.4-dev|libsoup-3.0-dev/ | .build-depends-arch ~ /libsoup2.4-dev|libsoup-gnome2.4-dev|libsoup-3.0-dev/ | .build-depends ~ /gir1.2-soup-2.4|gir1.2-soup-3.0/ | .depends ~ /gir1.2-soup-2.4/;
is_good = .depends ~ /libsoup-3.0-0|gir1.2-soup-3.0/;
is_bad = .depends ~ /libsoup-2.4-1|libsoup-gnome-2.4-1|gir1.2-soup-2.4/;
notes = "https://bugs.debian.org/cgi-bin/pkgreport.cgi?users=pkg-gnome-maintainers@lists.alioth.debian.org&tag=libsoup2";
export = false;
"###;
        let transition = read_transition(&mut input.as_bytes()).unwrap();
        assert_eq!(transition.title, Some("libsoup2.4 -> libsoup3".to_string()));
        assert_eq!(
            transition.is_affected,
            Some(Expr::Or(vec![
                Box::new(Expr::FieldRegex(
                    "build-depends".to_string(),
                    "libsoup2.4-dev|libsoup-gnome2.4-dev|libsoup-3.0-dev".to_string()
                )),
                Box::new(Expr::FieldRegex(
                    "build-depends-arch".to_string(),
                    "libsoup2.4-dev|libsoup-gnome2.4-dev|libsoup-3.0-dev".to_string()
                )),
                Box::new(Expr::FieldRegex(
                    "build-depends".to_string(),
                    "gir1.2-soup-2.4|gir1.2-soup-3.0".to_string()
                )),
                Box::new(Expr::FieldRegex(
                    "depends".to_string(),
                    "gir1.2-soup-2.4".to_string()
                ))
            ]))
        );
        assert_eq!(
            transition.is_good,
            Some(Expr::FieldRegex(
                "depends".to_string(),
                "libsoup-3.0-0|gir1.2-soup-3.0".to_string()
            ))
        );
        assert_eq!(
            transition.is_bad,
            Some(Expr::FieldRegex(
                "depends".to_string(),
                "libsoup-2.4-1|libsoup-gnome-2.4-1|gir1.2-soup-2.4".to_string()
            ))
        );
        assert_eq!(transition.notes, Some("https://bugs.debian.org/cgi-bin/pkgreport.cgi?users=pkg-gnome-maintainers@lists.alioth.debian.org&tag=libsoup2".to_string()));
        assert_eq!(transition.export, Some(false));
    }
}
