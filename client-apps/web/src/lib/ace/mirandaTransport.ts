/**
 * WO-5 T2 — browser client for the WO-4 Rust `miranda-transport` server.
 *
 * This is deliberately a SEPARATE client module from `controllerClient.ts`,
 * not a replacement for it. Per the WO-5 T1 topology rewrite, `ace-controller`
 * (Node.js, "cloud-bridge" node) and `miranda-transport` (Rust, "transport"
 * node) are different services with different roles: `ace-controller` is
 * Pipeline 1's ASR/LLM/persona/stage HTTP API; `miranda-transport` is
 * Pipeline 2's binary blendshape+kinematic frame broadcast and JSON
 * telemetry. Rewiring `controllerClient.ts` to point at `miranda-transport`
 * would delete Pipeline 1 entirely, which is not what T2 asks for — both
 * pipelines are meant to coexist in this harness (see PROJECT_OVERVIEW.md's
 * "quad-test" requirement).
 *
 * # Endpoints (see `miranda-transport/src/server.rs`)
 *
 * - `GET /health` — JSON health check.
 * - `GET /data` — binary WebSocket, 312-byte MRD1 packets at 60 FPS.
 * - `GET /telemetry` — JSON WebSocket, `TelemetrySnapshot` at ~2 Hz.
 *
 * # The binary decode is intentionally hand-rolled, not table-driven off
 * this file's own array
 *
 * `client-apps/web/src/lib/stageMachine/types.ts`'s `ARKIT_CHANNELS` array
 * is in a DIFFERENT order than `miranda-core::arkit::CHANNEL_NAMES` (the
 * Rust canonical order every crate in this workspace is built against —
 * see `miranda-core/src/lib.rs`). Decoding the wire format against the
 * wrong array would silently move the wrong muscle on every single frame:
 * it would compile, pass any range check, and only be visible as a
 * subtly-wrong face. That is exactly the class of bug this project's CAT-5
 * tier exists to prevent. `ARKIT_CANONICAL_ORDER` below is copied verbatim
 * from `miranda-core::arkit::CHANNEL_NAMES` and is the ONLY array this
 * module's decoder is allowed to index against.
 */

// Copied verbatim from `miranda-core::arkit::CHANNEL_NAMES`
// (miranda-core/src/lib.rs). Index i here MUST equal index i there.
// Do not reorder, do not sort, do not "clean up" — this is a wire contract,
// not a display ordering.
export const ARKIT_CANONICAL_ORDER = [
    "eyeBlinkLeft",
    "eyeLookDownLeft",
    "eyeLookInLeft",
    "eyeLookOutLeft",
    "eyeLookUpLeft",
    "eyeSquintLeft",
    "eyeWideLeft",
    "eyeBlinkRight",
    "eyeLookDownRight",
    "eyeLookInRight",
    "eyeLookOutRight",
    "eyeLookUpRight",
    "eyeSquintRight",
    "eyeWideRight",
    "jawForward",
    "jawLeft",
    "jawRight",
    "jawOpen",
    "mouthClose",
    "mouthFunnel",
    "mouthPucker",
    "mouthLeft",
    "mouthRight",
    "mouthSmileLeft",
    "mouthSmileRight",
    "mouthFrownLeft",
    "mouthFrownRight",
    "mouthDimpleLeft",
    "mouthDimpleRight",
    "mouthStretchLeft",
    "mouthStretchRight",
    "mouthRollLower",
    "mouthRollUpper",
    "mouthShrugLower",
    "mouthShrugUpper",
    "mouthPressLeft",
    "mouthPressRight",
    "mouthLowerDownLeft",
    "mouthLowerDownRight",
    "mouthUpperUpLeft",
    "mouthUpperUpRight",
    "browDownLeft",
    "browDownRight",
    "browInnerUp",
    "browOuterUpLeft",
    "browOuterUpRight",
    "cheekPuff",
    "cheekSquintLeft",
    "cheekSquintRight",
    "noseSneerLeft",
    "noseSneerRight",
    "tongueOut",
] as const;

export const BLENDSHAPE_COUNT = ARKIT_CANONICAL_ORDER.length; // 52

/** Joint order for `KinematicTransformFrame`, copied verbatim from
 * `miranda-core::kinematic_joints::JOINT_NAMES`. */
export const KINEMATIC_JOINT_NAMES = ["head", "neck", "shoulderLeft", "shoulderRight"] as const;
export const KINEMATIC_JOINT_COUNT = KINEMATIC_JOINT_NAMES.length; // 4

