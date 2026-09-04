# Design Document

## Overview

The Conversational Intelligence layer sits between raw user input and LLM inference. It converts a single-turn Q&A loop into a continuously-adapting conversation by tracking mood as a live vector, maintaining a hierarchical conversation state, predicting useful proactive moves, modeling the user's genuine interests, learning corrections in-session, switching personas fluidly, and calibrating its own autonomy through an interview with the user. All components feed a single System Prompt Builder that assembles the final LLM context.

## Architecture

```
User Input (streaming)
      │
      ▼
Mood Stream Processor ──► Mood Vector (continuous)
      │                         │
      ▼                         ▼
Conversation State Machine ◄────┘
      │
      ▼
Anticipatory Move Generator ◄── Interest Model / Curiosity Engine
      │
      ▼
Knowledge Update Pipeline (corrections, framework mentions, code style)
      │
      ▼
Role/Persona Injection ◄── Autonomy Calibration Interview (thresholds)
      │                          │
      ▼                          ▼
System Prompt Builder ◄── Partnership Investment Tracker
      │
      ▼
LLM Inference ──► Response Latency/Depth Tuning ──► Output (text + avatar color)
      │
      ▼
Memory Data Lake (WO-Memory) — every component's output logged for retrieval
```

## Components and Interfaces

### Mood Stream Processor
- Interface: `process_chunk(chunk: &str) -> MoodVector`
- `MoodVector { frustration: f32, curiosity: f32, engagement: f32, fatigue: f32, excitement: f32 }`
- Smoothing: EMA with configurable alpha (default 0.3)

### Conversation State Machine
- Interface: `transition(current: State, mood: &MoodVector, entities: &[String], explicit_cue: Option<Cue>) -> State`
- `enum State { Opening, DeepWork, Debugging, Reflection, Casual }` each with micro-states `{ Listening, Thinking, Talking, Probing, Leading }`

### Anticipatory Move Generator
- Interface: `generate_moves(state: &State, mood: &MoodVector, history: &[Turn]) -> Vec<ScoredMove>`
- `ScoredMove { text: String, confidence: f32 }`
- Only moves with `confidence >= 0.7` are surfaced; dismissal feedback lowers future scores for similar move categories.

### Interest Model / Curiosity Engine
- Interface: `update_interests(entities: &[String], sentiment: Sentiment) -> ()`
- Interface: `next_curiosity_question() -> Option<String>` (rate-limited to ≤1/hour)

### Knowledge Update Pipeline
- Interface: `detect_correction(prior_claim: &str, new_message: &str) -> Option<CorrectedFact>`
- Interface: `apply_session_knowledge(prompt: &mut PromptContext)`
- Interface: `profile_code_style(code_sample: &str) -> CodeStyleProfile`

### Role/Persona Injection
- Interface: `detect_role(message: &str, state: &State, mood: &MoodVector) -> Role`
- `enum Role { ResearchPartner, RubberDuck, PeerReviewer, Therapist, BrainstormCoCreator, General }`

### Autonomy Calibration Interview
- Interface: `run_calibration_interview() -> AutonomyThresholds`
- Interface: `get_threshold(category: ActionCategory) -> AutonomyLevel` where `AutonomyLevel { Autonomous, FastPathConfirm, ExplicitConfirm }`
- Fixed floor: `ActionCategory::DestructiveAtScale | ProductionImpacting | HighBlastRadius` always resolves to `ExplicitConfirm` regardless of stored thresholds.

### Partnership Investment Tracker
- Interface: `extract_goal(message: &str) -> Option<Goal>`
- Interface: `check_progress(goal: &Goal, recent_turns: &[Turn]) -> Option<ProgressAcknowledgment>`
- Content constraint: generated acknowledgments are filtered against a banned-pattern list (dependency/guilt language) before surfacing.

### System Prompt Builder
- Interface: `build_prompt(role: Role, state: &State, mood: &MoodVector, memory_context: &[RetrievedContext], moves: &[ScoredMove], goal_ack: Option<ProgressAcknowledgment>) -> String`
- Token budget default 2000; truncation priority (drop first): anticipatory moves → curiosity questions → partnership acknowledgment → role detail → memory context (never dropped below 1 item).

## Data Models

```rust
struct MoodVector { frustration: f32, curiosity: f32, engagement: f32, fatigue: f32, excitement: f32 }

enum State { Opening, DeepWork, Debugging, Reflection, Casual }
enum MicroState { Listening, Thinking, Talking, Probing, Leading }

struct ScoredMove { text: String, confidence: f32, category: MoveCategory }

struct InterestEntry { topic: String, frequency: u32, sentiment: Sentiment, last_mentioned: DateTime<Utc> }

struct CorrectedFact { fact: String, corrected_by_user: bool, confidence: f32, session_id: Uuid }

enum ActionCategory { FileOperations, Spending, VersionControl, InstallConfig, DestructiveAtScale, ProductionImpacting, HighBlastRadius }
enum AutonomyLevel { Autonomous, FastPathConfirm, ExplicitConfirm }
struct AutonomyThresholds(HashMap<ActionCategory, AutonomyLevel>);

struct Goal { description: String, created_at: DateTime<Utc>, status: GoalStatus }
enum GoalStatus { Active, Progressing, Achieved }
```

## Correctness Properties

### Property 1: Confidence gating
No anticipatory move or curiosity question is ever surfaced below its defined confidence/rate-limit threshold.

**Validates: Requirements 3.2, 4.3**

### Property 2: Autonomy floor invariant
`ActionCategory::DestructiveAtScale | ProductionImpacting | HighBlastRadius` can never resolve to `Autonomous`, regardless of interview answers or track record. This is the one property that must hold under every possible calibration input.

**Validates: Requirements 7.4**

### Property 3: Session knowledge precedence
A corrected fact from earlier in the session always takes precedence over conflicting base LLM knowledge for the remainder of that session.

**Validates: Requirements 5.2**

### Property 4: Non-dependency invariant
Partnership acknowledgment text generation is checked against a banned-pattern filter before being surfaced; matches are rejected and regenerated or dropped.

**Validates: Requirements 8.3**

## Error Handling

- If mood classification fails or times out (>50ms budget exceeded), the system falls back to the last known mood vector rather than blocking the turn.
- If the anticipation module cannot reach confidence on any candidate, the system silently omits proactive content — no error surfaced to the user.
- If the calibration interview is interrupted before completion, unanswered categories default to `ExplicitConfirm` until answered.
- If the Memory Data Lake (WO-Memory) is unavailable, the System Prompt Builder proceeds without historical context rather than failing the turn.

## Testing Strategy

- Unit tests per component with labeled fixtures (mood trajectories, state transition sequences, correction detection samples).
- Integration tests for the full pipeline: raw input → mood → state → moves → prompt, verifying expected elements appear/are suppressed correctly.
- Calibration floor test: verify no interview configuration can produce `Autonomous` for the fixed-floor categories.
- Content-safety test for Partnership Investment Tracker: run banned-pattern corpus against acknowledgment generator, verify 100% rejection.
- Manual tone review: sampled transcripts per role reviewed for persona consistency and non-dependency framing.

