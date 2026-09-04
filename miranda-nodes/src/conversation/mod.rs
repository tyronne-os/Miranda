//! WO-Conversational-Intelligence — Miranda's adaptive conversation layer.
//!
//! Sits between raw user input and LLM inference: continuous mood
//! tracking, hierarchical conversation state, anticipatory moves, an
//! interest/curiosity model, in-session knowledge updates, persona
//! fluidity, response tuning, the autonomy calibration interview,
//! partnership investment tracking, and the final system prompt builder.
//!
//! Per `.kiro/specs/wo-conversational-intelligence/design.md`.

pub mod anticipation;
pub mod autonomy_calibration;
pub mod interest_model;
pub mod knowledge_updater;
pub mod mood_stream;
pub mod partnership_tracker;
pub mod persona_injection;
pub mod prompt_builder;
pub mod response_tuning;
pub mod state_machine;
