/**
 * WO-5 T4 — the WebGPU Gaussian-splat renderer.
 *
 * # The decoupled render loop (WO-5 architectural guardrail)
 *
 * `requestAnimationFrame` in `startRenderLoop()` is completely independent
 * of the DataChannel/WebSocket message handler. The network side
 * (`mirandaTransport.ts`) only ever writes "the latest known frame" into a
 * small piece of shared state (via `getLastFrame()`); the render loop reads
 * whatever is currently there every single frame, regardless of whether a
 * new packet has arrived since the last one. If packets stop arriving —
 * dropped connection, a slow network, a paused Rust dispatcher — the render
 * loop keeps calling `requestAnimationFrame` and keeps rendering, holding
 * the last known pose (extrapolation is intentionally NOT implemented as
 * velocity-based dead reckoning here: with only position+rotation and no
 * velocity channel over the wire, extrapolating would require inventing
 * motion that was never actually commanded, which is worse than holding
 * still — WO-3's compositor already guarantees the SOURCE never sends a
 * static frame, so a held frame here is stale, not artificial).
 *
 * This is the actual, verifiable mechanism behind "the avatar must never
 * freeze because of a network stall": the render loop's control flow does
 * not go through the network callback at all.
 *
 * # Two-pass GPU pipeline
 *
 * See `shaders.wgsl.ts`'s module docs for the deform-compute + splat-render
 * pass structure. This class owns the buffers, uniform updates, and the
 * two `GPUCommandEncoder` passes per frame.
 */

import { buildPlaceholderSplat, type SplatData } from "./placeholderSplat";
import { DEFORM_SHADER, SPLAT_FRAGMENT_SHADER, SPLAT_VERTEX_SHADER } from "./shaders.wgsl";
import { ARKIT_CANONICAL_ORDER, BLENDSHAPE_COUNT, type DecodedFrame } from "@/lib/ace/mirandaTransport";

/** Number of vec4<f32> slots the packed FaceUniforms.weights array uses.
 * 52 channels / 4 per vec4, rounded up = 13. Matches shaders.wgsl.ts. */
const FACE_WEIGHTS_VEC4_COUNT = Math.ceil(BLENDSHAPE_COUNT / 4); // 13

/** Total FaceUniforms buffer size in bytes: 13 vec4s (208 bytes) + one
 * padding vec4 (16 bytes) = 224 bytes. WGSL uniform buffer struct members
 * must be 16-byte aligned; array<vec4<f32>, 13> already satisfies that
 * per-element, and the whole struct's size must be a multiple of its
 * largest member alignment (16), which 224 already is — the explicit
 * `_pad` field in the WGSL struct exists so this stays true if a field is
 * ever appended without recomputing this constant by hand. */
const FACE_UNIFORMS_BYTES = (FACE_WEIGHTS_VEC4_COUNT + 1) * 16; // 224

/** CameraUniforms buffer size: mat4x4<f32> (64 bytes) + 2 vec4 (32 bytes) = 96. */
const CAMERA_UNIFORMS_BYTES = 64 + 16 + 16;

export interface SplatRendererStats {
    framesRendered: number;
    lastFrameTimeMs: number;
    /** How many render frames had no new network data available at all
     * (never received a first frame) — distinct from "held a stale frame",
     * which is normal steady-state operation, not a fault. */
    framesWithNoDataYet: number;
    /** Milliseconds since the currently-held frame was received. Large
     * values indicate a stalled or slow network, surfaced for telemetry —
     * not used to change render behavior. */
    frameAgeMs: number;
}

/**
 * Owns the WebGPU device, pipelines, and buffers for one splat viewport.
 */
export class SplatRenderer {
    private device: GPUDevice;
    private context: GPUCanvasContext;
    private canvas: HTMLCanvasElement;
    private format: GPUTextureFormat;

    private splatData: SplatData;

    // Deform pass resources
    private deformPipeline: GPUComputePipeline;
    private deformBindGroup: GPUBindGroup;
    private faceUniformBuffer: GPUBuffer;
    private deformedPositionsBuffer: GPUBuffer;
    /**
     * Deformed rotation output from the compute pass. Not consumed by the
     * render pass: the billboard technique in SPLAT_VERTEX_SHADER doesn't
     * need per-splat rotation (see that shader's module note on why
     * binding it there would be a layout error, not just dead code). Kept
     * as a real GPU resource — exposed via `getDeformedRotationsBuffer()`
     * — because the natural next step, full covariance-to-screen-space
     * ellipse projection instead of a symmetric billboard, DOES need it.
     */
    private deformedRotationsBuffer: GPUBuffer;

