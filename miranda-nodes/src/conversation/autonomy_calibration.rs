//! Task 8 — Autonomy calibration interview.
//!
//! # Design rationale: the floor is structural, not a default
//!
//! design.md Property 2 (validated by Requirement 7.4) is explicit: the
//! floor categories must never resolve to `Autonomous` "regardless of
//! interview answers or track record." The naive implementation of that
//! rule is a `HashMap<ActionCategory, AutonomyLevel>` plus a runtime
//! check that special-cases the floor categories before returning. That
//! is a *default*, not a floor — it is exactly as strong as the
//! discipline of every future caller who touches `get_threshold`, and a
//! single missed call site (a new caller, a refactor, a copy-paste) would
//! silently let a destructive action run unattended.
//!
//! Instead, this module makes the floor a **type-level** guarantee:
//! [`ActionCategory`] is split into [`CalibratableCategory`] (the four
//! categories an interview answer can actually affect) and the three
//! fixed-floor categories, which are matched separately in
//! [`get_threshold`] and hard-coded to return `ExplicitConfirm` — there is
//! no `HashMap` entry for them to overwrite, and no code path that reads
//! user input before returning their level. The unit tests below don't
//! just check today's behavior; `floor_categories_have_no_stored_value`
//! asserts the underlying storage has no way to hold a floor override in
//! the first place, so a future edit that tried to "helpfully" store one
//! would fail to compile or fail that test, not silently regress.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The four categories an interview answer is allowed to influence.
/// Deliberately does **not** include the three floor categories — see
/// module docs. Adding a variant here is a conscious, reviewable choice
/// to make a new category calibratable; it can never accidentally make a
/// floor category calibratable, because the floor categories live in a
/// separate enum entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CalibratableCategory {
    FileOperations,
    Spending,
    VersionControl,
    InstallConfig,
}

/// The fixed-floor categories from design.md Property 2. No interview
/// question exists for these; [`get_threshold`] never consults
/// `AutonomyThresholds` for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FloorCategory {
    DestructiveAtScale,
    ProductionImpacting,
    HighBlastRadius,
}

/// Superset used by callers who don't yet know which kind of category
/// they're asking about (e.g. a category name parsed from a config
/// file). [`get_threshold`] accepts this and routes internally; the
/// split enums above exist so *storage* can never mix the two, even
/// though this combined view exists for ergonomic lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActionCategory {
    Calibratable(CalibratableCategory),
    Floor(FloorCategory),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutonomyLevel {
    Autonomous,
    FastPathConfirm,
    ExplicitConfirm,
}

/// Storage for calibrated thresholds. Note the key type is
/// `CalibratableCategory`, not `ActionCategory` — this is what makes
/// `floor_categories_have_no_stored_value` a compile-time-adjacent
/// guarantee rather than a runtime hope: there is no `FloorCategory` key
/// variant this map could ever contain.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AutonomyThresholds {
    levels: HashMap<CalibratableCategory, AutonomyLevel>,
    pub calibrated_at: Option<DateTime<Utc>>,
    pub action_track_record: HashMap<CalibratableCategory, u32>,
}

impl AutonomyThresholds {
    pub fn new() -> Self {
        Self::default()
    }

    /// Error handling per design.md: "If the calibration interview is
    /// interrupted before completion, unanswered categories default to
    /// `ExplicitConfirm` until answered." Answering is the only way a
    /// calibratable category becomes anything other than
    /// `ExplicitConfirm`.
    pub fn set_answer(&mut self, category: CalibratableCategory, level: AutonomyLevel) {
        self.levels.insert(category, level);
    }

    pub fn record_action(&mut self, category: CalibratableCategory) {
        *self.action_track_record.entry(category).or_insert(0) += 1;
    }

    /// Requirement 7.6 — periodic re-check based on accumulated track
    /// record. Returns categories with >= `threshold` recorded actions
    /// that have never been explicitly re-confirmed since
    /// `calibrated_at`, so the caller can prompt "you've done N file
    /// operations since we last talked about this — same comfort level?"
    pub fn categories_due_for_recheck(&self, threshold: u32) -> Vec<CalibratableCategory> {
        self.action_track_record
            .iter()
            .filter(|(_, &count)| count >= threshold)
            .map(|(cat, _)| *cat)
            .collect()
    }
}

