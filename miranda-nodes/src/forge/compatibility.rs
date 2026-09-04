//! Task 4 — Pre-merge compatibility validation.
//!
//! Requirement 2.3 / design.md Property 2 (first half): a merge job only
//! reaches the Model Library if it passes compatibility validation *and*
//! the coherence smoke test. This module owns the first gate:
//! architecture-family and tokenizer matching, checked before any actual
//! merge work (mergekit invocation) is attempted, so an incompatible
//! pairing fails fast and cheaply rather than burning GPU time on a merge
//! that would only fail (or worse, silently produce garbage) later.

use thiserror::Error;

use crate::forge::job_parser::ModelRef;

/// Architecture-relevant metadata mergekit itself needs to agree across
/// all source models. Kept separate from `ModelRef` (which is about
/// *identifying* a model) since this is about *merge-compatibility*
/// facts a caller would look up from each model's config — a real
/// integration would read this from each model's `config.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct ArchitectureProfile {
    pub architecture_family: String,
    pub hidden_size: u32,
    pub num_layers: u32,
    pub tokenizer_id: String,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum IncompatibilityReason {
    #[error("'{model}' has architecture family '{found}', expected '{expected}' to match the first model")]
    ArchitectureFamilyMismatch { expected: String, found: String, model: String },
    #[error("'{model}' has hidden size {found}, expected {expected}")]
    HiddenSizeMismatch { expected: u32, found: u32, model: String },
    #[error("'{model}' has {found} layers, expected {expected}")]
    LayerCountMismatch { expected: u32, found: u32, model: String },
    #[error("'{model}' uses tokenizer '{found}', expected '{expected}' to match the first model")]
    TokenizerMismatch { expected: String, found: String, model: String },
    #[error("a merge needs at least 2 models, got {count}")]
    InsufficientModels { count: usize },
}

