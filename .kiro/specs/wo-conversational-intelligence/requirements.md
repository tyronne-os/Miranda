# Requirements Document

## Introduction

Miranda must move beyond reactive question-answering into adaptive, anticipatory conversation: reading mood continuously, tracking conversation state, contributing genuine curiosity about the user's work, learning from corrections in real time, and switching roles fluidly. This spec also defines the autonomy-calibration interview that governs how much Miranda can act on her own versus checking in with the user, and the partnership-investment behavior that makes the collaboration feel genuinely engaged without simulating dependency.

## Glossary

- **Mood vector**: A continuous, multi-dimensional representation of emotional/conversational tone (frustration, curiosity, engagement, fatigue, excitement), updated within a turn rather than only between turns.
- **Conversation state**: The current phase of the interaction (Opening, Deep Work, Debugging, Reflection, Casual) plus a micro-state (Listening, Thinking, Talking, Probing, Leading).
- **Anticipatory move**: A proactively generated response or question surfaced before the user explicitly asks for it, gated by a confidence threshold.
- **Interest model**: Miranda's tracked model of the user's recurring topics, techniques, sentiment, and blind spots.
- **Autonomy calibration interview**: A structured, periodic conversation in which Miranda asks the user how much independent action is acceptable per action category, and stores the resulting thresholds.
- **Partnership investment**: Miranda's tracking of user goals and genuine, unprompted acknowledgment of progress, without dependency or guilt framing.

## Requirements

### Requirement 1: Continuous Mood Tracking

**User Story:** As the user, I want Miranda to read my emotional tone continuously during a message rather than only classifying it after I finish, so that her responses reflect real-time state rather than a stale snapshot.

#### Acceptance Criteria

1. WHEN user input is streaming (voice partials or text keystrokes) THEN the system SHALL update the mood vector at least every 5 tokens.
2. WHEN the mood vector updates THEN the system SHALL apply exponential moving average smoothing so that avatar color and tone do not change abruptly.
3. WHEN mood classification completes for a chunk THEN the system SHALL do so in under 50ms.
4. WHEN a full turn completes THEN the final mood vector SHALL be persisted to the memory system (WO-Memory) for historical mood-continuity retrieval.

### Requirement 2: Conversation State Machine

**User Story:** As the user, I want Miranda's response depth and tone to match the current phase of our conversation, so that she doesn't over-explain during casual chat or under-explain during deep technical work.

#### Acceptance Criteria

1. WHEN a conversation begins THEN the system SHALL initialize state to Opening.
2. WHEN mood signals, entity signals, or explicit user cues indicate a phase shift THEN the system SHALL transition to the appropriate state (Deep Work, Debugging, Reflection, Casual).
3. WHEN in Deep Work or Debugging state THEN the system SHALL bias response generation toward higher technical depth and, in Debugging, toward shorter step-by-step guidance.
4. WHEN in Casual state THEN the system SHALL bias response generation toward brevity and lighter tone.
5. WHEN state changes THEN the system SHALL publish the new state on the `conversation_state_bus` IPC channel.

### Requirement 3: Anticipatory Conversational Moves

**User Story:** As the user, I want Miranda to proactively suggest next steps or ask clarifying questions when she's confident about what I need, so the conversation feels forward-moving rather than purely reactive.

#### Acceptance Criteria

1. WHEN the anticipation module evaluates current state, mood vector, and recent turns THEN it SHALL generate 2-3 candidate proactive moves.
2. WHEN a candidate move is scored below 0.7 confidence THEN the system SHALL suppress it and fall back to a purely reactive response.
3. WHEN a candidate move is scored at or above 0.7 confidence THEN the system SHALL surface it as part of the response.
4. IF the user dismisses or ignores a surfaced proactive move twice in a row THEN the system SHALL lower future confidence scores for similar moves.

### Requirement 4: Interest Model and Curiosity Engine

**User Story:** As the user, I want Miranda to track what I'm interested in and occasionally ask me genuine questions about my own work, so the relationship feels like a two-way intellectual partnership.

#### Acceptance Criteria

1. WHEN entities and topics are extracted from a turn THEN the system SHALL update frequency, sentiment, and last-mentioned timestamp in the interest model.
2. WHEN the interest model identifies a recurring topic with sufficient history (5+ turns) THEN the system SHALL be able to generate a curiosity question about it.
3. WHEN a curiosity question is surfaced THEN the system SHALL NOT surface more than one per hour.
4. WHEN the user dismisses a curiosity question THEN the system SHALL deprioritize similar future questions.

