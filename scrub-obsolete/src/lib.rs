use debian_control::relations::{Relation,Relations};
use std::path::PathBuf;

pub mod action;

pub const DEFAULT_VALUE_MULTIARCH_HINT: usize = 30;

fn note_changelog_policy(policy: bool, msg: &str) {
    lazy_static::lazy_static! {
        static ref CHANGELOG_POLICY_NOTED: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
    }
    if let Ok(mut policy_noted) = CHANGELOG_POLICY_NOTED.lock() {
        if !*policy_noted {
            let extra = if policy {
                "Specify --no-update-changelog to override."
            } else {
                "Specify --update-changelog to override."
            };
            log::info!("{} {}", msg, extra);
        }
        *policy_noted = true;
    }
}