    // Render pass resources.
    // Note: the scales/colors storage buffers are intentionally NOT held as
    // fields. They are referenced by `renderBindGroup`, which keeps them
    // alive for as long as the bind group exists — holding a second
    // reference here would imply this class mutates them per frame, which
    // it does not (they are rest-pose constants uploaded once).
    private renderPipeline: GPURenderPipeline;
    private renderBindGroup: GPUBindGroup;
    private cameraUniformBuffer: GPUBuffer;

    private stats: SplatRendererStats = {
        framesRendered: 0,
        lastFrameTimeMs: 0,
        framesWithNoDataYet: 0,
        frameAgeMs: 0,
    };

    private rafHandle: number | null = null;
    private stopped = false;

    /** Latest weights to apply, written by whoever is feeding this renderer
     * (the network client, or a test harness). Read every render frame
     * regardless of when it was last written — see the class docs on the
     * decoupled render loop. */
    private currentWeights: Float32Array = new Float32Array(BLENDSHAPE_COUNT);
    private hasReceivedAnyFrame = false;
    private lastFrameReceivedAtMs = 0;

    private constructor(
        device: GPUDevice,
        context: GPUCanvasContext,
        canvas: HTMLCanvasElement,
        format: GPUTextureFormat,
        splatData: SplatData,
    ) {
        this.device = device;
        this.context = context;
        this.canvas = canvas;
        this.format = format;
        this.splatData = splatData;

        const deform = this.buildDeformPass();
        this.deformPipeline = deform.pipeline;
        this.deformBindGroup = deform.bindGroup;
        this.faceUniformBuffer = deform.faceUniformBuffer;
        this.deformedPositionsBuffer = deform.deformedPositionsBuffer;
        this.deformedRotationsBuffer = deform.deformedRotationsBuffer;

        const render = this.buildRenderPass();
        this.renderPipeline = render.pipeline;
        this.renderBindGroup = render.bindGroup;
        this.cameraUniformBuffer = render.cameraUniformBuffer;
    }

    /**
     * Requests a GPU adapter/device and sets up the canvas context.
     *
     * Throws if WebGPU is unavailable — callers are expected to catch this
     * and fall back to the existing L0 CSS-compositor presence layer, which
     * is exactly why the renderer node is marked `requiredFrom: "L2"` and
     * `warmNodes` at every earlier stage in `aceTopology.ts`: L0/L1 must
     * work with zero WebGPU support.
     */
    static async create(
        canvas: HTMLCanvasElement,
        splatData: SplatData = buildPlaceholderSplat(),
    ): Promise<SplatRenderer> {
        if (!("gpu" in navigator)) {
            throw new Error("WebGPU is not available in this browser");
        }
        const adapter = await navigator.gpu.requestAdapter();
        if (!adapter) {
            throw new Error("No WebGPU adapter available");
        }
        const device = await adapter.requestDevice();
        const context = canvas.getContext("webgpu");
        if (!context) {
            throw new Error("Failed to acquire a webgpu canvas context");
        }
        const format = navigator.gpu.getPreferredCanvasFormat();
        context.configure({ device, format, alphaMode: "premultiplied" });

        // A <canvas> element's backing store defaults to 300x150 regardless
        // of its CSS-driven display size, which would render at that fixed
        // resolution stretched to fill the layout (blurry, wrong aspect).
        // Size the backing store from the actual displayed size, in device
        // pixels, before the first frame.
        resizeCanvasToDisplaySize(canvas);

        return new SplatRenderer(device, context, canvas, format, splatData);
    }

