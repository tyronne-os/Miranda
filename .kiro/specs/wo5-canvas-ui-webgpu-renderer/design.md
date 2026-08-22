# WO-5: Design

## Start from what exists

`client-apps/web` (Vite + React + TypeScript) already has:
- `@xyflow/react` topology in `src/components/pipeline/` (`AceCortexPane.tsx`, `AceNodeCard.tsx`, `AceNodeTooltip.tsx`, `LiveMeter.tsx`)
- A presence viewport in `src/components/eve/` (`EvePresenceViewport.tsx`, `EveStudioPane.tsx`, `BlendshapeMeter.tsx`)
- A stage machine in `src/lib/stageMachine/` already implementing L0/L1/L2 staging
- A WebSocket/telemetry client in `src/lib/ace/` (`controllerClient.ts`, `spatialSyncer.ts`)

This is a substantial head start — the real net-new work for WO-5 is: (1) rewiring the topology data (`src/data/aceTopology.ts`) to match Miranda-Engine's actual 6-crate architecture instead of the original eve-ecc node set, (2) rewiring `controllerClient.ts` to speak to the new WO-4 Rust telemetry/transport servers instead of the old Node.js `ace-controller`, and (3) building the genuinely new piece — the WebGPU/WGSL Gaussian-splat viewport, which did not exist in the first attempt (the prior build used a CSS transform layer on a flat photo, explicitly documented as a placeholder in `ORCHESTRATION-PIVOT.md` — do not repeat that mistake).

## WebGPU viewport

New component, not an extension of the existing CSS presence layer. Ingests binary frames from the WO-4 WebRTC DataChannel, deforms a 3D Gaussian Splat mesh via WGSL compute/render shaders. Needs an actual splat asset to render against — coordinate with the `live-avatar-expert` skill's research (GaussianAvatars/FLAME/TetGS) for how that asset gets produced; this Work Order consumes it, doesn't produce it.

## Cross-reference

Full Hermes Execution Prompt: `nobility-posh-framework` skill. IPS behavioral spec: `eve-ecc-docs/INSTANT-PRESENCE-STANDARD.md`. Rendering science: `live-avatar-expert` skill.
