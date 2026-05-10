use chrono::Local;
use distro_info::{DebianDistroInfo, DistroInfo};

#[derive(Debug, PartialEq)]
pub struct Annotation {
    /// Optional marker name (e.g. for use on the command line to selectively apply)
    pub marker: Option<String>,
    /// The expression to evaluate
    pub expr: Expr,
}

#[derive(Debug, PartialEq)]
pub enum Expr {
    /// True when the named Debian release has been released
    ReleasedDebianCodename(String),
}

#[derive(Debug, PartialEq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid remove-after annotation: {}", self.0)
    }
}

/// Parse the content that follows "remove-after:" (or "begin-remove-after:").
///
/// Grammar (liberal):
///   annotation = [marker SP+] expr
///   marker     = word  (no spaces/commas, not "after")
///   expr       = codename
pub fn parse_annotation(s: &str) -> Result<Annotation, ParseError> {
    let s = s.trim();
    if s.is_empty() {
        return Err(ParseError("empty annotation".to_string()));
    }

    // Collect words, stopping at the first comment-start '#' that is not part of a word
    let tokens: Vec<&str> = s
        .split_whitespace()
        .take_while(|t| !t.starts_with('#'))
        .collect();

    if tokens.is_empty() {
        return Err(ParseError("empty annotation".to_string()));
    }

    // Determine if the first token is a marker or the start of the expression.
    // A marker cannot be "after" and cannot contain commas.
    let (marker, expr_tokens) =
        if tokens.len() >= 2 && tokens[0] != "after" && !tokens[0].contains(',') {
            (
                Some(tokens[0].trim_end_matches(',').to_string()),
                &tokens[1..],
            )
        } else {
            (None, tokens.as_slice())
        };

    let expr = parse_expr(expr_tokens)?;
    Ok(Annotation { marker, expr })
}

fn parse_expr(tokens: &[&str]) -> Result<Expr, ParseError> {
    // Skip leading "after" keyword if present
    let tokens = if tokens.first() == Some(&"after") {
        &tokens[1..]
    } else {
        tokens
    };

    // Strip trailing non-word tokens (trailing comments that slipped through)
    let tokens: Vec<&str> = tokens
        .iter()
        .take_while(|t| !t.starts_with('#'))
        .copied()
        .collect();

    if tokens.is_empty() {
        return Err(ParseError("missing expression".to_string()));
    }

    if tokens.len() > 1 {
        return Err(ParseError(format!(
            "unexpected tokens in expression: {}",
            tokens[1..].join(" ")
        )));
    }

    let word = tokens[0];
    // Strip a leading "released:" prefix if present
    let codename = word.strip_prefix("released:").unwrap_or(word);

    if codename.is_empty() {
        return Err(ParseError("empty codename".to_string()));
    }

    // Validate it looks like a Debian codename (lowercase alpha only)
    if !codename.chars().all(|c| c.is_ascii_lowercase()) {
        return Err(ParseError(format!("unknown expression: {}", word)));
    }

    Ok(Expr::ReleasedDebianCodename(codename.to_string()))
}

/// Evaluate whether an expression is currently true.
pub fn eval_expr(expr: &Expr) -> bool {
    match expr {
        Expr::ReleasedDebianCodename(codename) => {
            let Ok(info) = DebianDistroInfo::new() else {
                return false;
            };
            let today = Local::now().date_naive();
            info.released(today)
                .iter()
                .any(|r| r.series() == codename || r.codename().to_lowercase() == *codename)
        }
    }
}

/// Find the "remove-after" annotation payload in a shell comment line, if any.
///
/// Returns the slice after "remove-after:" if found.
fn find_inline_annotation(line: &str) -> Option<&str> {
    // A line may contain multiple '#'-introduced comment fragments.
    // We scan for "remove-after:" in any of them.
    let mut rest = line;
    while let Some(pos) = rest.find('#') {
        let comment = &rest[pos + 1..].trim_start();
        if let Some(tail) = comment.strip_prefix("remove-after:") {
            return Some(tail);
        }
        rest = &rest[pos + 1..];
    }
    None
}

/// Find the annotation payload for a "begin-remove-after:" block opener.
fn find_block_open(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let comment = trimmed.strip_prefix('#')?.trim_start();
    comment.strip_prefix("begin-remove-after:")
}

/// Return true if the line is a "# end-remove-after" marker.
fn is_block_close(line: &str) -> bool {
    let trimmed = line.trim();
    let Some(comment) = trimmed.strip_prefix('#') else {
        return false;
    };
    comment.trim() == "end-remove-after"
}