    private buildDeformPass() {
        const device = this.device;
        const count = this.splatData.count;

        const restPositions = toVec4Buffer(this.splatData.positions, count, 1.0);
        const restRotations = this.splatData.rotations; // already vec4-shaped

        const restPositionsBuffer = createStorageBuffer(device, restPositions, "restPositions");
        const restRotationsBuffer = createStorageBuffer(device, restRotations, "restRotations");
        const jawWeightsBuffer = createStorageBuffer(device, this.splatData.jawWeights, "jawWeights");
        const eyelidWeightsBuffer = createStorageBuffer(device, this.splatData.eyelidWeights, "eyelidWeights");

        const deformedPositionsBuffer = device.createBuffer({
            label: "deformedPositions",
            size: count * 16,
            usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_SRC,
        });
        const deformedRotationsBuffer = device.createBuffer({
            label: "deformedRotations",
            size: count * 16,
            usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_SRC,
        });

        const faceUniformBuffer = device.createBuffer({
            label: "faceUniforms",
            size: FACE_UNIFORMS_BYTES,
            usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
        });

        const module = device.createShaderModule({ label: "deform", code: DEFORM_SHADER });
        const pipeline = device.createComputePipeline({
            label: "deformPipeline",
            layout: "auto",
            compute: { module, entryPoint: "deform_main" },
        });

        const bindGroup = device.createBindGroup({
            label: "deformBindGroup",
            layout: pipeline.getBindGroupLayout(0),
            entries: [
                { binding: 0, resource: { buffer: faceUniformBuffer } },
                { binding: 1, resource: { buffer: restPositionsBuffer } },
                { binding: 2, resource: { buffer: restRotationsBuffer } },
                { binding: 3, resource: { buffer: jawWeightsBuffer } },
                { binding: 4, resource: { buffer: eyelidWeightsBuffer } },
                { binding: 5, resource: { buffer: deformedPositionsBuffer } },
                { binding: 6, resource: { buffer: deformedRotationsBuffer } },
            ],
        });

        return { pipeline, bindGroup, faceUniformBuffer, deformedPositionsBuffer, deformedRotationsBuffer };
    }

    private buildRenderPass() {
        const device = this.device;
        const count = this.splatData.count;

        const scales = toVec4Buffer(this.splatData.scales, count, 0.0);
        const colors = toVec4ColorBuffer(this.splatData.colors, this.splatData.opacities, count);

        const scalesBuffer = createStorageBuffer(device, scales, "scales");
        const colorsBuffer = createStorageBuffer(device, colors, "colors");

        const cameraUniformBuffer = device.createBuffer({
            label: "cameraUniforms",
            size: CAMERA_UNIFORMS_BYTES,
            usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
        });

        const vertexModule = device.createShaderModule({ label: "splatVertex", code: SPLAT_VERTEX_SHADER });
        const fragmentModule = device.createShaderModule({ label: "splatFragment", code: SPLAT_FRAGMENT_SHADER });

        const pipeline = device.createRenderPipeline({
            label: "splatRenderPipeline",
            layout: "auto",
            vertex: { module: vertexModule, entryPoint: "vertex_main" },
            fragment: {
                module: fragmentModule,
                entryPoint: "fragment_main",
                targets: [
                    {
                        format: this.format,
                        blend: {
                            color: { srcFactor: "src-alpha", dstFactor: "one-minus-src-alpha", operation: "add" },
                            alpha: { srcFactor: "one", dstFactor: "one-minus-src-alpha", operation: "add" },
                        },
                    },
                ],
            },
            primitive: { topology: "triangle-strip" },
        });

        const bindGroup = device.createBindGroup({
            label: "splatRenderBindGroup",
            layout: pipeline.getBindGroupLayout(0),
            entries: [
                { binding: 0, resource: { buffer: cameraUniformBuffer } },
                { binding: 1, resource: { buffer: this.deformedPositionsBuffer } },
                { binding: 2, resource: { buffer: scalesBuffer } },
                { binding: 3, resource: { buffer: colorsBuffer } },
            ],
        });

        return { pipeline, bindGroup, cameraUniformBuffer };
    }

    /**
     * Updates the target ARKit weights.
     *
     * Deliberately NOT wired to React state anywhere in this codebase.
     * `SplatViewport` calls this from its OWN `requestAnimationFrame` loop,
     * reading directly from `mirandaTransport.getLastFrame()` — never via a
     * `useState` setter. A `setState` call per network frame would force a
     * React re-render at up to 60 Hz, exactly what `EvePresenceViewport`'s
     * module docs identify as the thing to avoid on this project's
     * dual-core target. This method itself does not trigger a render
     * either; `renderFrame`/`startRenderLoop` read `currentWeights` on
     * their own schedule, which is the actual decoupling this class exists
     * to provide.
     */
    setWeights(frame: DecodedFrame, receivedAtMs: number) {
        // Skip the copy entirely if this is the same frame already applied —
        // a frame source polled every rAF tick will report the same
        // receivedAtMs many times between real network arrivals (60Hz
        // render loop vs. whatever rate frames actually arrive at).
        if (receivedAtMs === this.lastFrameReceivedAtMs) return;
        for (let i = 0; i < BLENDSHAPE_COUNT; i += 1) {
            this.currentWeights[i] = frame.weights[ARKIT_CANONICAL_ORDER[i]] ?? 0;
        }
        this.hasReceivedAnyFrame = true;
        this.lastFrameReceivedAtMs = receivedAtMs;
    }

