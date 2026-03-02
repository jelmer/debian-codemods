use crate::{FixerError, FixerResult, LintianIssue};
use std::fs;
use std::path::Path;

const DH_MAKE_TEMPLATE: &str = r"s/.+\/v?(\d\S+)\.tar\.gz/<project>-$1\.tar\.gz/";

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let watch_path = base_path.join("debian/watch");

    if !watch_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&watch_path)?;

    let watch_file = debian_watch::parse::parse(&content)
        .map_err(|e| FixerError::Other(format!("Failed to parse watch file: {}", e)))?;

    let mut found_template = None;

    for mut entry in watch_file.entries() {
        if let Some(filenamemangle) = entry.get_option("filenamemangle") {
            if filenamemangle == DH_MAKE_TEMPLATE {
                found_template = Some(filenamemangle.to_string());

                let issue = LintianIssue::source_with_info(
                    "debian-watch-contains-dh_make-template",
                    vec![format!("{} [debian/watch]", filenamemangle)],
                );

                if !issue.should_fix(base_path) {
                    return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
                }

                entry.remove_option(debian_watch::WatchOption::Filenamemangle(String::new()));
            }
        }
    }

    let Some(template) = found_template else {
        return Err(FixerError::NoChanges);
    };

    fs::write(&watch_path, watch_file.to_string())?;

    let issue = LintianIssue::source_with_info(
        "debian-watch-contains-dh_make-template",
        vec![format!("{} [debian/watch]", template)],
    );

    Ok(
        FixerResult::builder("Remove dh_make template from debian watch.")
            .fixed_issues(vec![issue])
            .build(),
    )
}

declare_fixer! {
    name: "debian-watch-contains-dh_make-template",
    tags: ["debian-watch-contains-dh_make-template"],
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
    fn test_removes_dh_make_template() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let watch_content = "version=2\nopts=filenamemangle=s/.+\\/v?(\\d\\S+)\\.tar\\.gz/<project>-$1\\.tar\\.gz/ https://github.com/example/project/releases .*\\/v?(\\d\\S+)\\.tar\\.gz\n";
        let watch_path = debian_dir.join("watch");
        fs::write(&watch_path, watch_content).unwrap();

        let result = run(temp_dir.path()).unwrap();
        assert_eq!(
            result.description,
            "Remove dh_make template from debian watch."
        );

        let updated_content = fs::read_to_string(&watch_path).unwrap();
        assert!(!updated_content.contains("filenamemangle"));
        assert!(!updated_content.contains("<project>"));
        assert!(updated_content.contains("https://github.com"));
    }

    #[test]
    fn test_no_watch_file() {
        let temp_dir = TempDir::new().unwrap();
        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_template_pattern() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let watch_content =
            "version=4\nhttps://github.com/example/project/releases .*/v?(\\d\\S+)\\.tar\\.gz\n";
        let watch_path = debian_dir.join("watch");
        fs::write(&watch_path, watch_content).unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
