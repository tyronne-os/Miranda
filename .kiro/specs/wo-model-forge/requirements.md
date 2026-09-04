# Requirements Document

## Introduction

Miranda needs the ability to customize local LLMs end-to-end — fine-tuning/LoRA adaptation on downloaded base models, seamless merging of multiple models into hybrids, and deployment of the result — all triggerable from a single text or voice command in an ongoing conversation. Every customized model produced by this system is renamed following the project's established convention (a female first name, followed by the base model family and size as the "last name," e.g. "Erica GLM-9B Uncensored"). This spec explicitly does not cover from-scratch pretraining, which requires research-scale compute and time and is out of scope for a conversationally-triggered feature.

## Glossary

- **Model Forge**: The subsystem that fine-tunes, merges, names, and deploys local LLMs on request.
- **LoRA adaptation**: Low-rank fine-tuning applied to a base model to specialize it without full-parameter retraining.
- **Model merge**: Combining weights of two or more compatible models into a single hybrid model (e.g., via SLERP, TIES, or DARE merge methods).
- **Base model**: A downloaded, pretrained model (GLM, Nemotron, Gemma, Phi, etc.) used as the starting point for fine-tuning or merging.
- **Naming convention**: `<Female First Name> <Base Model Family>-<Size> <Descriptor>` (e.g., "Erica GLM-9B Uncensored Quantized").
- **Deployment trigger**: A voice or text command issued during a conversation with Miranda that initiates a Model Forge job.

## Requirements

### Requirement 1: Voice/Text-Triggered Fine-Tuning

**User Story:** As the user, I want to tell Miranda in conversation to fine-tune a downloaded model with custom weights and instructions, so I don't need to leave the chat to run training scripts manually.

#### Acceptance Criteria

1. WHEN the user issues a text or voice command indicating intent to customize a model (e.g., "fine-tune GLM with these instructions") THEN the system SHALL parse the command into a structured fine-tuning job specification.
2. WHEN a fine-tuning job specification is created THEN the system SHALL identify the target base model, locate it locally or trigger a download, and validate compatibility with the configured LoRA pipeline.
3. WHEN a fine-tuning job runs THEN the system SHALL apply LoRA adaptation using user-provided instructions/data rather than full-parameter retraining, unless the user explicitly requests full fine-tuning and confirms the higher GPU cost.
4. WHEN a fine-tuning job completes THEN the system SHALL save the resulting adapted model to local storage and report real training metrics (loss curve, duration, GPU cost) as verification.

### Requirement 2: Seamless Model Merging

**User Story:** As the user, I want to merge two or more downloaded models into a hybrid via conversation, so I can combine strengths (e.g., a coding-strong model with an uncensored-persona model) without manual CLI work.

#### Acceptance Criteria

1. WHEN the user requests a model merge naming two or more base models THEN the system SHALL validate architecture/tokenizer compatibility before proceeding.
2. WHEN models are compatible THEN the system SHALL perform the merge using a configurable method (SLERP, TIES, or DARE) via an integrated merge tool (e.g., mergekit).
3. IF models are incompatible (mismatched architecture family or tokenizer) THEN the system SHALL report the incompatibility clearly rather than attempting a merge that would produce a broken model.
4. WHEN a merge completes THEN the system SHALL run a basic coherence smoke-test (sample generation) on the merged model before marking the job successful.

### Requirement 3: Naming Convention Enforcement

**User Story:** As the user, I want every customized or merged model automatically named per the established convention, so my local model library stays organized and identifiable at a glance.

#### Acceptance Criteria

1. WHEN a fine-tuning or merge job completes successfully THEN the system SHALL generate a name in the form `<Female First Name> <Base Model Family>-<Size> <Descriptor>`.
2. WHEN a name is generated THEN the system SHALL ensure it is unique within the local model library, appending a disambiguating suffix if a collision occurs.
3. WHEN a model is renamed THEN the system SHALL update all references in the composer/menu UI to reflect the new name immediately.

### Requirement 4: Conversational Deployment Trigger

**User Story:** As the user, I want to trigger Model Forge jobs from natural conversation with Miranda (voice or text), so the feature fits inside normal usage rather than requiring a separate tool.

#### Acceptance Criteria

1. WHEN the user's message contains a recognizable Model Forge intent THEN the conversational intelligence layer (WO-Conversational-Intelligence) SHALL route it to the Model Forge job parser rather than treating it as a normal chat turn.
2. WHEN a Model Forge job is queued THEN the system SHALL confirm the job parameters back to the user in natural language before starting (base model, method, estimated GPU cost/time) per the autonomy calibration threshold for the "Spending"/GPU-provisioning category defined in WO-Conversational-Intelligence Requirement 7.
3. WHEN a Model Forge job is running THEN the system SHALL report progress conversationally if asked (e.g., "how's the fine-tune going?").
4. WHEN a Model Forge job completes THEN the system SHALL announce completion and the new model's name in the same conversation thread that triggered it.

### Requirement 5: GPU Cost Discipline

**User Story:** As the user, I want Model Forge jobs to respect GPU cost discipline, so idle or runaway training doesn't burn money unattended.

#### Acceptance Criteria

1. WHEN a Model Forge job requires GPU provisioning THEN the system SHALL apply the same idle auto-stop / 15-minute inactivity timer defined in the project's build standards.
2. WHEN a job's estimated cost exceeds the user's configured spending threshold (from the autonomy calibration interview) THEN the system SHALL require explicit confirmation before starting, regardless of any "autonomous" calibration for lower-cost actions.
3. WHEN a job fails or is cancelled THEN the system SHALL ensure the GPU instance is torn down rather than left running idle.

### Requirement 6: Scope Boundary — No From-Scratch Pretraining

**User Story:** As the user, I want it to be clear that Model Forge customizes existing models rather than pretraining new ones from random initialization, so expectations match what's actually deliverable via a conversational trigger.

#### Acceptance Criteria

1. WHEN a user request implies from-scratch pretraining (random weight initialization, training on a raw uncurated corpus) THEN the system SHALL explain that this is out of scope for conversational triggering and instead offer fine-tuning or merging as the deliverable alternative.

