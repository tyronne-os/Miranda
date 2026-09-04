# Design Document

## Overview

Model Forge lets the user customize any downloaded base LLM to their exact use case — via LoRA fine-tuning with custom instructions/weights, or via merging multiple models into a hybrid — triggered entirely through natural conversation with Miranda (voice or text). Every output model is automatically renamed per the project convention and immediately available in the composer's model menu. This system explicitly customizes existing pretrained models; it does not perform from-scratch pretraining.

## Architecture

```
User (voice/text): "fine-tune GLM-9B with these instructions..."
      │
      ▼
Conversational Intelligence Layer (WO-Conversational-Intelligence)
  routes Model Forge intent ──► Job Parser
      │
      ▼
Job Parser ──► JobSpec { base_model, method, instructions/models_to_merge, estimated_cost }
      │
      ▼
Autonomy Calibration Check (WO-Conversational-Intelligence Req 7)
  Spending/GPU category threshold ──► confirm or proceed
      │
      ▼
GPU Provisioner ──► spins up instance (if needed), enforces 15-min idle auto-stop
      │
      ├──► Fine-Tune Pipeline (LoRA via peft/axolotl)
      │        base_model + instructions → adapted_model
      │
      └──► Merge Pipeline (mergekit: SLERP/TIES/DARE)
               model_a + model_b (+...) → merged_model
      │
      ▼
Coherence Smoke Test (sample generation, basic sanity check)
      │
      ▼
Naming Engine ──► "<Female Name> <Family>-<Size> <Descriptor>"
      │
      ▼
Model Library Registry ──► composer/menu UI updated
      │
      ▼
Miranda (conversational): announces completion + new model name
```

## Components and Interfaces

### Job Parser
- Interface: `parse_forge_intent(message: &str) -> Option<JobSpec>`
- `enum JobMethod { LoraFineTune, Merge(MergeMethod) }`
- `enum MergeMethod { Slerp, Ties, Dare }`
- Detects intent via the conversational intelligence routing layer before falling through to normal chat handling.

### GPU Provisioner
- Interface: `provision_gpu(estimated_duration: Duration, estimated_cost: f32) -> Result<GpuHandle, ProvisionError>`
- Enforces idle auto-stop timer (15 min), consistent with existing `aws-pipeline-architect` cost-discipline rules.
- Interface: `teardown(handle: GpuHandle)` called on completion, failure, or cancellation.

### Fine-Tune Pipeline
- Interface: `run_lora_finetune(base_model: &ModelRef, instructions: &TrainingSpec) -> Result<AdaptedModel, TrainError>`
- Backed by a LoRA library (e.g., HF `peft`, or `axolotl` for orchestration) — reuses whatever training stack the repo already depends on if present, otherwise adds a pinned, well-known dependency.
- Emits real training metrics: loss per step/epoch, wall-clock duration, GPU-hours consumed.

### Merge Pipeline
- Interface: `run_merge(models: &[ModelRef], method: MergeMethod) -> Result<MergedModel, MergeError>`
- Backed by `mergekit` (subprocess or Python binding call from the Rust/Node orchestration layer).
- Pre-merge validation: `validate_compatibility(models: &[ModelRef]) -> Result<(), IncompatibilityReason>` checks architecture family and tokenizer match before attempting a merge.

### Coherence Smoke Test
- Interface: `smoke_test(model: &ModelRef) -> Result<(), SmokeTestFailure>`
- Runs a small fixed set of sample prompts through the model, checks for non-garbage output (basic perplexity/repetition heuristic) before the job is marked successful.

### Naming Engine
- Interface: `generate_name(base_family: &str, size: &str, descriptor: &str, existing_names: &HashSet<String>) -> String`
- Format: `"<Female First Name> <Family>-<Size> <Descriptor>"`, e.g. `"Erica GLM-9B Uncensored Quantized"`.
- Collision handling: appends a numeric suffix if the generated name already exists in the library.

### Model Library Registry
- Interface: `register_model(name: String, path: PathBuf, metadata: ModelMetadata) -> Result<(), RegistryError>`
- Interface: `list_models() -> Vec<ModelEntry>` — feeds the composer/menu UI hover-overlay (role, source, specs) already speced for the desktop IDE.

## Data Models

```rust
struct JobSpec {
    method: JobMethod,
    base_models: Vec<ModelRef>,      // one for fine-tune, 2+ for merge
    instructions: Option<TrainingSpec>,
    merge_method: Option<MergeMethod>,
    estimated_cost: f32,
    estimated_duration: Duration,
}

struct ModelRef { name: String, family: String, size: String, local_path: Option<PathBuf>, hf_repo: Option<String> }

struct TrainingSpec { custom_instructions: String, dataset_ref: Option<PathBuf>, target_behavior: String }

struct AdaptedModel { base: ModelRef, adapter_path: PathBuf, training_metrics: TrainingMetrics }
struct TrainingMetrics { final_loss: f32, duration: Duration, gpu_hours: f32 }

struct MergedModel { sources: Vec<ModelRef>, method: MergeMethod, output_path: PathBuf }

struct ModelEntry { display_name: String, path: PathBuf, family: String, size: String, descriptor: String, created_at: DateTime<Utc> }
```

## Correctness Properties

### Property 1: No silent GPU cost overrun
A Model Forge job whose estimated cost exceeds the user's configured spending threshold never starts without explicit confirmation, regardless of any autonomous calibration for lower-cost actions.

**Validates: Requirements 5.2**

### Property 2: No broken merges reach the library
A merge job is only registered in the Model Library if it passes both compatibility validation and the coherence smoke test; incompatible or incoherent outputs are rejected, not silently registered.

**Validates: Requirements 2.3, 2.4**

### Property 3: Idle GPU teardown
Any GPU instance provisioned by Model Forge is torn down on job completion, failure, cancellation, or 15 minutes of inactivity — no code path leaves it running unattended.

**Validates: Requirements 5.1, 5.3**

### Property 4: Naming uniqueness
No two models in the Model Library Registry share the same display name; collisions are always resolved with a disambiguating suffix before registration.

**Validates: Requirements 3.2**

## Error Handling

- If the requested base model is not found locally and cannot be downloaded (network failure, gated repo without access), the job fails clearly with a specific reason rather than silently substituting a different model.
- If LoRA training diverges (loss increases sustained over N steps) the pipeline aborts early and reports the divergence rather than completing and registering a degraded model.
- If mergekit reports an error, the raw error is surfaced to the user in conversational form rather than swallowed.
- If GPU provisioning fails (quota, availability), the job is not silently queued indefinitely — it reports failure so the user can retry or adjust.

## Testing Strategy

- Unit tests for Job Parser intent detection against labeled sample commands (fine-tune vs. merge vs. non-Forge chat).
- Unit tests for compatibility validation (matching and mismatched architecture/tokenizer fixtures).
- Integration test: run a real small-scale LoRA fine-tune on a small local model end-to-end, verify adapted model saves and produces different output than the base.
- Integration test: run a real merge of two small compatible models, verify smoke test passes and naming/registration works.
- Cost-discipline test: simulate a job exceeding the spending threshold, verify it blocks on confirmation; simulate idle GPU, verify auto-teardown fires.

