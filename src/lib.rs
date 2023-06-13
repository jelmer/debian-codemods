#[derive(Clone, PartialEq, Eq, Debug, Default, PartialOrd, Ord)]
pub enum Certainty {
    #[default]
    Certain,
    Confident,
    Likely,
    Possible,
}

impl TryFrom<&str> for Certainty {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "certain" => Ok(Certainty::Certain),
            "confident" => Ok(Certainty::Confident),
            "likely" => Ok(Certainty::Likely),
            "possible" => Ok(Certainty::Possible),
            _ => Err(format!("Invalid certainty: {}", value)),
        }
    }
}

impl ToString for Certainty {
    fn to_string(&self) -> String {
        match self {
            Certainty::Certain => "certain".to_string(),
            Certainty::Confident => "confident".to_string(),
            Certainty::Likely => "likely".to_string(),
            Certainty::Possible => "possible".to_string(),
        }
    }
}

// TODO(jelmer): Use breezy::RevisionId instead
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RevisionId(Vec<u8>);

impl RevisionId {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl From<Vec<u8>> for RevisionId {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PackageType {
    Source,
    Binary,
}

impl ToString for PackageType {
    fn to_string(&self) -> String {
        match self {
            PackageType::Source => "source".to_string(),
            PackageType::Binary => "binary".to_string(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LintianIssue {
    pub package: Option<String>,
    pub package_type: Option<PackageType>,
    pub tag: Option<String>,
    pub info: Option<Vec<String>>,
}

impl LintianIssue {
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "package": self.package,
            "package_type": self.package_type.as_ref().map(|t| t.to_string()),
            "tag": self.tag,
            "info": self.info,
        })
    }

    pub fn just_tag(tag: String) -> Self {
        Self {
            package: None,
            package_type: None,
            tag: Some(tag),
            info: None,
        }
    }
}

impl TryFrom<&str> for LintianIssue {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let value = value.trim();
        let package_type;
        let package;
        let after = if let Some((before, after)) = value.split_once(':') {
            if let Some((package_type_str, package_str)) = before.trim().split_once(' ') {
                package_type = Some(match package_type_str {
                    "source" => PackageType::Source,
                    "binary" => PackageType::Binary,
                    _ => {
                        return Err(format!("Invalid package type: {}", package_type_str));
                    }
                });
                package = Some(package_str.to_string());
            } else {
                package_type = None;
                package = Some(before.to_string());
            }
            after
        } else {
            package_type = None;
            package = None;
            value
        };
        let mut parts = after.trim().split(' ');
        let tag = parts.next().map(|s| s.to_string());
        let info: Vec<_> = parts.map(|s| s.to_string()).collect();
        let info = if info.is_empty() { None } else { Some(info) };
        Ok(Self {
            package,
            package_type,
            tag,
            info,
        })
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FixerResult {
    pub description: String,
    pub certainty: Option<Certainty>,
    pub patch_name: Option<String>,
    pub revision_id: Option<RevisionId>,
    pub fixed_lintian_issues: Vec<LintianIssue>,
    pub overridden_lintian_issues: Vec<LintianIssue>,
}

impl FixerResult {
    pub fn new(
        description: String,
        fixed_lintian_tags: Option<Vec<String>>,
        certainty: Option<Certainty>,
        patch_name: Option<String>,
        revision_id: Option<RevisionId>,
        mut fixed_lintian_issues: Vec<LintianIssue>,
        overridden_lintian_issues: Option<Vec<LintianIssue>>,
    ) -> Self {
        if let Some(fixed_lintian_tags) = fixed_lintian_tags.as_ref() {
            fixed_lintian_issues.extend(
                fixed_lintian_tags
                    .iter()
                    .map(|tag| LintianIssue::just_tag(tag.to_string())),
            );
        }
        Self {
            description,
            certainty,
            patch_name,
            revision_id,
            fixed_lintian_issues,
            overridden_lintian_issues: overridden_lintian_issues.unwrap_or(vec![]),
        }
    }
    pub fn fixed_lintian_tags(&self) -> Vec<&str> {
        self.fixed_lintian_issues
            .iter()
            .filter_map(|issue| issue.tag.as_deref())
            .collect()
    }
}
