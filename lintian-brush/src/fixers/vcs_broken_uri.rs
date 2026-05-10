use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use debian_workspace::Workspace;
use crate::{FixerError, FixerPreferences};
use std::path::PathBuf;

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
    let Some(vcs_git) = source.get("Vcs-Git") else {
        return Ok(Vec::new());
    };

    let fixed = crate::vcs::fixup_broken_git_url(&vcs_git);
    if fixed == vcs_git {
        return Ok(Vec::new());
    }

    // This fixer isn't associated with a lintian tag, so emit an untagged
    // diagnostic. Override and tag bookkeeping skip it, but the action
    // still applies.
    Ok(vec![Diagnostic::untagged(
        "Vcs-Git URL is broken.",
        "Fix broken Vcs URL.",
        vec![Action::Deb822(Deb822Action::SetField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Vcs-Git".into(),
            value: fixed,
        })],
    )])
}

declare_detector! {
    name: "vcs-broken-uri",
    tags: [],
    // Must fix broken URIs after infrastructure updates and before type mismatch checks
    after: ["vcs-field-bitrotted"],
    before: ["vcs-field-mismatch"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Vcs-Git",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_fix_broken_git_url_extra_colon() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: blah\nVcs-Git: https://github.com:jelmer/dulwich\n",
        )
        .unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(result.description, "Fix broken Vcs URL.");

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: blah\nVcs-Git: https://github.com/jelmer/dulwich\n",
        );
    }

    #[test]
    fn test_fix_git_to_https() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test\nVcs-Git: git://github.com/jelmer/dulwich\n",
        )
        .unwrap();

        run_apply(base_path).unwrap();

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test\nVcs-Git: https://github.com/jelmer/dulwich\n",
        );
    }

    #[test]
    fn test_no_change_when_url_already_correct() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: blah\nVcs-Git: https://github.com/jelmer/dulwich\n",
        )
        .unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_vcs_field() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: blah\n").unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        assert!(run_apply(temp_dir.path()).is_err());
    }

    #[test]
    fn test_fix_salsa_cgit_url() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test\nVcs-Git: https://salsa.debian.org/cgit/jelmer/dulwich\n",
        )
        .unwrap();

        run_apply(base_path).unwrap();

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test\nVcs-Git: https://salsa.debian.org/jelmer/dulwich\n",
        );
    }

    #[test]
    fn test_fix_strip_username() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test\nVcs-Git: git://git@github.com:RPi-Distro/pgzero.git\n",
        )
        .unwrap();

        run_apply(base_path).unwrap();

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test\nVcs-Git: https://github.com/RPi-Distro/pgzero.git\n",
        );
    }

    #[test]
    fn test_fix_freedesktop_anongit() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test\nVcs-Git: git://anongit.freedesktop.org/xorg/xserver\n",
        )
        .unwrap();

        run_apply(base_path).unwrap();

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test\nVcs-Git: https://gitlab.freedesktop.org/xorg/xserver\n",
        );
    }
}