// ---------------------------------------------------------------------------
// Wire layout constants — must match miranda-transport/src/frame.rs exactly.
// ---------------------------------------------------------------------------

/** `b"MRD1"` as four bytes, little-endian read as a u32 for a single compare. */
const MAGIC_BYTES = [0x4d, 0x52, 0x44, 0x31]; // 'M','R','D','1'

const BLENDSHAPE_FRAME_BYTES = 8 + BLENDSHAPE_COUNT * 4; // timestamp_us + 52 f32 = 216
const QUATERNION_BYTES = 16; // 4 f32
const KINEMATIC_FRAME_BYTES =
    8 + KINEMATIC_JOINT_COUNT * QUATERNION_BYTES + 4 + 4 + 8; // timestamp_us + joints + head_pitch_deg + clavicle_rise + reserved[8] = 88

/** Total packet size: 4 (magic) + 2 + 2 (size fields) + 216 + 88 = 312. */
export const PACKET_SIZE = 4 + 2 + 2 + BLENDSHAPE_FRAME_BYTES + KINEMATIC_FRAME_BYTES;

export interface DecodedKinematicJoint {
    x: number;
    y: number;
    z: number;
    w: number;
}

export interface DecodedFrame {
    timestampUs: number;
    /** ARKit-52 weights keyed by canonical camelCase name — safe to index
     * with the SAME `ArkitChannel` union `stageMachine/types.ts` exports,
     * since the string values (not array position) are what's shared. */
    weights: Record<string, number>;
    kinematicTimestampUs: number;
    joints: Record<(typeof KINEMATIC_JOINT_NAMES)[number], DecodedKinematicJoint>;
    headPitchDeg: number;
    clavicleRise: number;
}

/** Thrown when a binary payload fails to decode as an MRD1 packet. */
export class DecodeError extends Error {
    constructor(message: string) {
        super(`MRD1 decode failed: ${message}`);
        this.name = "DecodeError";
    }
}

/**
 * Decodes one 312-byte MRD1 packet into a structured frame.
 *
 * Throws {@link DecodeError} rather than returning `null` on malformed
 * input — a caller silently swallowing a decode failure would mean the
 * renderer keeps showing a stale frame with no signal that the wire format
 * has drifted from what this decoder expects. The render loop (T4) is
 * expected to catch this at the call site and count it as a dropped frame
 * in its own telemetry, not let it propagate into a crash.
 */
export function decodeFrame(buf: ArrayBuffer): DecodedFrame {
    if (buf.byteLength !== PACKET_SIZE) {
        throw new DecodeError(
            `expected ${PACKET_SIZE} bytes, got ${buf.byteLength}`,
        );
    }
    const view = new DataView(buf);

    for (let i = 0; i < 4; i += 1) {
        if (view.getUint8(i) !== MAGIC_BYTES[i]) {
            throw new DecodeError("bad magic bytes (expected MRD1)");
        }
    }

    const blendSz = view.getUint16(4, true);
    const kinSz = view.getUint16(6, true);
    if (blendSz !== BLENDSHAPE_FRAME_BYTES) {
        throw new DecodeError(
            `blendshape size field says ${blendSz}, expected ${BLENDSHAPE_FRAME_BYTES}`,
        );
    }
    if (kinSz !== KINEMATIC_FRAME_BYTES) {
        throw new DecodeError(
            `kinematic size field says ${kinSz}, expected ${KINEMATIC_FRAME_BYTES}`,
        );
    }

    let offset = 8;

    // --- BlendshapeFrame ---
    // timestamp_us: u64 LE. JS has no native u64; BigInt round-trips exactly,
    // then we narrow to Number for the ~microsecond-resolution timestamps
    // this project produces (safe well past any realistic session length —
    // Number.MAX_SAFE_INTEGER microseconds is over 285 years).
    const timestampUs = Number(view.getBigUint64(offset, true));
    offset += 8;

    const weights: Record<string, number> = {};
    for (let i = 0; i < BLENDSHAPE_COUNT; i += 1) {
        weights[ARKIT_CANONICAL_ORDER[i]] = view.getFloat32(offset, true);
        offset += 4;
    }

    // --- KinematicTransformFrame ---
    const kinematicTimestampUs = Number(view.getBigUint64(offset, true));
    offset += 8;

    const joints = {} as Record<(typeof KINEMATIC_JOINT_NAMES)[number], DecodedKinematicJoint>;
    for (const name of KINEMATIC_JOINT_NAMES) {
        joints[name] = {
            x: view.getFloat32(offset, true),
            y: view.getFloat32(offset + 4, true),
            z: view.getFloat32(offset + 8, true),
            w: view.getFloat32(offset + 12, true),
        };
        offset += QUATERNION_BYTES;
    }

    const headPitchDeg = view.getFloat32(offset, true);
    offset += 4;
    const clavicleRise = view.getFloat32(offset, true);
    offset += 4;
    // 8 reserved bytes, deliberately not read — see KinematicTransformFrame's
    // `_reserved` field docs in miranda-core: always zero on write, ignored
    // on read, expansion room for future fields.
    offset += 8;

    return { timestampUs, weights, kinematicTimestampUs, joints, headPitchDeg, clavicleRise };
}

