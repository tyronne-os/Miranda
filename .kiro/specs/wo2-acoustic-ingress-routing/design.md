# WO-2: Design

## Architecture

`miranda-audio` owns mic capture + VAD + the parakeet.cpp FFI wrapper. `miranda-supervisor` owns the Nemotron-Flash routing logic and turn-taking state machine — kept separate because the audio crate should have zero LLM/networking dependencies (keeps it embeddable/testable in isolation).

## parakeet.cpp FFI

This project already has a real, verified-working `parakeet.cpp` deployment (built from source with `-march=native` because this hardware class lacks AVX2 — see `llamacpp-huggingface-expert` skill for the exact lesson). The FFI wrapper (`cxx` crate) should link against that existing build's shared library rather than vendoring a second copy. Bind the C API surface `parakeet.cpp` already exposes (transcribe from PCM buffer → token stream) — do not reinvent a Rust-native ASR path.

## Node Warden concept (Vanguard Innovation #1)

`miranda-supervisor` is also where the Work Order 1 "Node Warden" pattern lives operationally: a small local GGUF model (1-4B, via llama.cpp — see `llamacpp-huggingface-expert`) monitoring this node's own throughput. This is a later refinement, not required for WO-2's first pass — get the deterministic ASR/VAD/routing path working and correct before adding self-monitoring.

## Cross-reference

Full Hermes Execution Prompt and Sequence Matrix: `nobility-posh-framework` Kiro skill.