    getStats(): SplatRendererStats {
        return { ...this.stats };
    }

    /** Exposes the deform pass's rotation output buffer, for a future
     * covariance-projection render pass or for debugging/verification
     * tooling. Not used by the current billboard render pass — see the
     * field's doc comment. */
    getDeformedRotationsBuffer(): GPUBuffer {
        return this.deformedRotationsBuffer;
    }

    /** Renders exactly one frame. Exposed separately from the loop so tests
     * (and the No-Loop verification harness, T6) can drive frames
     * deterministically without waiting on real rAF timing. */
    renderFrame() {
        const t0 = performance.now();

        // Cheap on every frame (two integer reads + comparisons); only
        // reconfigures the GPU context on the frames where the canvas's
        // display size has actually changed (e.g. a window resize or the
        // split-pane divider being dragged).
        if (resizeCanvasToDisplaySize(this.canvas)) {
            this.context.configure({ device: this.device, format: this.format, alphaMode: "premultiplied" });
        }

        if (!this.hasReceivedAnyFrame) {
            this.stats.framesWithNoDataYet += 1;
        }
        this.stats.frameAgeMs = this.hasReceivedAnyFrame
            ? performance.now() - this.lastFrameReceivedAtMs
            : 0;

        this.updateFaceUniforms();
        this.updateCameraUniforms();

        const encoder = this.device.createCommandEncoder({ label: "frameEncoder" });

        const computePass = encoder.beginComputePass({ label: "deformPass" });
        computePass.setPipeline(this.deformPipeline);
        computePass.setBindGroup(0, this.deformBindGroup);
        const workgroups = Math.ceil(this.splatData.count / 64);
        computePass.dispatchWorkgroups(workgroups);
        computePass.end();

        const view = this.context.getCurrentTexture().createView();
        const renderPass = encoder.beginRenderPass({
            label: "splatRenderPass",
            colorAttachments: [
                {
                    view,
                    clearValue: { r: 0.04, g: 0.04, b: 0.06, a: 1 },
                    loadOp: "clear",
                    storeOp: "store",
                },
            ],
        });
        renderPass.setPipeline(this.renderPipeline);
        renderPass.setBindGroup(0, this.renderBindGroup);
        renderPass.draw(4, this.splatData.count);
        renderPass.end();

        this.device.queue.submit([encoder.finish()]);

        this.stats.framesRendered += 1;
        this.stats.lastFrameTimeMs = performance.now() - t0;
    }

    private updateFaceUniforms() {
        const packed = new Float32Array(FACE_UNIFORMS_BYTES / 4);
        packed.set(this.currentWeights, 0);
        this.device.queue.writeBuffer(this.faceUniformBuffer, 0, packed);
    }

    private updateCameraUniforms() {
        const aspect = this.canvas.width / Math.max(1, this.canvas.height);
        const viewProj = buildSimpleViewProj(aspect);
        const buf = new Float32Array(CAMERA_UNIFORMS_BYTES / 4);
        buf.set(viewProj, 0); // 16 floats, mat4x4
        buf.set([1, 0, 0, 0], 16); // cameraRight
        buf.set([0, 1, 0, 0], 20); // cameraUp
        this.device.queue.writeBuffer(this.cameraUniformBuffer, 0, buf);
    }

    /**
     * Starts the decoupled render loop. Returns a stop function.
     *
     * This function is the entire mechanism satisfying the "render loop
     * must never freeze on network stall" guardrail: it calls
     * `requestAnimationFrame` unconditionally every frame and renders
     * whatever `currentWeights` currently holds, with no await, no message
     * wait, and no dependency on `setWeights` having been called recently.
     */
    startRenderLoop(): () => void {
        this.stopped = false;
        const loop = () => {
            if (this.stopped) return;
            this.renderFrame();
            this.rafHandle = requestAnimationFrame(loop);
        };
        this.rafHandle = requestAnimationFrame(loop);
        return () => this.stop();
    }

    stop() {
        this.stopped = true;
        if (this.rafHandle !== null) {
            cancelAnimationFrame(this.rafHandle);
            this.rafHandle = null;
        }
    }

