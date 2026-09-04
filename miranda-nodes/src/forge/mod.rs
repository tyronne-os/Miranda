//! WO-Model-Forge — conversational LLM customization: LoRA fine-tuning,
//! model merging, naming, GPU cost discipline, and library registration.
//!
//! Per `.kiro/specs/wo-model-forge/design.md`. This module explicitly does
//! NOT perform from-scratch pretraining (see `job_parser::detect_scope_boundary`).
//!
//! Tasks 6/7 (`finetune_pipeline`, `merge_pipeline`) implement the real
//! orchestration *shape* (job spec -> subprocess/API invocation, error
//! handling, metrics types) but have NOT been executed end-to-end against
//! real model weights in this session — no GPU/model weights were
//! available. Do not read their tests as proof of real training/merge
//! correctness; they test the orchestration logic only (argument building,
//! error propagation, divergence-abort logic) with a stubbed backend.

pub mod compatibility;
pub mod finetune_pipeline;
pub mod gpu_provisioner;
pub mod job_parser;
pub mod job_orchestrator;
pub mod merge_pipeline;
pub mod model_registry;
pub mod naming;
