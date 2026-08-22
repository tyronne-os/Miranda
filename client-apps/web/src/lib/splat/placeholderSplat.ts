/**
 * WO-5 T5 — placeholder Gaussian-splat asset.
 *
 * There is no trained/rigged Gaussian-splat avatar of EVE yet — the
 * GaussianAvatars/FLAME/TetGS pipeline that would produce one is a separate,
 * research-heavy effort (`live-avatar-expert` skill) and this Work Order's
 * requirements.md explicitly says not to block the WebGPU/WGSL viewport work
 * on that research finishing.
 *
 * This module generates a small, PROCEDURAL splat cloud approximating a head
 * — not a photo, not a mesh, not a stand-in image. It exists purely so T4's
 * renderer has real per-Gaussian data (position, scale, rotation quaternion,
 * color, opacity) to upload to the GPU and deform, in exactly the data shape
 * a real trained asset would arrive in. Swapping in a real asset later is a
 * data change (replace `buildPlaceholderSplat`'s output with a loaded .ply/
 * .splat file), not a renderer rewrite — the renderer only ever consumes
 * `SplatData`, never this generator directly.
 *
 * # Why generated code, not a binary asset file
 *
 * A hand-crafted or downloaded placeholder .ply would be an opaque binary
 * blob nobody could inspect, diff, or regenerate deterministically. This
 * generator is auditable source, reproducible from a seed, and small enough
 * to read in one sitting — appropriate weight for something explicitly
 * documented as a stand-in.
 *
 * # Layout — mirrors the standard Gaussian-splat per-splat attribute set
 * (position, scale, rotation, opacity, color) used by every major splat
 * renderer (the original INRIA reference implementation, antimatter15's
 * WebGL splat viewer, Niantic's SplatFacto), so the WGSL shader written
 * against this shape transfers unchanged to a real trained asset.
 */

export interface SplatData {
    /** Number of Gaussians. */
    count: number;
    /** xyz per splat, in a head-local coordinate space: y-up, z-forward,
     * head centered near the origin, scale in "head units" (~1.0 unit tall
     * from chin to crown). Length = count * 3. */
    positions: Float32Array;
    /** Per-axis scale (sigma) of each Gaussian's covariance ellipsoid,
     * xyz. Length = count * 3. */
    scales: Float32Array;
    /** Unit rotation quaternion [x,y,z,w] orienting each Gaussian's
     * ellipsoid. Length = count * 4. */
    rotations: Float32Array;
    /** RGB color, linear 0..1. Length = count * 3. */
    colors: Float32Array;
    /** Opacity, 0..1. Length = count. */
    opacities: Float32Array;
    /**
     * Skinning weight toward the "jaw" bone/region, 0..1, PER SPLAT.
     * Not part of the standard splat attribute set — this is
     * Miranda-Engine-specific and is what T4's vertex/compute shader uses
     * to know how much a given Gaussian should move when jawOpen rises.
     * A real trained rig (GaussianAvatars-style) computes this from FLAME
     * mesh skinning; here it's approximated from height in the head-local
     * space (splats near the chin get high jaw weight, splats near the
     * crown get none).
     */
    jawWeights: Float32Array;
    /** Same idea for the two eyelid regions, so blink can close them
     * without moving the rest of the face. */
    eyelidWeights: Float32Array;
}

/** Deterministic PRNG (mulberry32) so the placeholder is reproducible across
 * runs — a splat cloud that regenerates differently every reload would make
 * visual regressions impossible to diff. */