    destroy() {
        this.stop();
        this.device.destroy();
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function createStorageBuffer(device: GPUDevice, data: Float32Array, label: string): GPUBuffer {
    const buffer = device.createBuffer({
        label,
        size: alignTo(data.byteLength, 4),
        usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST,
    });
    device.queue.writeBuffer(buffer, 0, data);
    return buffer;
}

function alignTo(size: number, align: number): number {
    return Math.ceil(size / align) * align;
}

/**
 * Sets a canvas's backing-store width/height to match its actual displayed
 * (CSS) size, in device pixels, if they differ. Returns true if a resize
 * happened, so callers know whether they need to reconfigure the GPU
 * context (required whenever the canvas's drawing buffer size changes).
 */
function resizeCanvasToDisplaySize(canvas: HTMLCanvasElement): boolean {
    const dpr = window.devicePixelRatio || 1;
    const displayWidth = Math.max(1, Math.round(canvas.clientWidth * dpr));
    const displayHeight = Math.max(1, Math.round(canvas.clientHeight * dpr));
    if (canvas.width === displayWidth && canvas.height === displayHeight) {
        return false;
    }
    canvas.width = displayWidth;
    canvas.height = displayHeight;
    return true;
}

/** Expands a tightly-packed xyz Float32Array into vec4-per-element layout
 * (WGSL storage buffers of `array<vec4<f32>>` require 16-byte stride per
 * element even though only 12 bytes are meaningful — this is the standard
 * "vec3 in a buffer is really vec4" rule every WGSL/std430-derived layout
 * has, and skipping it is exactly the class of buffer-alignment mistake
 * this Work Order's Rule 5 guardrail calls out). `w` is a caller-supplied
 * fill value (1.0 for positions so they're valid homogeneous points, 0.0
 * for scales where the 4th component is unused). */
function toVec4Buffer(xyz: Float32Array, count: number, w: number): Float32Array {
    const out = new Float32Array(count * 4);
    for (let i = 0; i < count; i += 1) {
        out[i * 4 + 0] = xyz[i * 3 + 0];
        out[i * 4 + 1] = xyz[i * 3 + 1];
        out[i * 4 + 2] = xyz[i * 3 + 2];
        out[i * 4 + 3] = w;
    }
    return out;
}

function toVec4ColorBuffer(rgb: Float32Array, opacity: Float32Array, count: number): Float32Array {
    const out = new Float32Array(count * 4);
    for (let i = 0; i < count; i += 1) {
        out[i * 4 + 0] = rgb[i * 3 + 0];
        out[i * 4 + 1] = rgb[i * 3 + 1];
        out[i * 4 + 2] = rgb[i * 3 + 2];
        out[i * 4 + 3] = opacity[i];
    }
    return out;
}

/**
 * A deliberately simple fixed camera: looks at the origin from a small
 * positive-z offset, orthographic-ish perspective. Real camera controls
 * (orbit, zoom) are out of scope for T4's shader-correctness goal — the
 * placeholder splat only needs to be visible and deformable, not
 * cinematically framed.
 */
function buildSimpleViewProj(aspect: number): Float32Array {
    const fovY = 0.8; // radians
    const near = 0.05;
    const far = 10;
    const f = 1 / Math.tan(fovY / 2);

    // Column-major, matching WGSL's mat4x4<f32> column-major convention.
    const proj = new Float32Array([
        f / aspect, 0, 0, 0,
        0, f, 0, 0,
        0, 0, far / (near - far), -1,
        0, 0, (far * near) / (near - far), 0,
    ]);

    // Simple lookAt from (0, 0.05, 1.4) toward the origin, y-up. With the
    // camera on the +z axis looking toward -z, view = translate(-eye) since
    // no rotation is needed (forward is already -z, up is already +y).
    const eye = [0, 0.05, 1.4];
    const view = new Float32Array([
        1, 0, 0, 0,
        0, 1, 0, 0,
        0, 0, 1, 0,
        -eye[0], -eye[1], -eye[2], 1,
    ]);

    return multiplyMat4(proj, view);
}

/** Column-major 4x4 matrix multiply: returns proj * view. */
function multiplyMat4(a: Float32Array, b: Float32Array): Float32Array {
    const out = new Float32Array(16);
    for (let col = 0; col < 4; col += 1) {
        for (let row = 0; row < 4; row += 1) {
            let sum = 0;
            for (let k = 0; k < 4; k += 1) {
                sum += a[k * 4 + row] * b[col * 4 + k];
            }
            out[col * 4 + row] = sum;
        }
    }
    return out;
}
