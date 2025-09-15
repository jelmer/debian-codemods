use crate::{declare_fixer, Certainty, FixerError, FixerResult};
use deb822_lossless::Paragraph;
use debian_analyzer::control::TemplatedControlEditor;
use debian_control::lossless::relations::Relations;

declare_fixer! {
    name: "build-depends-on-build-essential",
    tags: ["build-depends-on-build-essential"],
    apply: |basedir, _package, _version, _preferences| {
        let control_path = basedir.join("debian").join("control");
        if !control_path.exists() {
            return Err(FixerError::NoChanges);
        }

        let mut editor = TemplatedControlEditor::open(&control_path)?;
        let mut changed = false;

        if let Some(mut source) = editor.source() {
            // Handle Build-Depends
            changed |= filter_build_essential_from_field(source.as_mut_deb822(), "Build-Depends");
            // Handle Build-Depends-Indep
            changed |= filter_build_essential_from_field(source.as_mut_deb822(), "Build-Depends-Indep");
        }

        if changed {
            editor.commit()?;
            Ok(FixerResult::new(
                "Drop unnecessary dependency on build-essential.".to_string(),
                Some(vec!["build-depends-on-build-essential".to_string()]),
                Some(Certainty::Certain),
                None,
                None,
                vec![],
                None,
            ))
        } else {
            Err(FixerError::NoChanges)
        }
    }
}

fn filter_build_essential_from_field(base: &mut Paragraph, field: &str) -> bool {
    let old_contents = base.get(field).unwrap_or_default();
    if old_contents.is_empty() {
        return false;
    }

    let mut relations: Relations = match old_contents.parse() {
        Ok(r) => r,
        Err(_) => return false,
    };

    let mut changed = false;

    // Remove build-essential relations from each entry
    for mut entry in relations.entries() {
        let mut to_remove = Vec::new();

        for (i, relation) in entry.relations().enumerate() {
            if relation.name() == "build-essential" {
                to_remove.push(i);
                changed = true;
            }
        }

        // Remove relations in reverse order to avoid index shifts
        for i in to_remove.into_iter().rev() {
            entry.remove_relation(i);
        }
    }

    // Remove empty entries
    let mut empty_entries = Vec::new();
    for (i, entry) in relations.entries().enumerate() {
        if entry.relations().count() == 0 {
            empty_entries.push(i);
        }
    }

    for i in empty_entries.into_iter().rev() {
        relations.remove_entry(i);
    }

    if changed {
        let new_contents = relations.to_string();
        if new_contents.trim().is_empty() || relations.is_empty() {
            base.remove(field);
        } else {
            base.set(field, &new_contents);
        }
    }

    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_removes_build_essential_from_build_depends() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Build-Depends: build-essential, debhelper-compat (= 13)

Package: test-package
Architecture: any
Depends: ${shlibs:Depends}, ${misc:Depends}
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        // Test filter_build_essential_from_field function
        let mut editor = TemplatedControlEditor::open(&control_path).unwrap();
        let mut source = editor.source().unwrap();

        let changed = filter_build_essential_from_field(source.as_mut_deb822(), "Build-Depends");
        assert!(changed);

        editor.commit().unwrap();

        // Verify the change was made
        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(!updated_content.contains("build-essential"));
        assert!(updated_content.contains("debhelper-compat (= 13)"));
    }
}
