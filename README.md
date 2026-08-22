# Miranda-Engine

The harness for the **NOBILITY POSH FRAMEWORK** — exceptional 2D-to-4D humanized companions & colleagues. Turns a 2D control image ("EVE") into a zero-latency, photorealistic, emotionally-aware 4D digital human via a self-optimizing graph of expert agent nodes, not a rigid linear pipeline.

This is a **Hermes multi-agent project**: the build is organized as 5 Work Orders, each written as a Hermes execution prompt for a distinct specialist role. Full science, architecture rationale, and verbatim Work Orders live in the Kiro skill `nobility-posh-framework` (`~/.kiro/skills/nobility-posh-framework/`) — hand this repo to Kiro/Hermes with that skill active to build it out.

## What's here (basic scaffold only — the real implementation is Kiro/Hermes's job)

```
Cargo.toml              Workspace root — 6 crates, currently empty skeletons that `cargo build` clean
miranda-core/            Shared types (WO-1)
miranda-ipc/              POSIX SHM ring buffer (WO-1)
miranda-audio/            Mic ingress, VAD, parakeet.cpp FFI (WO-2)
miranda-nodes/            ARKit-52 blendshape SIMD kinematics (WO-3)
miranda-supervisor/       Nemotron-Flash routing + Node Wardens (WO-2/WO-1)
miranda-transport/        webrtc-rs + Axum telemetry (WO-4)

client-apps/web/         The WO-5 client — seeded from the first-attempt eve-ecc repo
                          (Vite+React+TS, @xyflow/react ACE Cortex topology UI, already
                          matches the Work Order 5 spec closely — extend, don't rewrite)
client-services/          The eve-ecc "ace-controller" streaming orchestrator, carried over as-is
client-scripts/           Dev scripts carried over from eve-ecc
eve-ecc-docs/              Design docs from the first attempt: the Instant Presence Standard
                          (control-plane < 1s, L0→L1→L2 staging), the IDE/backend contract,
                          and the orchestration pivot notes — genuinely good prior design work,
                          read before touching client-apps/web

images/
  evedefault.jpg          The EVE reference image (2D control element)
```

## Origin note — why this repo looks the way it does

This repo (`tyronne-os/miranda`) was originally created as a fork of `microsoft/agent-framework` — an unrelated multi-agent template with no connection to this project. It's been rebuilt from scratch with the content above, at the owner's explicit direction. If you're looking at git history predating this README, that's the wrong fork's content — safe to disregard.

## Where the science and build plan live

- **Kiro skill `nobility-posh-framework`** — the master reference: why this architecture (semantic graph vs. DAG, the GaussianAvatars/TetGS/FitMe rendering chain), the 5 Work Orders verbatim, the 25 Vanguard Innovations, and the AWS deployment pathway.
- **Kiro skill `live-avatar-expert`** — the real-time avatar generation science (LiveAvatar, NVIDIA ACE, AWS's own approach, top-10 code-backed research papers).
- **Kiro skill `aws-pipeline-architect`** — cost-contained AWS infrastructure (the CloudWatch GPU kill-switch, instance tiering).
- **Kiro skill `llamacpp-huggingface-expert`** — the Node Warden micro-LLM tooling (GGUF quantization, real deployment lessons from this project's own parakeet.cpp build).

## Known gap

A second reference image (the "prototype" screenshot from the first eve-ecc build attempt) was shown during this repo's setup but couldn't be exported to a file from the chat session — no local copy exists yet. Add it to `images/` manually if you want it preserved here.

## Build

```bash
cargo build   # Rust workspace — currently empty skeletons, real builds happen at once each Work Order lands
cd client-apps/web && npm install && npm run dev   # the WO-5 client
```