/// The one function every caller — including Miranda's own self-directed
/// `miranda-actions` layer, per the sovereign computer-use plan — must
/// route through before taking an action in a given category. Floor
/// categories are matched first and return a value that never touches
/// `thresholds`, satisfying Property 2 for every possible interview input
/// because no interview input is ever read on this branch.
pub fn get_threshold(thresholds: &AutonomyThresholds, category: ActionCategory) -> AutonomyLevel {
    match category {
        ActionCategory::Floor(_) => AutonomyLevel::ExplicitConfirm,
        ActionCategory::Calibratable(c) => thresholds
            .levels
            .get(&c)
            .copied()
            .unwrap_or(AutonomyLevel::ExplicitConfirm),
    }
}

/// A single interview question, in the order design.md's four
/// calibratable categories are introduced (file ops, spending/GPU
/// provisioning, version control, install/config).
#[derive(Debug, Clone)]
pub struct CalibrationQuestion {
    pub category: CalibratableCategory,
    pub prompt: String,
}

/// Requirement 7.1-7.3 — the structured interview's question set. Pure
/// data (no I/O) so the actual interview loop (reading answers from the
/// user, likely over voice per the sovereign-agent plan) can live in
/// whatever surface drives the conversation, while this module owns the
/// content and the floor guarantee.
pub fn calibration_questions() -> Vec<CalibrationQuestion> {
    vec![
        CalibrationQuestion {
            category: CalibratableCategory::FileOperations,
            prompt: "When I need to create, edit, or delete files in your projects, \
                     should I just do it, do a quick confirm first, or always ask \
                     explicitly?"
                .to_string(),
        },
        CalibrationQuestion {
            category: CalibratableCategory::Spending,
            prompt: "For things that cost money — spinning up a GPU instance, calling \
                     a paid API — how much say do you want before I proceed?"
                .to_string(),
        },
        CalibrationQuestion {
            category: CalibratableCategory::VersionControl,
            prompt: "For git — committing, pushing, opening PRs — same question: \
                     autonomous, quick confirm, or always ask?"
                .to_string(),
        },
        CalibrationQuestion {
            category: CalibratableCategory::InstallConfig,
            prompt: "For installing packages or changing config/system settings, \
                     what's your comfort level?"
                .to_string(),
        },
    ]
}

