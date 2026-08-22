# WO-3: Design

## Architecture

Lives entirely in `miranda-nodes`, depending on `miranda-core` (shared types) and `miranda-ipc` (to write output frames). No networking, no LLM — this crate should be pure math and should be unit-testable without any of the audio/network stack running.

## Thread model

Each oscillator (Perlin micro-saccade, eye-blink state machine, respiratory modulator) runs on its own isolated high-priority thread per the source spec — not multiplexed onto a single "animation" thread, so a slow blink calculation can't stall breathing. Combine their outputs into one blendshape frame just before writing to the WO-1 ring, not earlier.

## Why this directly implements the Instant Presence Standard

This crate is *the* mechanism satisfying `INSTANT-PRESENCE-STANDARD.md`'s No Loop Video Protocol and Vanguard Innovations #21/#25: because the oscillators run continuously regardless of speech state, "idle" and "speaking" are never two different code paths — there is no seam where a canned idle loop could be swapped in. Keep it that way; do not add a separate "idle animation" mode later as a shortcut.

## Cross-reference

Full Hermes Execution Prompt: `nobility-posh-framework` skill. Full IPS spec: `eve-ecc-docs/INSTANT-PRESENCE-STANDARD.md` in this repo.