// ---------------------------------------------------------------------------
// Telemetry (control plane)
// ---------------------------------------------------------------------------

/** Mirrors `miranda_transport::telemetry::TelemetrySnapshot`, camelCase per
 * `#[serde(rename_all = "camelCase")]` on the Rust side. */
export interface TelemetrySnapshot {
    tUs: number;
    framesPublished: number;
    framesDropped: number;
    lateFrames: number;
    publishFailures: number;
    meanBuildUs: number;
    maxBuildUs: number;
    audioChunksConsumed: number;
    dataSubscribers: number;
    telemetrySubscribers: number;
    framesBroadcast: number;
    framesDroppedBackpressure: number;
    circuitBreaker: "closed" | "halfopen" | "open";
}

// ---------------------------------------------------------------------------
// Connection management
// ---------------------------------------------------------------------------

export type TransportLinkState = "offline" | "connecting" | "live" | "error";

const DEFAULT_HTTP = "http://127.0.0.1:9090";
const DEFAULT_DATA_WS = "ws://127.0.0.1:9090/data";
const DEFAULT_TELEMETRY_WS = "ws://127.0.0.1:9090/telemetry";

function httpBase() {
    return (import.meta.env.VITE_TRANSPORT_HTTP_URL || DEFAULT_HTTP).replace(/\/$/, "");
}

function dataWsUrl() {
    return import.meta.env.VITE_TRANSPORT_DATA_WS_URL || DEFAULT_DATA_WS;
}

function telemetryWsUrl() {
    return import.meta.env.VITE_TRANSPORT_TELEMETRY_WS_URL || DEFAULT_TELEMETRY_WS;
}

type FrameHandler = (frame: DecodedFrame) => void;
type TelemetryHandler = (snap: TelemetrySnapshot) => void;
type LinkHandler = (state: TransportLinkState, detail?: string) => void;
/** Fired on a decode failure so the render loop can count it, never crash. */
type DecodeErrorHandler = (err: DecodeError) => void;

const RECONNECT_DELAY_MS = 1500;

/**
 * Client for `miranda-transport`'s two WebSocket endpoints.
 *
 * Deliberately does NOT drive any render loop itself — per the WO-5
 * architectural guardrail, the render loop (T4) must be fully decoupled
 * from network arrival. This class's only job is: connect, decode, hand
 * the decoded frame to whoever is listening. It never blocks, never awaits
 * a render, and reconnects on its own schedule regardless of whether
 * anyone is currently consuming frames.
 */
class MirandaTransportClient {
    private dataSocket: WebSocket | null = null;
    private telemetrySocket: WebSocket | null = null;
    private dataReconnectTimer: number | null = null;
    private telemetryReconnectTimer: number | null = null;
    private intentionalClose = false;

    private onFrame: FrameHandler | null = null;
    private onTelemetry: TelemetryHandler | null = null;
    private onLink: LinkHandler | null = null;
    private onDecodeError: DecodeErrorHandler | null = null;

    private dataLink: TransportLinkState = "offline";
    private telemetryLink: TransportLinkState = "offline";

    /** Most recently decoded frame, for callers that poll instead of subscribing. */
    private lastFrame: DecodedFrame | null = null;
    private lastFrameReceivedAtMs = 0;

    get dataLinkState() {
        return this.dataLink;
    }
    get telemetryLinkState() {
        return this.telemetryLink;
    }

    /** Last frame received and the wall-clock time (performance.now()) it
     * arrived at. The render loop uses this pair to extrapolate, never the
     * socket's own timing. */
    getLastFrame(): { frame: DecodedFrame | null; receivedAtMs: number } {
        return { frame: this.lastFrame, receivedAtMs: this.lastFrameReceivedAtMs };
    }

    configure(handlers: {
        onFrame?: FrameHandler;
        onTelemetry?: TelemetryHandler;
        onLink?: LinkHandler;
        onDecodeError?: DecodeErrorHandler;
    }) {
        this.onFrame = handlers.onFrame ?? null;
        this.onTelemetry = handlers.onTelemetry ?? null;
        this.onLink = handlers.onLink ?? null;
        this.onDecodeError = handlers.onDecodeError ?? null;
    }

