# WO-2: Acoustic Ingress, VAD & Supervisory Routing — Requirements

**Role**: Real-Time Audio Systems Specialist. **Depends on**: WO-1 (ring buffer must exist). **Target**: <30ms transcription.

## Requirements (EARS notation)

1. WHEN the microphone produces PCM audio THE SYSTEM SHALL capture it via a non-blocking stream (`cpal`) and write chunks into the WO-1 ring buffer without blocking the audio callback thread.
2. WHEN speech begins or ends in the incoming audio THE SYSTEM SHALL detect the transition using Silero VAD within 10ms of the actual transition.
3. WHEN a PCM frame is ready for transcription THE SYSTEM SHALL pass it directly from the lock-free ring to the parakeet.cpp engine via the `cxx` FFI boundary — no intermediate copy through a non-realtime path (e.g. no round-trip through a Python process).
4. WHEN parakeet.cpp returns a transcript token stream THE SYSTEM SHALL forward it to the Nemotron-Flash supervisory agent in `miranda-supervisor` for intent parsing and turn-taking evaluation.
5. WHEN the supervisory agent determines the user has finished their turn THE SYSTEM SHALL dispatch a signal to the downstream TTS and motion nodes.
6. WHEN audio is actively streaming THE SYSTEM SHALL maintain a rolling lookahead buffer so mouth coarticulation shaping has upcoming audio context, not just the current frame.

## Acceptance criteria

- Reuse the existing native `parakeet.cpp` build already deployed in this project (see `llamacpp-huggingface-expert` Kiro skill for the exact binary/build flags — `-march=native`, CPU-only) rather than rebuilding it from scratch.
- Measure and report real end-to-end transcription latency, not a vendor-claimed number.
- VAD transition latency is measured against real audio, not synthetic silence-to-tone test signals only.
