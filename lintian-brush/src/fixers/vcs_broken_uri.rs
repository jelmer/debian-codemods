use crate::{declare_fixer, FixerError, FixerResult};
use debian_analyzer::abstract_control::AbstractSource;
use debian_analyzer::control::TemplatedControlEditor;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");
    let editor = TemplatedControlEditor::open(&control_path)?;

    let mut made_changes = false;

    if let Some(mut source) = editor.source() {
        if let Some(vcs_git) = source.get_vcs_url("Git") {
            let fixed = crate::vcs::fixup_broken_git_url(&vcs_git);
            if fixed != vcs_git {
                source.set_vcs_url("Git", &fixed);
                made_changes = true;
            }
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    Ok(FixerResult::builder("Fix broken Vcs URL.").build())
}

declare_fixer! {
    name: "vcs-broken-uri",
    tags: [],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

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

        let result = run(base_path);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert_eq!(result.description, "Fix broken Vcs URL.");

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert_eq!(
            updated_content,
            "Source: blah\nVcs-Git: https://github.com/jelmer/dulwich\n"
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

        let result = run(base_path);
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert_eq!(
            updated_content,
            "Source: test\nVcs-Git: https://github.com/jelmer/dulwich\n"
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

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_vcs_field() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: blah\n").unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(result.is_err());
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

        let result = run(base_path);
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert_eq!(
            updated_content,
            "Source: test\nVcs-Git: https://salsa.debian.org/jelmer/dulwich\n"
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

        let result = run(base_path);
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert_eq!(
            updated_content,
            "Source: test\nVcs-Git: https://github.com/RPi-Distro/pgzero.git\n"
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

        let result = run(base_path);
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert_eq!(
            updated_content,
            "Source: test\nVcs-Git: https://gitlab.freedesktop.org/xorg/xserver\n"
        );
    }
}
