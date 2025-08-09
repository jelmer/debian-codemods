use crate::{declare_fixer, FixerError, FixerResult};
use std::fs;
use std::path::Path;

// Convert CRLF line endings to LF in debian/control files
declare_fixer! {
    name: "control-file-with-CRLF-EOLs",
    tags: ["carriage-return-line-feed"],
    apply: |basedir, _package, _version, _preferences| {
        let control_path = basedir.join("debian/control");
        
        if !control_path.exists() {
            return Err(FixerError::NoChanges);
        }

        let changed = convert_line_endings(&control_path)?;
        
        if changed {
            Ok(FixerResult::builder("Format control file with unix-style line endings.")
                .fixed_tag("carriage-return-line-feed")
                .build())
        } else {
            Err(FixerError::NoChanges)
        }
    }
}

fn convert_line_endings(path: &Path) -> Result<bool, FixerError> {
    let content = fs::read_to_string(path)
        .map_err(|e| FixerError::Other(format!("Failed to read file {}: {}", path.display(), e)))?;
    
    // Check if file has CRLF line endings
    if !content.contains("\r\n") {
        return Ok(false);
    }
    
    // Convert CRLF to LF
    let converted = content.replace("\r\n", "\n");
    
    fs::write(path, converted)
        .map_err(|e| FixerError::Other(format!("Failed to write file {}: {}", path.display(), e)))?;
    
    Ok(true)
}