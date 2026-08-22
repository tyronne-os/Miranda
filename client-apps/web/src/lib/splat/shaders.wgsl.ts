/**
 * WO-5 T4 — WGSL shaders for the Gaussian-splat viewport.
 *
 * Kept as template-literal exports in a .ts file rather than standalone
 * .wgsl files: Vite's default config in this project has no WGSL loader
 * configured, and adding one is more surface area than this Work Order
 * needs. `?raw` imports would work too, but a plain exported string keeps
 * this dependency-free.
 *
 * # Two-pass structure
 *
 * 1. **Deform compute pass** (`DEFORM_SHADER`): one thread per Gaussian.
 *    Reads the rest-pose position/rotation plus per-splat jaw/eyelid
 *    weights, reads the CURRENT interpolated ARKit weights from a uniform
 *    buffer, and writes deformed position + rotation to a storage buffer.
 *    This is where "jawOpen weight 0.6" becomes "this splat moved down and
 *    back by X". Separated from rendering so deformation cost is paid once
 *    per Gaussian per frame, not once per Gaussian per triangle/vertex.
 *
 * 2. **Splat render pass** (`SPLAT_VERTEX_SHADER` + `SPLAT_FRAGMENT_SHADER`):
 *    instanced quads, 4 vertices per instance, one instance per Gaussian.
 *    Each quad is billboarded to face the camera and scaled/rotated per the
 *    Gaussian's covariance so it reads as a soft ellipsoid, not a hard
 *    square — the projected-Gaussian-as-billboard technique used by every
 *    real-time splat renderer (this is deliberately the simple, well-
 *    understood approach: a full 3D covariance projection with proper
 *    depth sorting is the natural next step once a real trained asset with
 *    real depth complexity exists to justify it — see the module docs on
 *    why a placeholder splat doesn't need that yet).
 */

/**
 * Deformation compute shader.
 *
 * Workgroup size 64: chosen because it divides evenly into common splat
 * counts without excessive tail-thread waste, and comfortably fits under
 * this hardware's measured `maxComputeInvocationsPerWorkgroup` of 256 with
 * headroom for larger workgroups later if a real asset needs more per-
 * thread registers. Not a guess — verified against this project's actual
 * WebGPU adapter limits before being chosen (see WO-5 T4 verification
 * notes: real hardware probe returned maxComputeWorkgroupSizeX=256,
 * maxComputeInvocationsPerWorkgroup=256).
 */
export const DEFORM_SHADER = /* wgsl */ `
struct FaceUniforms {
    // ARKit-52 weights, one f32 per channel. Indices match
    // miranda-core::arkit::CHANNEL_NAMES exactly (see
    // client-apps/web/src/lib/ace/mirandaTransport.ts's ARKIT_CANONICAL_ORDER
    // — this buffer's layout is a direct extension of that same contract).
    // vec4 packing (52 -> 13 vec4s = 208 bytes) keeps this uniform buffer
    // trivially under the 65536-byte maxUniformBufferBindingSize this
    // hardware reports, with room to spare for future channels.
    weights: array<vec4<f32>, 13>,
    // Jaw-open weight is read out of the packed array at shader-build time
    // via a helper below, not duplicated here — one source of truth.
    _pad: vec4<f32>,
};

const JAW_OPEN_INDEX: u32 = 17u;
const EYE_BLINK_LEFT_INDEX: u32 = 0u;
const EYE_BLINK_RIGHT_INDEX: u32 = 7u;

fn weight_at(u: FaceUniforms, index: u32) -> f32 {
    let vec_index = index / 4u;
    let component = index % 4u;
    let v = u.weights[vec_index];
    if (component == 0u) { return v.x; }
    if (component == 1u) { return v.y; }
    if (component == 2u) { return v.z; }
    return v.w;
}

@group(0) @binding(0) var<uniform> face: FaceUniforms;
@group(0) @binding(1) var<storage, read> restPositions: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> restRotations: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> jawWeights: array<f32>;
@group(0) @binding(4) var<storage, read> eyelidWeights: array<f32>;
@group(0) @binding(5) var<storage, read_write> outPositions: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read_write> outRotations: array<vec4<f32>>;

// Quaternion multiply — used to compose the rest-pose rotation with the
// small deformation rotation, in that order (deformation applied in the
// splat's local frame, not world space).
fn qmul(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(
        a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
        a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
        a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
        a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
    );
}

fn quat_from_angle_x(radians: f32) -> vec4<f32> {
    let half = radians * 0.5;
    return vec4<f32>(sin(half), 0.0, 0.0, cos(half));
}

@compute @workgroup_size(64)
fn deform_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= arrayLength(&restPositions)) {
        return;
    }

    let restPos = restPositions[i].xyz;
    let restRot = restRotations[i];
    let jawW = jawWeights[i];
    let lidW = eyelidWeights[i];

    let jawOpen = weight_at(face, JAW_OPEN_INDEX);
    let blinkL = weight_at(face, EYE_BLINK_LEFT_INDEX);
    let blinkR = weight_at(face, EYE_BLINK_RIGHT_INDEX);
    // Eyelid closes for whichever side is nearer — approximated here with
    // the max of the two channels since the placeholder splat doesn't
    // distinguish left/right eyelid regions (see placeholderSplat.ts).
    let blink = max(blinkL, blinkR);

    // Jaw deformation: rotate downward and pull back slightly around a
    // hinge point near the top of the jaw region, scaled by this splat's
    // jaw weight AND the live jawOpen value. 0.35 rad (~20 deg) is the
    // maximum jaw rotation at jawOpen=1.0 — chosen to be visually obvious
    // on the placeholder without the jaw region detaching from the face.
    let jawAngle = jawOpen * jawW * 0.35;
    let hingeY = -0.18; // matches placeholderSplat.ts's JAW_Y_MAX
    var pos = restPos;
    pos.y -= hingeY;
    let jawRot = quat_from_angle_x(jawAngle);
    // Rotating a position by a quaternion: p' = q * p * q^-1, implemented
    // via the standard cross-product expansion (avoids a second qmul with
    // a vector-as-quaternion, which is the more common but slower form).
    let qv = jawRot.xyz;
    let qw = jawRot.w;
    let t = 2.0 * cross(qv, pos);
    pos = pos + qw * t + cross(qv, t);
    pos.y += hingeY;
    // Eyelid closure: collapse this splat's y toward a fixed lid line
    // (0.12, matching placeholderSplat.ts's EYE_Y_MIN/EYE_Y_MAX midpoint)
    // proportional to blink * this splat's eyelid weight. Only touches
    // splats with nonzero lidW - the jaw rotation above already applied to
    // pos unconditionally, but jawW is 0 outside the jaw region so it has
    // no effect there, and the two deformations never target the same
    // splats (placeholderSplat.ts's jaw and eyelid regions don't overlap).
    if (lidW > 0.0) {
        let closeAmount = blink * lidW;
        pos.y = mix(restPos.y, 0.12, closeAmount);
    }

    outPositions[i] = vec4<f32>(pos, 1.0);
    outRotations[i] = qmul(jawRot, restRot);
}
`;

