//! Diagnostic and action types for the detector/applier split.
//!
//! See `doc/detector-action-split.md` for the design rationale. A detector
//! returns a list of [`Diagnostic`]s, each carrying one or more
//! [`ActionPlan`]s; the driver picks a plan and applies its [`Action`]s.
//!
//! Actions are `serde`-serialisable so they can be sent over an LSP wire.

use crate::{Certainty, LintianIssue, PackageType};
use std::path::PathBuf;

pub use debian_workspace::action::{
    Action, ActionPlan, ChangelogAction, Deb822Action, DebcargoAction, Dep3Action,
    DesktopIniAction, FilesystemAction, IndentPattern, LintianOverridesAction, MaintscriptAction,
    MakefileAction, OverrideLineSelector, ParagraphSelector, RunCommandAction, SystemdAction,
    TextRange, WatchAction, YamlAction, YamlPathComponent,
};

/// A single issue found by a detector, together with the actions that would
/// fix it.
///
/// `issue` is optional: a fixer that doesn't correspond to a lintian tag
/// (declared with `tags: []`) emits diagnostics whose `issue` is `None`.
/// The driver still applies their actions, but lintian-override filtering
/// is skipped and the diagnostic does not surface in
/// `FixerResult::fixed_lintian_issues`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Diagnostic {
    /// The lintian issue this diagnostic corresponds to, if any.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub issue: Option<LintianIssue>,
    /// Human-readable summary, used for the commit message / LSP message.
    pub message: String,
    /// Certainty of the fix(es). Mirrors `FixerResult::certainty`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub certainty: Option<Certainty>,
    /// Quilt patch name used when this diagnostic's actions touch
    /// upstream files, surfacing as `FixerResult::patch_name`. The
    /// applier picks the first non-`None` value across the diagnostics
    /// it fires.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub patch_name: Option<String>,
    /// Alternative action plans that fix this diagnostic. The first plan is
    /// the default chosen by the batch driver; an LSP exposes all of them
    /// as code actions.
    pub plans: Vec<ActionPlan>,
}

impl Diagnostic {
    /// Build a diagnostic with a single default plan.
    ///
    /// * `description` — human-readable summary of *what's wrong*. Used
    ///   in the per-issue commit-message line and shown to the user.
    /// * `label` — imperative description of *what the plan would do*.
    ///   Shown in `lintian-brush --interactive` and the LSP code-action
    ///   menu. Should be written from the actor's perspective ("Set
    ///   Priority to optional.", "Trim trailing whitespace.").
    ///
    /// The two are different intents and must be written distinctly.
    pub fn with_actions(
        issue: LintianIssue,
        description: impl Into<String>,
        label: impl Into<String>,
        actions: Vec<Action>,
    ) -> Self {
        Self::with_plans(
            issue,
            description,
            vec![ActionPlan {
                label: label.into(),
                opinionated: false,
                certainty: None,
                actions,
            }],
        )
    }

    /// Build a diagnostic with caller-provided plans. Use this when the
    /// fixer offers more than one plan (e.g. a safe default plus an
    /// opinionated alternative).
    pub fn with_plans(
        issue: LintianIssue,
        message: impl Into<String>,
        plans: Vec<ActionPlan>,
    ) -> Self {
        Self {
            issue: Some(issue),
            message: message.into(),
            certainty: None,
            patch_name: None,
            plans,
        }
    }

    /// Build a diagnostic that has no associated lintian issue.
    ///
    /// Used by fixers that aren't tied to a lintian tag (their `tags: []`
    /// declaration). The driver still applies the actions but skips
    /// override / tag bookkeeping.
    /// Build a diagnostic with no associated lintian issue.
    ///
    /// `description` describes *what's wrong*; `label` is the imperative
    /// description of *what the plan would do*. See [`with_actions`] for
    /// the distinction.
    pub fn untagged(
        description: impl Into<String>,
        label: impl Into<String>,
        actions: Vec<Action>,
    ) -> Self {
        Self {
            issue: None,
            message: description.into(),
            certainty: None,
            patch_name: None,
            plans: vec![ActionPlan {
                label: label.into(),
                opinionated: false,
                certainty: None,
                actions,
            }],
        }
    }

    /// Set the certainty of this diagnostic.
    pub fn with_certainty(mut self, certainty: Certainty) -> Self {
        self.certainty = Some(certainty);
        self
    }

    /// Set the quilt patch name to use when this diagnostic's actions
    /// produce a patch.
    pub fn with_patch_name(mut self, name: impl Into<String>) -> Self {
        self.patch_name = Some(name.into());
        self
    }
}

/// Build a lintian-override [`ActionPlan`] for `issue`, or `None` if the issue
/// carries no tag.
///
/// The target file is chosen by package type:
/// - binary packages write to `debian/<package>.lintian-overrides`
/// - everything else writes to `debian/source/lintian-overrides`
pub fn override_action_plan(issue: &LintianIssue) -> Option<ActionPlan> {
    let tag = issue.tag.as_ref()?;
    let file = match issue.package_type {
        Some(PackageType::Binary) => issue
            .package
            .as_deref()
            .map(|p| PathBuf::from(format!("debian/{}.lintian-overrides", p)))
            .unwrap_or_else(|| PathBuf::from("debian/source/lintian-overrides")),
        _ => PathBuf::from("debian/source/lintian-overrides"),
    };
    Some(ActionPlan {
        label: format!("Add lintian override for {}", tag),
        opinionated: false,
        certainty: None,
        actions: vec![Action::LintianOverrides(LintianOverridesAction::AddLine {
            file,
            package: issue.package.clone(),
            tag: tag.clone(),
            info: issue.info.clone(),
        })],
    })
}

/// Append a lintian-override plan to every diagnostic in `diags` that has a
/// tagged [`LintianIssue`] but does not already carry an override plan.
pub fn add_override_plans(diags: &mut Vec<Diagnostic>) {
    for diag in diags.iter_mut() {
        if let Some(plan) = diag.issue.as_ref().and_then(override_action_plan) {
            let already_has_override = diag.plans.iter().any(|p| {
                p.actions.iter().any(|a| {
                    matches!(
                        a,
                        Action::LintianOverrides(LintianOverridesAction::AddLine { .. })
                    )
                })
            });
            if !already_has_override {
                diag.plans.push(plan);
            }
        }
    }
}