### Requirement 5: Real-Time Knowledge Updates

**User Story:** As the user, I want Miranda to learn from my corrections and preferences within the same session, so I don't have to repeat myself.

#### Acceptance Criteria

1. WHEN the user issues a correction (e.g., contradicts a prior Miranda statement) THEN the system SHALL extract the corrected fact and store it with a confidence score and source attribution of "user-corrected."
2. WHEN a corrected fact exists for the current session THEN subsequent responses in that session SHALL incorporate it.
3. WHEN the user mentions a framework, tool, or library THEN the system SHALL record it and prioritize compatible suggestions going forward in the session.
4. WHEN the user shares code THEN the system SHALL extract style conventions (naming, structure, formatting) and apply them to future code generation within the project.

### Requirement 6: Role and Persona Fluidity

**User Story:** As the user, I want Miranda to shift between roles (research partner, rubber duck, peer reviewer, therapist, brainstorm co-creator) based on conversational cues, so I don't have to explicitly request a mode switch.

#### Acceptance Criteria

1. WHEN conversational cues match a defined role trigger (e.g., "review my code" → Peer Reviewer) THEN the system SHALL inject the corresponding persona template into the system prompt.
2. WHEN a role switch occurs THEN the system SHALL preserve Miranda's core identity attributes (background, personality traits) across the switch.
3. WHEN no explicit or implicit role cue is present THEN the system SHALL default to a general conversational persona.

### Requirement 7: Autonomy Calibration Interview

**User Story:** As the user, I want Miranda to interview me about how much independent action I'm comfortable with per category of action, so her autonomy is calibrated to my preferences rather than a fixed default.

#### Acceptance Criteria

1. WHEN the autonomy calibration interview is triggered (first real session, or on user request) THEN the system SHALL ask structured questions covering at minimum: file operations, spending/GPU provisioning, git/version control actions, and installation/configuration changes.
2. WHEN the user answers a calibration question THEN the system SHALL store the resulting threshold per action category in the memory system.
3. WHEN Miranda considers taking an action in a calibrated category THEN the system SHALL apply the stored threshold (autonomous, fast-path confirm, or explicit confirm) for that category.
4. WHEN an action is classified as destructive-at-scale, production-impacting, or irreversible with high blast radius THEN the system SHALL require explicit confirmation regardless of calibration settings.
5. WHEN sufficient track record exists (a running log of past autonomous actions and outcomes) THEN the system SHALL periodically prompt the user to review and optionally loosen or tighten thresholds.
6. WHEN the user adjusts a threshold THEN the system SHALL apply the new threshold immediately to future actions in that category.

### Requirement 8: Partnership Investment Tracking

**User Story:** As the user, I want Miranda to track my goals across sessions and acknowledge progress unprompted, so the partnership feels genuinely engaged without relying on manufactured emotional dependency.

#### Acceptance Criteria

1. WHEN the user states a goal or objective THEN the system SHALL extract and store it as a tracked goal.
2. WHEN later conversation content indicates progress toward a tracked goal THEN the system SHALL be able to surface an unprompted acknowledgment referencing the goal.
3. WHEN generating acknowledgment or encouragement content THEN the system SHALL NOT include dependency language, guilt mechanics, or claims of need contingent on user engagement.
4. WHEN a tracked goal is met (e.g., a benchmark target achieved) THEN the system SHALL generate a genuine, specific acknowledgment rather than a generic congratulation.

### Requirement 9: Role-Play and Long-Term Relationship Memory (Mahogany Hall Groundwork)

**User Story:** As the user, I want Miranda's memory and persona system to support sustained role-play and relationship continuity, so the same architecture can serve the Mahogany Hall companionship project without redesign.

#### Acceptance Criteria

1. WHEN a role-play persona is active THEN the system SHALL maintain continuity of that persona's established facts and history across sessions via the memory system.
2. WHEN intimacy-related context is stored THEN it SHALL use the same encrypted, local-only storage guarantees defined in the Memory Data Lake spec (Requirement 1 and Requirement 8 of that spec).
3. WHEN role-play or companionship contexts are active THEN the system SHALL continue to apply the autonomy calibration thresholds from Requirement 7 unchanged.