/// design.md: `validate_compatibility(models: &[ModelRef]) -> Result<(), IncompatibilityReason>`.
/// The design signature takes `&[ModelRef]`; this module's actual check
/// needs each model's architecture facts too, so this crate-internal
/// entry point takes the paired profiles a caller resolved from each
/// model's config. `ModelRef` is still the identity a failure message
/// refers to.
pub fn validate_compatibility(
    models: &[(ModelRef, ArchitectureProfile)],
) -> Result<(), IncompatibilityReason> {
    if models.len() < 2 {
        return Err(IncompatibilityReason::InsufficientModels { count: models.len() });
    }

    let (reference_ref, reference_profile) = &models[0];

    for (model_ref, profile) in &models[1..] {
        if profile.architecture_family != reference_profile.architecture_family {
            return Err(IncompatibilityReason::ArchitectureFamilyMismatch {
                expected: reference_profile.architecture_family.clone(),
                found: profile.architecture_family.clone(),
                model: model_ref.name.clone(),
            });
        }
        if profile.hidden_size != reference_profile.hidden_size {
            return Err(IncompatibilityReason::HiddenSizeMismatch {
                expected: reference_profile.hidden_size,
                found: profile.hidden_size,
                model: model_ref.name.clone(),
            });
        }
        if profile.num_layers != reference_profile.num_layers {
            return Err(IncompatibilityReason::LayerCountMismatch {
                expected: reference_profile.num_layers,
                found: profile.num_layers,
                model: model_ref.name.clone(),
            });
        }
        if profile.tokenizer_id != reference_profile.tokenizer_id {
            return Err(IncompatibilityReason::TokenizerMismatch {
                expected: reference_profile.tokenizer_id.clone(),
                found: profile.tokenizer_id.clone(),
                model: model_ref.name.clone(),
            });
        }
    }

    let _ = reference_ref; // identity of the reference model isn't needed once validated
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn model_ref(name: &str) -> ModelRef {
        ModelRef {
            name: name.to_string(),
            family: "GLM".to_string(),
            size: "9B".to_string(),
            local_path: Some(PathBuf::from(format!("/models/{name}"))),
            hf_repo: None,
        }
    }

    fn profile(family: &str, hidden: u32, layers: u32, tokenizer: &str) -> ArchitectureProfile {
        ArchitectureProfile {
            architecture_family: family.to_string(),
            hidden_size: hidden,
            num_layers: layers,
            tokenizer_id: tokenizer.to_string(),
        }
    }

    #[test]
    fn matching_architectures_pass_validation() {
        let models = vec![
            (model_ref("GLM-9B-A"), profile("glm", 4096, 40, "glm-tokenizer")),
            (model_ref("GLM-9B-B"), profile("glm", 4096, 40, "glm-tokenizer")),
        ];
        assert!(validate_compatibility(&models).is_ok());
    }

    #[test]
    fn mismatched_architecture_family_is_rejected() {
        let models = vec![
            (model_ref("GLM-9B"), profile("glm", 4096, 40, "glm-tokenizer")),
            (model_ref("Nemotron-9B"), profile("nemotron", 4096, 40, "glm-tokenizer")),
        ];
        let err = validate_compatibility(&models).unwrap_err();
        matches!(err, IncompatibilityReason::ArchitectureFamilyMismatch { .. });
    }

    #[test]
    fn mismatched_hidden_size_is_rejected() {
        let models = vec![
            (model_ref("GLM-9B"), profile("glm", 4096, 40, "glm-tokenizer")),
            (model_ref("GLM-14B"), profile("glm", 5120, 40, "glm-tokenizer")),
        ];
        let err = validate_compatibility(&models).unwrap_err();
        matches!(err, IncompatibilityReason::HiddenSizeMismatch { .. });
    }

    #[test]
    fn mismatched_layer_count_is_rejected() {
        let models = vec![
            (model_ref("GLM-9B"), profile("glm", 4096, 40, "glm-tokenizer")),
            (model_ref("GLM-9B-variant"), profile("glm", 4096, 48, "glm-tokenizer")),
        ];
        let err = validate_compatibility(&models).unwrap_err();
        matches!(err, IncompatibilityReason::LayerCountMismatch { .. });
    }

    #[test]
    fn mismatched_tokenizer_is_rejected() {
        let models = vec![
            (model_ref("GLM-9B"), profile("glm", 4096, 40, "glm-tokenizer")),
            (model_ref("GLM-9B-other-tok"), profile("glm", 4096, 40, "different-tokenizer")),
        ];
        let err = validate_compatibility(&models).unwrap_err();
        matches!(err, IncompatibilityReason::TokenizerMismatch { .. });
    }

    #[test]
    fn fewer_than_two_models_is_rejected() {
        let models = vec![(model_ref("GLM-9B"), profile("glm", 4096, 40, "glm-tokenizer"))];
        let err = validate_compatibility(&models).unwrap_err();
        assert_eq!(err, IncompatibilityReason::InsufficientModels { count: 1 });
    }

    #[test]
    fn empty_model_list_is_rejected() {
        let models: Vec<(ModelRef, ArchitectureProfile)> = vec![];
        let err = validate_compatibility(&models).unwrap_err();
        assert_eq!(err, IncompatibilityReason::InsufficientModels { count: 0 });
    }

    #[test]
    fn three_way_merge_with_one_incompatible_model_is_rejected() {
        let models = vec![
            (model_ref("GLM-9B-A"), profile("glm", 4096, 40, "glm-tokenizer")),
            (model_ref("GLM-9B-B"), profile("glm", 4096, 40, "glm-tokenizer")),
            (model_ref("GLM-9B-C-bad"), profile("glm", 4096, 32, "glm-tokenizer")),
        ];
        assert!(validate_compatibility(&models).is_err());
    }

    #[test]
    fn error_display_includes_the_offending_model_name() {
        let models = vec![
            (model_ref("GLM-9B"), profile("glm", 4096, 40, "glm-tokenizer")),
            (model_ref("Nemotron-9B"), profile("nemotron", 4096, 40, "glm-tokenizer")),
        ];
        let err = validate_compatibility(&models).unwrap_err();
        assert!(err.to_string().contains("Nemotron-9B"));
    }
}
