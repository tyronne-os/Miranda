//! miranda-audio — Work Order 2: mic ingress, Silero VAD, and the
//! parakeet.cpp FFI bindings. This project already has a real, working
//! parakeet.cpp deployment (see `llamacpp-huggingface-expert` Kiro skill) —
//! bind to that existing native build rather than starting from scratch.
//!
//! This crate is Pipeline 2 (native/bare-metal) only. Pipeline 1's mic
//! capture is browser-side (see `client-apps/web/src/audio/MicCapture.ts`
//! and `client-services/ace-controller/`) because the AWS deployment
//! target for Pipeline 1 is a headless EC2 instance with no sound card.

pub mod asr;
pub mod capture;