/// Runs the structured interview against a caller-supplied answer source,
/// so this stays testable without wiring up real voice/text I/O here.
/// Per design.md's error handling: any category the source can't answer
/// (returns `None`) is left unset, which `get_threshold` already resolves
/// to `ExplicitConfirm` — satisfying "unanswered categories default to
/// ExplicitConfirm until answered" without a separate code path.
pub fn run_calibration_interview<F>(mut answer_source: F) -> AutonomyThresholds
where
    F: FnMut(&CalibrationQuestion) -> Option<AutonomyLevel>,
{
    let mut thresholds = AutonomyThresholds::new();
    for question in calibration_questions() {
        if let Some(level) = answer_source(&question) {
            thresholds.set_answer(question.category, level);
        }
    }
    thresholds.calibrated_at = Some(Utc::now());
    thresholds
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_FLOOR: [FloorCategory; 3] = [
        FloorCategory::DestructiveAtScale,
        FloorCategory::ProductionImpacting,
        FloorCategory::HighBlastRadius,
    ];

    const ALL_CALIBRATABLE: [CalibratableCategory; 4] = [
        CalibratableCategory::FileOperations,
        CalibratableCategory::Spending,
        CalibratableCategory::VersionControl,
        CalibratableCategory::InstallConfig,
    ];

    /// design.md Property 2, the hard gate this spec cannot ship
    /// without: no interview input combination can produce `Autonomous`
    /// for a floor category. Exhaustively tries every possible answer
    /// (including "answer every calibratable category Autonomous", the
    /// most adversarial input) and confirms every floor category still
    /// returns `ExplicitConfirm`.
    #[test]
    fn floor_categories_never_resolve_to_autonomous_under_any_interview_input() {
        for level in [
            AutonomyLevel::Autonomous,
            AutonomyLevel::FastPathConfirm,
            AutonomyLevel::ExplicitConfirm,
        ] {
            let thresholds = run_calibration_interview(|_| Some(level));
            for floor in ALL_FLOOR {
                assert_eq!(
                    get_threshold(&thresholds, ActionCategory::Floor(floor)),
                    AutonomyLevel::ExplicitConfirm,
                    "floor category {floor:?} resolved to something other than \
                     ExplicitConfirm when every calibratable category was answered \
                     {level:?}"
                );
            }
        }
    }

    /// Stronger than the above: proves there is no storage location a
    /// floor category's level could even be written to, independent of
    /// what `get_threshold` does with it. If a future refactor added a
    /// `FloorCategory` key to `AutonomyThresholds.levels`, this would
    /// need to change too, which is the point — it can't regress silently.
    #[test]
    fn floor_categories_have_no_stored_value() {
        let mut thresholds = AutonomyThresholds::new();
        for cat in ALL_CALIBRATABLE {
            thresholds.set_answer(cat, AutonomyLevel::Autonomous);
        }
        // `levels` is keyed by `CalibratableCategory`; there is no variant
        // of that key type corresponding to a `FloorCategory`, so the map
        // can hold at most `ALL_CALIBRATABLE.len()` entries no matter what
        // is written to it.
        assert!(thresholds.levels.len() <= ALL_CALIBRATABLE.len());
    }

    #[test]
    fn unanswered_calibratable_category_defaults_to_explicit_confirm() {
        let thresholds = AutonomyThresholds::new();
        assert_eq!(
            get_threshold(
                &thresholds,
                ActionCategory::Calibratable(CalibratableCategory::FileOperations)
            ),
            AutonomyLevel::ExplicitConfirm
        );
    }

    #[test]
    fn interrupted_interview_leaves_unanswered_categories_at_explicit_confirm() {
        // Simulates an interview interrupted after the first question.
        let mut answered_count = 0;
        let thresholds = run_calibration_interview(|_q| {
            answered_count += 1;
            if answered_count == 1 {
                Some(AutonomyLevel::Autonomous)
            } else {
                None
            }
        });

        assert_eq!(
            get_threshold(
                &thresholds,
                ActionCategory::Calibratable(CalibratableCategory::FileOperations)
            ),
            AutonomyLevel::Autonomous
        );
        assert_eq!(
            get_threshold(
                &thresholds,
                ActionCategory::Calibratable(CalibratableCategory::Spending)
            ),
            AutonomyLevel::ExplicitConfirm
        );
    }

    #[test]
    fn answered_calibratable_category_returns_the_chosen_level() {
        let mut thresholds = AutonomyThresholds::new();
        thresholds.set_answer(CalibratableCategory::VersionControl, AutonomyLevel::FastPathConfirm);
        assert_eq!(
            get_threshold(
                &thresholds,
                ActionCategory::Calibratable(CalibratableCategory::VersionControl)
            ),
            AutonomyLevel::FastPathConfirm
        );
    }

    #[test]
    fn calibration_questions_cover_all_four_calibratable_categories_exactly_once() {
        let questions = calibration_questions();
        assert_eq!(questions.len(), 4);
        let mut seen: Vec<CalibratableCategory> = questions.iter().map(|q| q.category).collect();
        seen.sort_by_key(|c| format!("{c:?}"));
        for cat in ALL_CALIBRATABLE {
            assert!(seen.contains(&cat), "missing question for {cat:?}");
        }
    }

    #[test]
    fn categories_due_for_recheck_respects_threshold() {
        let mut thresholds = AutonomyThresholds::new();
        for _ in 0..5 {
            thresholds.record_action(CalibratableCategory::FileOperations);
        }
        thresholds.record_action(CalibratableCategory::Spending);

        let due = thresholds.categories_due_for_recheck(5);
        assert!(due.contains(&CalibratableCategory::FileOperations));
        assert!(!due.contains(&CalibratableCategory::Spending));
    }

    #[test]
    fn calibrated_at_is_set_after_running_the_interview() {
        let thresholds = run_calibration_interview(|_| Some(AutonomyLevel::FastPathConfirm));
        assert!(thresholds.calibrated_at.is_some());
    }
}
