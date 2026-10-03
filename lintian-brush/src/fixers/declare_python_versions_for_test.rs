use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction, TextRange};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use deb822_lossless::Deb822;
use debian_control::lossless::relations::Relations;
use debian_workspace::Workspace;
use lazy_regex::bytes_regex;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const DESCRIPTION: &str =
    "Test queries the requested Python versions, but X-Python3-Version is not declared.";

/// Packages that pull in all supported Python 3 versions.
const PYTHON3_ALL_PACKAGES: &[&str] = &["python3-all", "python3-all-dev", "python3-all-dbg"];

/// A `py3versions --requested` call in a test script.
#[derive(Debug, PartialEq, Eq)]
struct RequestedQuery {
    /// 1-based line number of the call.
    line: usize,
    /// The command as lintian reports it.
    command: String,
    /// Byte range of the option to rewrite and what to rewrite it to, if
    /// the call can safely be switched to `--supported`.
    replacement: Option<(TextRange, &'static str)>,
}

/// Whether the text after a py3versions call starts with a positional
/// argument, i.e. an explicit version string or control file.
fn followed_by_argument(rest: &[u8]) -> bool {
    let rest = rest.trim_ascii_start();
    // Leading digits are either an argument or the file descriptor of a
    // redirection.
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    match rest.get(digits) {
        Some(b'<' | b'>') => false,
        Some(b')' | b'`' | b';' | b'|' | b'&') | None => digits > 0,
        Some(_) => true,
    }
}

/// Find the option that selects the requested versions, and what it should
/// become to select the supported versions instead.
///
/// `options` is the option string following `py3versions`; the returned
/// range is relative to it. Returns `None` if the options are anything
/// other than a single `--requested`, optionally combined with
/// `--version`.
fn supported_replacement(options: &[u8]) -> Option<(TextRange, &'static str)> {
    let mut found = None;
    for token in bytes_regex!(r"(?-u)--?\w+").find_iter(options) {
        let to = match token.as_bytes() {
            b"-v" | b"--version" => continue,
            b"-r" => "-s",
            b"-vr" => "-vs",
            b"-rv" => "-sv",
            b"--requested" => "--supported",
            _ => return None,
        };
        if found.is_some() {
            return None;
        }
        found = Some((
            TextRange {
                start: token.start(),
                end: token.end(),
            },
            to,
        ));
    }
    found
}

/// Find the calls in a test script that lintian flags.
fn find_requested_queries(content: &[u8]) -> Vec<RequestedQuery> {
    let mut queries = Vec::new();
    let mut offset = 0;
    for (index, line) in content.split_inclusive(|&b| b == b'\n').enumerate() {
        let line_offset = offset;
        offset += line.len();

        // Like lintian, only look at the first call on a line.
        let Some(caps) = bytes_regex!(r"(?-u)(py3versions)((?:\s+--?\w+)*)").captures(line) else {
            continue;
        };
        let command = caps.get(0).unwrap();
        let options = caps.get(2).unwrap();
        if !bytes_regex!(r"(?-u)\s(?:-\w*r|--requested)").is_match(options.as_bytes()) {
            continue;
        }

        let replacement = if followed_by_argument(&line[command.end()..]) {
            None
        } else {
            supported_replacement(options.as_bytes()).map(|(range, to)| {
                let base = line_offset + options.start();
                (
                    TextRange {
                        start: base + range.start,
                        end: base + range.end,
                    },
                    to,
                )
            })
        };

        queries.push(RequestedQuery {
            line: index + 1,
            command: String::from_utf8_lossy(command.as_bytes()).into_owned(),
            replacement,
        });
    }
    queries
}

/// Whether a relations field guarantees that all supported Python 3
/// versions are installed.
fn depends_on_python3_all(relations: &Relations) -> bool {
    relations.entries().any(|entry| {
        let mut alternatives = entry.relations().peekable();
        alternatives.peek().is_some()
            && alternatives.all(|r| {
                r.try_name()
                    .is_some_and(|name| PYTHON3_ALL_PACKAGES.contains(&name.as_str()))
            })
    })
}

/// Whether the tests in a debian/tests/control stanza run with all
/// supported Python 3 versions installed.
fn stanza_depends_on_python3_all(
    depends: Option<&str>,
    source: &debian_control::lossless::Source,
) -> bool {
    let Some(depends) = depends else {
        return false;
    };
    // Entries the parser can't make sense of (such as "@") are dropped,
    // which at worst means not offering a fix.
    let (relations, _errors) = Relations::parse_relaxed(depends, true);
    if depends_on_python3_all(&relations) {
        return true;
    }
    depends.contains("@builddeps@")
        && [
            source.build_depends(),
            source.build_depends_indep(),
            source.build_depends_arch(),
        ]
        .into_iter()
        .flatten()
        .any(|relations| depends_on_python3_all(&relations))
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
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    if source.as_deb822().get("X-Python3-Version").is_some() {
        return Ok(Vec::new());
    }

    let Some(bytes) = ws.read_file(Path::new("debian/tests/control"))? else {
        return Ok(Vec::new());
    };
    let content = std::str::from_utf8(&bytes)
        .map_err(|e| FixerError::Other(format!("debian/tests/control is not UTF-8: {}", e)))?;
    let tests_control = Deb822::parse(content).tree();

    // Switching to --supported is only right if every stanza that runs a
    // test installs all supported versions; otherwise lintian would flag
    // the missing test dependency instead.
    let mut tests: BTreeMap<PathBuf, bool> = BTreeMap::new();
    for paragraph in tests_control.paragraphs() {
        let Some(names) = paragraph.get("Tests") else {
            continue;
        };
        let directory = paragraph
            .get("Tests-Directory")
            .map(|d| d.trim().to_string())
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| "debian/tests".to_string());
        let has_all = stanza_depends_on_python3_all(paragraph.get("Depends").as_deref(), &source);
        for name in names
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|n| !n.is_empty())
        {
            let path = if directory == "." {
                PathBuf::from(name)
            } else {
                Path::new(&directory).join(name)
            };
            tests
                .entry(path)
                .and_modify(|all| *all &= has_all)
                .or_insert(has_all);
        }
    }

    let mut diagnostics = Vec::new();
    for (path, has_all) in tests {
        if ws.is_dir(&path)? {
            continue;
        }
        let Some(script) = ws.read_file(&path)? else {
            continue;
        };
        for query in find_requested_queries(&script) {
            let issue = LintianIssue::source_with_info(
                "declare-python-versions-for-test",
                Visibility::Warning,
                vec![format!(
                    "{} [{}:{}]",
                    query.command,
                    path.display(),
                    query.line
                )],
            );
            let Some((range, to)) = query.replacement.filter(|_| has_all) else {
                diagnostics.push(Diagnostic::with_plans(issue, DESCRIPTION, Vec::new()));
                continue;
            };
            // Without X-Python3-Version, py3versions --requested already
            // falls back to the supported versions.
            diagnostics.push(
                Diagnostic::with_actions(
                    issue,
                    DESCRIPTION,
                    format!(
                        "Query supported rather than requested Python versions in {}.",
                        path.display()
                    ),
                    vec![Action::Filesystem(FilesystemAction::ReplaceText {
                        file: path.clone(),
                        range,
                        replacement: to.to_string(),
                    })],
                )
                .with_certainty(Certainty::Certain),
            );
        }
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "declare-python-versions-for-test",
    tags: ["declare-python-versions-for-test"],
    triggers: [
        debian_workspace::Trigger::Glob("debian/tests/*"),
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "X-Python3-Version",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends*",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test".into()),
            Some(version),
        );
        DetectorImpl.apply(&ws, &FixerPreferences::default())
    }

    fn setup(control: &str, tests_control: &str, script: &str) -> TempDir {
        let tmp = TempDir::new().unwrap();
        let tests_dir = tmp.path().join("debian/tests");
        fs::create_dir_all(&tests_dir).unwrap();
        fs::write(tmp.path().join("debian/control"), control).unwrap();
        fs::write(tests_dir.join("control"), tests_control).unwrap();
        fs::write(tests_dir.join("run"), script).unwrap();
        tmp
    }

    const CONTROL: &str = "Source: foo\n\nPackage: python3-foo\nArchitecture: all\n";
    const TESTS_CONTROL: &str = "Tests: run\nDepends: python3-all, python3-foo\n";

    fn range(start: usize, end: usize) -> TextRange {
        TextRange { start, end }
    }

    #[test]
    fn test_followed_by_argument() {
        assert!(!followed_by_argument(b""));
        assert!(!followed_by_argument(b"\n"));
        assert!(!followed_by_argument(b")\n"));
        assert!(!followed_by_argument(b"`; do\n"));
        assert!(!followed_by_argument(b" 2>/dev/null); do\n"));
        assert!(!followed_by_argument(b" >/dev/null\n"));
        assert!(!followed_by_argument(b" | wc -w\n"));
        assert!(!followed_by_argument(b"; do\n"));
        assert!(followed_by_argument(b" debian/control)\n"));
        assert!(followed_by_argument(b" '>= 3.9')\n"));
        assert!(followed_by_argument(b" 3.9\n"));
        assert!(followed_by_argument(b" 2\n"));
        assert!(followed_by_argument(b" 2"));
        assert!(followed_by_argument(b" 2)\n"));
        assert!(followed_by_argument(b"\"\n"));
    }

    #[test]
    fn test_supported_replacement() {
        assert_eq!(supported_replacement(b" -r"), Some((range(1, 3), "-s")));
        assert_eq!(
            supported_replacement(b" --requested"),
            Some((range(1, 12), "--supported"))
        );
        assert_eq!(supported_replacement(b" -vr"), Some((range(1, 4), "-vs")));
        assert_eq!(supported_replacement(b" -rv"), Some((range(1, 4), "-sv")));
        assert_eq!(supported_replacement(b" -r -v"), Some((range(1, 3), "-s")));
        assert_eq!(
            supported_replacement(b" --version  --requested"),
            Some((range(12, 23), "--supported"))
        );
        assert_eq!(supported_replacement(b" -rs"), None);
        assert_eq!(supported_replacement(b" -r -i"), None);
        assert_eq!(supported_replacement(b" -r --supported"), None);
        assert_eq!(supported_replacement(b" -r --requested"), None);
        assert_eq!(supported_replacement(b" -rr"), None);
        assert_eq!(supported_replacement(b" -v"), None);
        assert_eq!(supported_replacement(b""), None);
    }

    #[test]
    fn test_find_requested_queries() {
        let script =
            b"#!/bin/sh\nfor py in $(py3versions -r 2>/dev/null); do\n  $py -m foo\ndone\n";
        assert_eq!(
            find_requested_queries(script),
            vec![RequestedQuery {
                line: 2,
                command: "py3versions -r".to_string(),
                replacement: Some((range(34, 36), "-s")),
            }]
        );
        assert_eq!(&script[34..36], b"-r");
    }

    #[test]
    fn test_find_requested_queries_ignores_other_options() {
        assert_eq!(
            find_requested_queries(b"py3versions -s\npy3versions -i\npy3versions --version\n"),
            vec![]
        );
    }

    #[test]
    fn test_find_requested_queries_explicit_argument() {
        assert_eq!(
            find_requested_queries(b"py3versions --requested debian/control\n"),
            vec![RequestedQuery {
                line: 1,
                command: "py3versions --requested".to_string(),
                replacement: None,
            }]
        );
    }

    #[test]
    fn test_find_requested_queries_first_call_only() {
        assert_eq!(
            find_requested_queries(b"py3versions -s; py3versions -r\n"),
            vec![]
        );
    }

    #[test]
    fn test_depends_on_python3_all() {
        let check = |s: &str| depends_on_python3_all(&Relations::parse_relaxed(s, true).0);
        assert!(check("python3-all"));
        assert!(check("@, python3-all-dev:any (>= 3.9)"));
        assert!(check("foo, python3-all | python3-all-dev"));
        assert!(!check("python3-all | python3"));
        assert!(!check("python3, @"));
        assert!(!check(""));
    }

    #[test]
    fn test_replaces_short_option() {
        let tmp = setup(
            CONTROL,
            TESTS_CONTROL,
            "#!/bin/sh\nfor py in $(py3versions -r 2>/dev/null); do $py -m foo; done\n",
        );
        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Query supported rather than requested Python versions in debian/tests/run."
        );
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/tests/run")).unwrap(),
            "#!/bin/sh\nfor py in $(py3versions -s 2>/dev/null); do $py -m foo; done\n"
        );
    }

    #[test]
    fn test_replaces_several_lines() {
        let tmp = setup(
            CONTROL,
            TESTS_CONTROL,
            "#!/bin/sh\nPYS=$(py3versions --requested)\nVERS=`py3versions -vr`\n",
        );
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/tests/run")).unwrap(),
            "#!/bin/sh\nPYS=$(py3versions --supported)\nVERS=`py3versions -vs`\n"
        );
    }

    #[test]
    fn test_builddeps() {
        let tmp = setup(
            "Source: foo\nBuild-Depends: debhelper-compat (= 13), python3-all\n\nPackage: python3-foo\nArchitecture: all\n",
            "Tests: run\nDepends: @, @builddeps@\n",
            "py3versions -r\n",
        );
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/tests/run")).unwrap(),
            "py3versions -s\n"
        );
    }

    #[test]
    fn test_tests_directory() {
        let tmp = setup(
            CONTROL,
            "Tests: smoke\nTests-Directory: t\nDepends: python3-all\n",
            "py3versions -r\n",
        );
        fs::create_dir_all(tmp.path().join("t")).unwrap();
        fs::write(tmp.path().join("t/smoke"), "py3versions -r\n").unwrap();
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("t/smoke")).unwrap(),
            "py3versions -s\n"
        );
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/tests/run")).unwrap(),
            "py3versions -r\n"
        );
    }

    #[test]
    fn test_versions_declared() {
        let tmp = setup(
            "Source: foo\nX-Python3-Version: >= 3.11\n\nPackage: python3-foo\nArchitecture: all\n",
            TESTS_CONTROL,
            "py3versions -r\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_python3_all_dependency() {
        let tmp = setup(
            CONTROL,
            "Tests: run\nDepends: python3, python3-foo\n",
            "py3versions -r\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/tests/run")).unwrap(),
            "py3versions -r\n"
        );
    }

    #[test]
    fn test_stanza_without_python3_all_dependency() {
        let tmp = setup(
            CONTROL,
            "Tests: run\nDepends: python3-all\n\nTests: run\nDepends: python3\n",
            "py3versions -r\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_explicit_argument() {
        let tmp = setup(CONTROL, TESTS_CONTROL, "py3versions -r debian/control\n");
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_unlisted_script() {
        let tmp = setup(
            CONTROL,
            "Tests: other\nDepends: python3-all\n",
            "py3versions -r\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_tests_control() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("debian")).unwrap();
        fs::write(tmp.path().join("debian/control"), CONTROL).unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