#[derive(Debug, PartialEq)]
pub enum AnnotationError {
    ParseError(ParseError),
    UnbalancedBlockClose { line: usize },
    UnclosedBlock { open_line: usize },
}

impl std::fmt::Display for AnnotationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnnotationError::ParseError(e) => write!(f, "{}", e),
            AnnotationError::UnbalancedBlockClose { line } => {
                write!(f, "unbalanced # end-remove-after at line {}", line)
            }
            AnnotationError::UnclosedBlock { open_line } => {
                write!(
                    f,
                    "# begin-remove-after at line {} has no matching end",
                    open_line
                )
            }
        }
    }
}

/// Process a shell script's lines, removing any lines/blocks where the
/// remove-after annotation evaluates to true.
///
/// Returns `Ok(lines)` with the filtered content, or `Err(errors)` listing
/// all annotation problems found (in which case nothing is removed).
pub fn process_shell(lines: &[&str]) -> Result<Vec<String>, Vec<AnnotationError>> {
    // First pass: collect all annotations and check for parse errors.
    // We must verify every annotation before removing anything.

    #[derive(Debug)]
    enum Directive {
        InlineLine {
            line_idx: usize,
            annotation: Annotation,
        },
        Block {
            start: usize,
            end: usize,
            annotation: Annotation,
        },
    }

    let mut directives: Vec<Directive> = vec![];
    let mut errors: Vec<AnnotationError> = vec![];

    // Stack of (open_line_idx, annotation) for nested blocks
    let mut block_stack: Vec<(usize, Annotation)> = vec![];

    for (i, line) in lines.iter().enumerate() {
        if let Some(payload) = find_block_open(line) {
            match parse_annotation(payload) {
                Ok(ann) => block_stack.push((i, ann)),
                Err(e) => errors.push(AnnotationError::ParseError(e)),
            }
        } else if is_block_close(line) {
            if let Some((open_idx, ann)) = block_stack.pop() {
                // The block spans open_idx..=i (inclusive).
                directives.push(Directive::Block {
                    start: open_idx,
                    end: i,
                    annotation: ann,
                });
            } else {
                errors.push(AnnotationError::UnbalancedBlockClose { line: i + 1 });
            }
        } else if let Some(payload) = find_inline_annotation(line) {
            match parse_annotation(payload) {
                Ok(ann) => directives.push(Directive::InlineLine {
                    line_idx: i,
                    annotation: ann,
                }),
                Err(e) => errors.push(AnnotationError::ParseError(e)),
            }
        }
    }

    for (open_idx, _) in block_stack {
        errors.push(AnnotationError::UnclosedBlock {
            open_line: open_idx + 1,
        });
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    // Second pass: determine which line indices to drop.
    let mut drop = vec![false; lines.len()];

    for directive in &directives {
        match directive {
            Directive::InlineLine {
                line_idx,
                annotation,
            } => {
                if eval_expr(&annotation.expr) {
                    drop[*line_idx] = true;
                }
            }
            Directive::Block {
                start,
                end,
                annotation,
            } => {
                if eval_expr(&annotation.expr) {
                    drop[*start..=*end].fill(true);
                }
            }
        }
    }

    Ok(lines
        .iter()
        .enumerate()
        .filter(|(i, _)| !drop[*i])
        .map(|(_, l)| l.to_string())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    mod test_parse_annotation {
        use super::*;

        #[test]
        fn test_simple_codename() {
            assert_eq!(
                parse_annotation("trixie"),
                Ok(Annotation {
                    marker: None,
                    expr: Expr::ReleasedDebianCodename("trixie".to_string()),
                })
            );
        }

        #[test]
        fn test_released_prefix() {
            assert_eq!(
                parse_annotation("released:trixie"),
                Ok(Annotation {
                    marker: None,
                    expr: Expr::ReleasedDebianCodename("trixie".to_string()),
                })
            );
        }

        #[test]
        fn test_after_keyword() {
            assert_eq!(
                parse_annotation("after trixie"),
                Ok(Annotation {
                    marker: None,
                    expr: Expr::ReleasedDebianCodename("trixie".to_string()),
                })
            );
        }

        #[test]
        fn test_marker() {
            assert_eq!(
                parse_annotation("myfixup trixie"),
                Ok(Annotation {
                    marker: Some("myfixup".to_string()),
                    expr: Expr::ReleasedDebianCodename("trixie".to_string()),
                })
            );
        }

        #[test]
        fn test_marker_with_after() {
            assert_eq!(
                parse_annotation("myfixup after trixie"),
                Ok(Annotation {
                    marker: Some("myfixup".to_string()),
                    expr: Expr::ReleasedDebianCodename("trixie".to_string()),
                })
            );
        }

        #[test]
        fn test_trailing_comment() {
            assert_eq!(
                parse_annotation("trixie # some note"),
                Ok(Annotation {
                    marker: None,
                    expr: Expr::ReleasedDebianCodename("trixie".to_string()),
                })
            );
        }

        #[test]
        fn test_marker_with_trailing_comment() {
            assert_eq!(
                parse_annotation("myfixup trixie # some note"),
                Ok(Annotation {
                    marker: Some("myfixup".to_string()),
                    expr: Expr::ReleasedDebianCodename("trixie".to_string()),
                })
            );
        }

        #[test]
        fn test_whitespace_only() {
            assert_eq!(
                parse_annotation("   "),
                Err(ParseError("empty annotation".to_string()))
            );
        }

        #[test]
        fn test_empty() {
            assert_eq!(
                parse_annotation(""),
                Err(ParseError("empty annotation".to_string()))
            );
        }

        #[test]
        fn test_only_comment() {
            assert_eq!(
                parse_annotation("# some note"),
                Err(ParseError("empty annotation".to_string()))
            );
        }

        #[test]
        fn test_non_alpha_codename_rejected() {
            // Contains digits — not a valid codename
            assert!(parse_annotation("release123").is_err());
        }

        #[test]
        fn test_two_codenames_parsed_as_marker_and_expr() {
            // The first word is treated as a marker; the second as the expression.
            assert_eq!(
                parse_annotation("trixie bookworm"),
                Ok(Annotation {
                    marker: Some("trixie".to_string()),
                    expr: Expr::ReleasedDebianCodename("bookworm".to_string()),
                })
            );
        }

        #[test]
        fn test_three_words_rejected() {
            assert!(parse_annotation("marker trixie bookworm").is_err());
        }

        #[test]
        fn test_after_is_not_a_marker() {
            // "after" must not be treated as a marker name even when followed by two tokens
            assert_eq!(
                parse_annotation("after trixie"),
                Ok(Annotation {
                    marker: None,
                    expr: Expr::ReleasedDebianCodename("trixie".to_string()),
                })
            );
        }
    }

    mod test_find_inline_annotation {
        use super::*;

        #[test]
        fn test_after_code() {
            assert_eq!(
                find_inline_annotation("blah  # remove-after: trixie"),
                Some(" trixie")
            );
        }

        #[test]
        fn test_comment_before() {
            assert!(find_inline_annotation(
                "blah  # Trixie comes with blah built in # remove-after: trixie"
            )
            .is_some());
        }

        #[test]
        fn test_no_annotation() {
            assert_eq!(find_inline_annotation("blah  # just a comment"), None);
        }

        #[test]
        fn test_standalone_annotation() {
            assert!(find_inline_annotation("# remove-after: trixie").is_some());
        }

        #[test]
        fn test_no_hash() {
            assert_eq!(find_inline_annotation("echo hello"), None);
        }
    }

    mod test_find_block_open {
        use super::*;

        #[test]
        fn test_basic() {
            assert_eq!(
                find_block_open("# begin-remove-after: trixie"),
                Some(" trixie")
            );
        }

        #[test]
        fn test_with_leading_whitespace() {
            assert_eq!(
                find_block_open("  # begin-remove-after: trixie"),
                Some(" trixie")
            );
        }

        #[test]
        fn test_not_a_block_open() {
            assert_eq!(find_block_open("# remove-after: trixie"), None);
        }

        #[test]
        fn test_not_a_comment() {
            assert_eq!(find_block_open("begin-remove-after: trixie"), None);
        }
    }

    mod test_is_block_close {
        use super::*;

        #[test]
        fn test_basic() {
            assert!(is_block_close("# end-remove-after"));
        }

        #[test]
        fn test_with_leading_whitespace() {
            assert!(is_block_close("  # end-remove-after"));
        }

        #[test]
        fn test_not_a_close() {
            assert!(!is_block_close("# begin-remove-after: trixie"));
        }

        #[test]
        fn test_not_a_comment() {
            assert!(!is_block_close("end-remove-after"));
        }

        #[test]
        fn test_has_trailing_text() {
            assert!(!is_block_close("# end-remove-after trixie"));
        }
    }

    mod test_eval_expr {
        use super::*;

        #[test]
        fn test_ancient_release_is_true() {
            // buzz was released in 1996 — always true
            assert!(eval_expr(&Expr::ReleasedDebianCodename("buzz".to_string())));
        }

        #[test]
        fn test_unknown_codename_is_false() {
            assert!(!eval_expr(&Expr::ReleasedDebianCodename(
                "fakefuture".to_string()
            )));
        }
    }

    mod test_process_shell {
        use super::*;

        #[test]
        fn test_no_annotations() {
            let lines = vec!["echo hello", "echo world"];
            let result = process_shell(&lines).unwrap();
            assert_eq!(result, vec!["echo hello", "echo world"]);
        }

        #[test]
        fn test_inline_released() {
            // buzz is definitely released
            let lines = vec!["echo old  # remove-after: buzz", "echo keep"];
            let result = process_shell(&lines).unwrap();
            assert_eq!(result, vec!["echo keep"]);
        }

        #[test]
        fn test_inline_not_yet_released() {
            let lines = vec!["echo future  # remove-after: fakefuture", "echo keep"];
            let result = process_shell(&lines).unwrap();
            assert_eq!(
                result,
                vec!["echo future  # remove-after: fakefuture", "echo keep"]
            );
        }

        #[test]
        fn test_block_released() {
            let lines = vec![
                "# begin-remove-after: buzz",
                "alternatives --add foo bar",
                "alternatives --add foo bar1",
                "# end-remove-after",
                "echo keep",
            ];
            let result = process_shell(&lines).unwrap();
            assert_eq!(result, vec!["echo keep"]);
        }

        #[test]
        fn test_block_not_released() {
            let lines = vec![
                "# begin-remove-after: fakefuture",
                "alternatives --add foo bar",
                "# end-remove-after",
                "echo keep",
            ];
            let result = process_shell(&lines).unwrap();
            assert_eq!(
                result,
                vec![
                    "# begin-remove-after: fakefuture",
                    "alternatives --add foo bar",
                    "# end-remove-after",
                    "echo keep",
                ]
            );
        }

        #[test]
        fn test_parse_error_prevents_all_removals() {
            // The valid "buzz" removal must NOT happen when another annotation is invalid.
            let lines = vec![
                "echo old  # remove-after: buzz",
                "echo bad  # remove-after: !!!invalid",
                "echo keep",
            ];
            let result = process_shell(&lines);
            assert!(result.is_err());
        }

        #[test]
        fn test_multiple_parse_errors_all_reported() {
            let lines = vec![
                "echo a  # remove-after: !!!one",
                "echo b  # remove-after: !!!two",
            ];
            let errs = process_shell(&lines).unwrap_err();
            assert_eq!(errs.len(), 2);
        }

        #[test]
        fn test_unbalanced_end() {
            let lines = vec!["# end-remove-after", "echo keep"];
            let errs = process_shell(&lines).unwrap_err();
            assert_eq!(
                errs,
                vec![AnnotationError::UnbalancedBlockClose { line: 1 }]
            );
        }

        #[test]
        fn test_unclosed_block() {
            let lines = vec!["# begin-remove-after: buzz", "echo content"];
            let errs = process_shell(&lines).unwrap_err();
            assert_eq!(errs, vec![AnnotationError::UnclosedBlock { open_line: 1 }]);
        }

        #[test]
        fn test_nested_blocks() {
            let lines = vec![
                "# begin-remove-after: buzz",
                "outer",
                "# begin-remove-after: buzz",
                "inner",
                "# end-remove-after",
                "outer2",
                "# end-remove-after",
                "keep",
            ];
            let result = process_shell(&lines).unwrap();
            assert_eq!(result, vec!["keep"]);
        }

        #[test]
        fn test_inline_comment_variants() {
            // All three variants from the spec
            let lines = vec![
                "blah  # remove-after: buzz # Buzz comes with blah built in",
                "blah2  # remove-after: buzz",
                "blah3  # Buzz comes with blah built in # remove-after: buzz",
            ];
            let result = process_shell(&lines).unwrap();
            assert_eq!(result, Vec::<String>::new());
        }

        #[test]
        fn test_marker_does_not_affect_removal() {
            // A marker name should not prevent removal when the expression is true.
            let lines = vec!["echo old  # remove-after: myfixup buzz", "echo keep"];
            let result = process_shell(&lines).unwrap();
            assert_eq!(result, vec!["echo keep"]);
        }

        #[test]
        fn test_empty_input() {
            let result = process_shell(&[]).unwrap();
            assert_eq!(result, Vec::<String>::new());
        }
    }
}
