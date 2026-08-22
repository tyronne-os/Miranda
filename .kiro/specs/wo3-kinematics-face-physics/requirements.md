# WO-3: 4D Kinematics, ARKit Blendshape Math & Face Physics — Requirements

**Role**: Computational Kinematics and Graphics Engineer. **Depends on**: WO-1 (ring buffer for output). **Target**: 60 FPS continuous math.

## Requirements (EARS notation)

1. WHEN audio energy data arrives THE SYSTEM SHALL compute 52 ARKit-compatible blendshape weights using native SIMD routines (`wide`/`simba`), not scalar loops.
2. WHILE the system is running, regardless of speech activity, THE SYSTEM SHALL continuously run a Perlin-noise ocular micro-saccade generator on an isolated high-priority thread.
3. WHILE the system is running THE SYSTEM SHALL run an asymmetric eye-blink state machine (not perfectly symmetric/periodic — see the Instant Presence Standard's micromovement requirement).
4. WHILE the system is running THE SYSTEM SHALL run a sine-wave respiratory modulator affecting clavicle and jaw priors, even during silence.
5. WHEN a blendshape value changes between consecutive frames THE SYSTEM SHALL clamp its velocity to prevent mesh tearing during extreme/rapid speech transitions.
6. WHEN a frame payload is computed THE SYSTEM SHALL export it to the WO-1 shared memory bus at a sustained 60 FPS.
7. THE SYSTEM SHALL satisfy the Instant Presence Standard's No Loop Video Protocol: zero motion for more than one frame interval is a defect, even in an otherwise-idle state (see `eve-ecc-docs/INSTANT-PRESENCE-STANDARD.md` in this repo).

## Acceptance criteria

- Sustained 60 FPS measured under real load, not just claimed.
- Blendshape output never holds perfectly static for more than one frame interval while the system is "on," per the No Loop Video Protocol.
- Velocity clamping is demonstrated to prevent a specific reproducible tearing case (construct one, show it's fixed).