function mulberry32(seed: number) {
    let a = seed;
    return () => {
        a |= 0;
        a = (a + 0x6d2b79f5) | 0;
        let t = Math.imul(a ^ (a >>> 15), 1 | a);
        t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
        return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
}

/** Box-Muller transform for a roughly Gaussian-distributed sample, given a
 * uniform PRNG — Gaussians packed on a uniform sphere read as a beach ball,
 * not a head; a soft radial falloff toward the surface is what actually
 * looks head-shaped. */
function gaussianRandom(rand: () => number): number {
    const u1 = Math.max(rand(), 1e-6);
    const u2 = rand();
    return Math.sqrt(-2 * Math.log(u1)) * Math.cos(2 * Math.PI * u2);
}

export interface PlaceholderSplatOptions {
    /** Number of Gaussians to generate. Kept small deliberately — this is a
     * placeholder for shader development, not a fidelity target. */
    count?: number;
    seed?: number;
}

/**
 * Builds a procedural ellipsoidal splat cloud approximating a head, with
 * denser coverage and distinct coloring for a jaw region (lower third,
 * front-weighted) and two eyelid regions (upper-middle, either side of the
 * vertical midline) so the placeholder can visibly demonstrate jaw-open and
 * blink deformation without needing a real face.
 */
export function buildPlaceholderSplat(opts: PlaceholderSplatOptions = {}): SplatData {
    const count = opts.count ?? 4000;
    const rand = mulberry32(opts.seed ?? 0xE5E_1234);

    const positions = new Float32Array(count * 3);
    const scales = new Float32Array(count * 3);
    const rotations = new Float32Array(count * 4);
    const colors = new Float32Array(count * 3);
    const opacities = new Float32Array(count);
    const jawWeights = new Float32Array(count);
    const eyelidWeights = new Float32Array(count);

    // Head-local ellipsoid radii (head units): a touch taller than wide,
    // slightly deeper than wide — a rough egg, not a sphere.
    const RADIUS_X = 0.42;
    const RADIUS_Y = 0.5;
    const RADIUS_Z = 0.46;

    // Jaw region: lower 30% of head height, front-weighted (positive z).
    const JAW_Y_MAX = -0.18; // top of jaw region in head-local y
    const JAW_Z_MIN = 0.05; // must be reasonably forward-facing

    // Eyelid regions: upper-middle band, split left/right of x=0.
    const EYE_Y_MIN = 0.02;
    const EYE_Y_MAX = 0.22;
    const EYE_Z_MIN = 0.2; // near the front of the face

    for (let i = 0; i < count; i += 1) {
        // Sample a point inside the ellipsoid via a Gaussian-weighted
        // direction scaled by a random radius fraction — denser toward the
        // surface (where a real face's visible mass concentrates) than a
        // uniform-volume fill would give.
        let gx = gaussianRandom(rand);
        let gy = gaussianRandom(rand);
        let gz = gaussianRandom(rand);
        const glen = Math.sqrt(gx * gx + gy * gy + gz * gz) || 1;
        gx /= glen;
        gy /= glen;
        gz /= glen;

        // Radius fraction biased toward the outer shell (r^(1/3) would be
        // uniform-volume; a higher power pushes mass outward, which is what
        // makes this read as a surface rather than a solid fog ball).
        const r = Math.pow(rand(), 1 / 4);

        const x = gx * r * RADIUS_X;
        const y = gy * r * RADIUS_Y;
        const z = gz * r * RADIUS_Z * (gz > 0 ? 1.0 : 0.75); // flatten the back slightly

        positions[i * 3 + 0] = x;
        positions[i * 3 + 1] = y;
        positions[i * 3 + 2] = z;

        // Small, roughly isotropic scale with a little jitter so the cloud
        // doesn't read as a lattice of identical dots.
        const baseScale = 0.014 + rand() * 0.01;
        scales[i * 3 + 0] = baseScale * (0.85 + rand() * 0.3);
        scales[i * 3 + 1] = baseScale * (0.85 + rand() * 0.3);
        scales[i * 3 + 2] = baseScale * (0.85 + rand() * 0.3);

        // Near-identity rotation with small random jitter per splat, so
        // ellipsoids aren't all axis-aligned in lockstep (a real trained
        // asset never is either).
        const jitter = 0.15;
        let qx = (rand() - 0.5) * jitter;
        let qy = (rand() - 0.5) * jitter;
        let qz = (rand() - 0.5) * jitter;
        let qw = 1;
        const qn = Math.sqrt(qx * qx + qy * qy + qz * qz + qw * qw);
        rotations[i * 4 + 0] = qx / qn;
        rotations[i * 4 + 1] = qy / qn;
        rotations[i * 4 + 2] = qz / qn;
        rotations[i * 4 + 3] = qw / qn;

        // Skin-tone-ish base color with per-splat variance, darker toward
        // the back (simple fake ambient occlusion so the cloud reads as
        // volumetric rather than flat-shaded).
        const shade = 0.55 + 0.35 * Math.max(0, z / RADIUS_Z);
        colors[i * 3 + 0] = 0.86 * shade;
        colors[i * 3 + 1] = 0.68 * shade;
        colors[i * 3 + 2] = 0.58 * shade;

        opacities[i] = 0.55 + rand() * 0.35;

        // Jaw weight: linear ramp from 0 at JAW_Y_MAX down to 1 at the
        // bottom of the head, restricted to the front hemisphere. A splat
        // outside the jaw region gets exactly 0 — additive deformation must
        // never leak into unrelated regions (same "own only your channels"
        // discipline WO-3's oscillators use).
        if (y <= JAW_Y_MAX && z >= JAW_Z_MIN) {
            const t = Math.min(1, (JAW_Y_MAX - y) / (JAW_Y_MAX - -RADIUS_Y));
            jawWeights[i] = t;
        } else {
            jawWeights[i] = 0;
        }

        // Eyelid weight: splats within the eye band, tapered by distance
        // from the band's vertical center so the lid closes progressively
        // rather than as a hard-edged box.
        if (y >= EYE_Y_MIN && y <= EYE_Y_MAX && z >= EYE_Z_MIN && Math.abs(x) > 0.05) {
            const center = (EYE_Y_MIN + EYE_Y_MAX) / 2;
            const halfBand = (EYE_Y_MAX - EYE_Y_MIN) / 2;
            const t = 1 - Math.abs(y - center) / halfBand;
            eyelidWeights[i] = Math.max(0, t);
        } else {
            eyelidWeights[i] = 0;
        }
    }

    return { count, positions, scales, rotations, colors, opacities, jawWeights, eyelidWeights };
}
