use debian_control::lossless::relations::{Relation};

pub enum Action {
    /// Drop a dependency on an essential package.
    DropEssential(Relation),
    /// Drop a minimum version constraint on a package.
    DropMinimumVersion(Relation),
    /// Drop a dependency on a transitional package.
    DropTransition(Relation),
    /// Replace a dependency on a transitional package with a list of replacements.
    ReplaceTransition(Relation, Vec<Relation>),
    /// Drop a conflict with a removed package.
    DropObsoleteConflict(Relation),
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Action::DropEssential(r) => write!(f, "Drop dependency on essential package {}", r),
            Action::DropMinimumVersion(r) => write!(f, "Drop versioned constraint on {}", r),
            Action::DropTransition(r) => write!(f, "Drop dependency on transitional package {}", r),
            Action::ReplaceTransition(r, replacement) => {
                let package_names = replacement.iter().map(|p| p.name()).collect::<Vec<_>>();
                write!(
                    f,
                    "Replace dependency on transitional package {} with replacement {}",
                    r,
                    name_list(package_names.iter().map(|p| p.as_str()).collect())
                )
            }
            Action::DropObsoleteConflict(r) => write!(f, "Drop conflict with removed package {}", r),
        }
    }
}

impl serde::Serialize for Action {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Action::DropEssential(rel) => {
                let action = serde_json::json!(["drop-essential", rel.to_string()]);
                action.serialize(serializer)
            }
            Action::DropMinimumVersion(rel) => {
                let action = serde_json::json!(["drop-minimum-version", rel.to_string()]);
                action.serialize(serializer)
            }
            Action::DropTransition(rel) => {
                let action = serde_json::json!(["drop-transitional", rel.to_string()]);
                action.serialize(serializer)
            }
            Action::ReplaceTransition(rel, replacement) => {
                let action = serde_json::json!(["inline-transitional", rel.to_string(), replacement.iter().map(|x| x.to_string()).collect::<Vec<String>>()]);
                action.serialize(serializer)
            }
            Action::DropObsoleteConflict(rel) => {
                let action = serde_json::json!(["drop-obsolete-conflict", rel.to_string()]);
                action.serialize(serializer)
            }
        }
    }
}

impl<'a> serde::Deserialize<'a> for Action {
    fn deserialize<D>(deserializer: D) -> Result<Action, D::Error>
    where
        D: serde::Deserializer<'a>
    {
        let action = serde_json::Value::deserialize(deserializer)?;
        match action {
            serde_json::Value::Array(ref action) => {
                use std::str::FromStr;
                if action.len() < 2 {
                    return Err(serde::de::Error::custom("Action array must have at least two elements"));
                }
                let action_type = action[0].as_str().ok_or_else(|| serde::de::Error::custom("Action type must be a string"))?;
                match action_type {
                    "drop-essential" => {
                        let rel = Relation::from_str(action[1].as_str().ok_or_else(|| serde::de::Error::custom("Relation must be a string"))?).map_err(|e| serde::de::Error::custom(e))?;
                        Ok(Action::DropEssential(rel))
                    }
                    "drop-minimum-version" => {
                        let rel = Relation::from_str(action[1].as_str().ok_or_else(|| serde::de::Error::custom("Relation must be a string"))?).map_err(|e| serde::de::Error::custom(e))?;
                        Ok(Action::DropMinimumVersion(rel))
                    }
                    "drop-transitional" => {
                        let rel = Relation::from_str(action[1].as_str().ok_or_else(|| serde::de::Error::custom("Relation must be a string"))?).map_err(|e| serde::de::Error::custom(e))?;
                        Ok(Action::DropTransition(rel))
                    }
                    "inline-transitional" => {
                        let rel = Relation::from_str(action[1].as_str().ok_or_else(|| serde::de::Error::custom("Relation must be a string"))?).map_err(|e| serde::de::Error::custom(e))?;
                        let replacements = action[2].as_array().ok_or_else(|| serde::de::Error::custom("Replacements must be an array"))?;
                        let replacements = replacements.iter().map(|x| {
                            let s = x.as_str().ok_or_else(|| "Replacement must be a string".to_string())?;
                            let r: Relation = Relation::from_str(s).map_err(|e| e.to_string())?;
                            Ok::<Relation, String>(r)
                        }).collect::<Result<Vec<Relation>, _>>().map_err(|e| serde::de::Error::custom(e))?;
                        Ok(Action::ReplaceTransition(rel, replacements))
                    }
                    "drop-obsolete-conflict" => {
                        let rel = Relation::from_str(action[1].as_str().ok_or_else(|| serde::de::Error::custom("Relation must be a string"))?).map_err(|e| serde::de::Error::custom(e))?;
                        Ok(Action::DropObsoleteConflict(rel))
                    }
                    _ => Err(serde::de::Error::custom("Unknown action type")),
                }
            }
            _ => Err(serde::de::Error::custom("Action must be an array")),
        }
    }
}

/// Format a list of package names for use in prose.
///
/// # Arguments:
/// * `packages`: non-empty list of packages to format
///
/// # Returns
/// human-readable string
fn name_list(mut packages: Vec<&str>) -> String  {
    if packages.is_empty() {
        return "".to_string();
    }
    if packages.len() == 1 {
        return packages[0].to_string();
    }
    packages.sort();
    let last = packages.pop().unwrap();
    return packages.join(". ") + " and " + last
}