/**
 * Splat vertex shader — billboard quad per instance, positioned and scaled
 * from the deformed Gaussian data.
 *
 * `@builtin(vertex_index) % 4u` walks a fixed unit quad (two triangles via
 * a triangle-strip-equivalent index pattern) so no separate index buffer is
 * needed — 4 vertices per instance, drawn with `draw(4, splatCount)`.
 */
export const SPLAT_VERTEX_SHADER = /* wgsl */ `
struct CameraUniforms {
    viewProj: mat4x4<f32>,
    // Camera right/up in world space, for billboard construction. Storing
    // these directly (rather than deriving from the view matrix in-shader)
    // is one fewer matrix inverse per vertex across thousands of splats.
    cameraRight: vec4<f32>,
    cameraUp: vec4<f32>,
};

// NOTE: per-splat rotation is deliberately NOT bound here even though the
// deform pass computes it (see DEFORM_SHADER's outRotations). WGSL's
// layout:"auto" bind-group-layout derivation only includes bindings that
// are actually READ in the entry point's reachable code, not merely
// declared — a variable declared but unused is silently dropped from the
// auto layout, and binding a buffer to a dropped slot from the JS side is
// a validation error ("binding index N not present in the bind group
// layout"). This billboard technique (see module docs: simple projected-
// Gaussian-as-billboard, not full covariance projection) does not need
// per-splat rotation for a symmetric quad, so it is correctly left out
// rather than bound-but-unread, which would have hidden this exact
// footgun instead of avoiding it.
@group(0) @binding(0) var<uniform> camera: CameraUniforms;
@group(0) @binding(1) var<storage, read> positions: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> scales: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> colors: array<vec4<f32>>;

struct VertexOutput {
    @builtin(position) clipPosition: vec4<f32>,
    @location(0) color: vec4<f32>,
    // Local quad coordinate in [-1, 1]^2, used by the fragment shader to
    // evaluate the Gaussian falloff — this is what turns a flat square into
    // a soft ellipse instead of a hard-edged box.
    @location(1) localUv: vec2<f32>,
};

const QUAD_OFFSETS = array<vec2<f32>, 4>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>(1.0, -1.0),
    vec2<f32>(-1.0, 1.0),
    vec2<f32>(1.0, 1.0),
);

@vertex
fn vertex_main(
    @builtin(vertex_index) vertexIndex: u32,
    @builtin(instance_index) instanceIndex: u32,
) -> VertexOutput {
    let center = positions[instanceIndex].xyz;
    let scale = scales[instanceIndex].xyz;
    let color = colors[instanceIndex];

    let offset = QUAD_OFFSETS[vertexIndex % 4u];
    // Use the larger of the x/y/z scale components as the billboard's
    // radius — an exact per-axis-projected ellipse needs the full
    // covariance-to-screen-space projection this simple billboard approach
    // deliberately skips (see module docs); this is a defensible
    // approximation for a placeholder asset, not the final technique.
    let radius = max(scale.x, max(scale.y, scale.z)) * 2.2;

    let worldOffset = camera.cameraRight.xyz * offset.x * radius
                     + camera.cameraUp.xyz * offset.y * radius;
    let worldPos = center + worldOffset;

    var out: VertexOutput;
    out.clipPosition = camera.viewProj * vec4<f32>(worldPos, 1.0);
    out.color = color;
    out.localUv = offset;
    return out;
}
`;

/**
 * Splat fragment shader — evaluates a 2D Gaussian falloff over the quad and
 * discards (via alpha) outside a soft radius, so the billboard reads as an
 * ellipsoid blob rather than a square. Alpha-blended, not depth-tested for
 * opacity, matching how every real splat renderer composites overlapping
 * Gaussians.
 */
export const SPLAT_FRAGMENT_SHADER = /* wgsl */ `
struct FragmentInput {
    @location(0) color: vec4<f32>,
    @location(1) localUv: vec2<f32>,
};

@fragment
fn fragment_main(in: FragmentInput) -> @location(0) vec4<f32> {
    let distSq = dot(in.localUv, in.localUv);
    // Gaussian falloff: exp(-distSq * k). k=4 gives a soft-but-defined edge
    // within the quad's [-1,1] extent rather than a barely-visible smear.
    let falloff = exp(-distSq * 4.0);
    if (falloff < 0.02) {
        discard;
    }
    return vec4<f32>(in.color.rgb, in.color.a * falloff);
}
`;