    start() {
        this.intentionalClose = false;
        this.connectData();
        this.connectTelemetry();
    }

    stop() {
        this.intentionalClose = true;
        if (this.dataReconnectTimer != null) window.clearTimeout(this.dataReconnectTimer);
        if (this.telemetryReconnectTimer != null) window.clearTimeout(this.telemetryReconnectTimer);
        this.dataReconnectTimer = null;
        this.telemetryReconnectTimer = null;
        this.dataSocket?.close();
        this.telemetrySocket?.close();
        this.dataSocket = null;
        this.telemetrySocket = null;
        this.setDataLink("offline");
        this.setTelemetryLink("offline");
    }

    private setDataLink(state: TransportLinkState, detail?: string) {
        this.dataLink = state;
        this.onLink?.(state, detail);
    }
    private setTelemetryLink(state: TransportLinkState, detail?: string) {
        this.telemetryLink = state;
        this.onLink?.(state, detail);
    }

    private scheduleDataReconnect() {
        if (this.intentionalClose || this.dataReconnectTimer != null) return;
        this.dataReconnectTimer = window.setTimeout(() => {
            this.dataReconnectTimer = null;
            this.connectData();
        }, RECONNECT_DELAY_MS);
    }

    private scheduleTelemetryReconnect() {
        if (this.intentionalClose || this.telemetryReconnectTimer != null) return;
        this.telemetryReconnectTimer = window.setTimeout(() => {
            this.telemetryReconnectTimer = null;
            this.connectTelemetry();
        }, RECONNECT_DELAY_MS);
    }

    private connectData() {
        this.setDataLink("connecting");
        try {
            const ws = new WebSocket(dataWsUrl());
            ws.binaryType = "arraybuffer";
            this.dataSocket = ws;

            ws.onopen = () => this.setDataLink("live", "data channel open");

            ws.onmessage = (ev) => {
                if (!(ev.data instanceof ArrayBuffer)) return; // ignore stray text frames
                try {
                    const frame = decodeFrame(ev.data);
                    this.lastFrame = frame;
                    this.lastFrameReceivedAtMs = performance.now();
                    this.onFrame?.(frame);
                } catch (err) {
                    if (err instanceof DecodeError) {
                        this.onDecodeError?.(err);
                    }
                    // A single bad packet must not close the socket — that
                    // would turn one transient corruption into a full
                    // reconnect cycle. Drop it and keep listening.
                }
            };

            ws.onerror = () => this.setDataLink("error", "data websocket error");

            ws.onclose = () => {
                this.dataSocket = null;
                if (!this.intentionalClose) {
                    this.setDataLink("offline", "data ws closed");
                    this.scheduleDataReconnect();
                }
            };
        } catch (err) {
            const msg = err instanceof Error ? err.message : "data ws failed";
            this.setDataLink("error", msg);
            this.scheduleDataReconnect();
        }
    }

    private connectTelemetry() {
        this.setTelemetryLink("connecting");
        try {
            const ws = new WebSocket(telemetryWsUrl());
            this.telemetrySocket = ws;

            ws.onopen = () => this.setTelemetryLink("live", "telemetry channel open");

            ws.onmessage = (ev) => {
                try {
                    const snap = JSON.parse(String(ev.data)) as TelemetrySnapshot;
                    this.onTelemetry?.(snap);
                } catch {
                    /* malformed snapshot — ignore, next tick will arrive */
                }
            };

            ws.onerror = () => this.setTelemetryLink("error", "telemetry websocket error");

            ws.onclose = () => {
                this.telemetrySocket = null;
                if (!this.intentionalClose) {
                    this.setTelemetryLink("offline", "telemetry ws closed");
                    this.scheduleTelemetryReconnect();
                }
            };
        } catch (err) {
            const msg = err instanceof Error ? err.message : "telemetry ws failed";
            this.setTelemetryLink("error", msg);
            this.scheduleTelemetryReconnect();
        }
    }

    /** Plain health probe, independent of the WebSocket connections. */
    async health(): Promise<{ ok: boolean; dataSubscribers?: number; circuitBreaker?: string } | null> {
        try {
            const res = await fetch(`${httpBase()}/health`, { cache: "no-store" });
            if (!res.ok) return null;
            return await res.json();
        } catch {
            return null;
        }
    }
}

export const mirandaTransport = new MirandaTransportClient();
