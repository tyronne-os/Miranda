/**
 * WO-2 Pipeline 1, T2 — browser audio session state.
 *
 * Holds each connected browser's PCM audio entirely in this process's
 * memory, the same pattern run.mjs already uses for eveChat/nodeChats.
 * Deliberately does NOT touch /dev/shm/miranda_bus — Pipeline 1's audio
 * never needs to cross into the WO-1 ring buffer. See
 * .kiro/specs/wo2-acoustic-ingress-routing/tasks.md, "Architectural
 * resolution #2" for the reasoning: that ring buffer is WO-1's contract for
 * the Rust-native Pipeline 2 path (native cpal capture), not for browser
 * audio arriving over a WebSocket.
 *
 * Expected wire format: binary WebSocket frames, each frame a
 * little-endian Float32Array of PCM samples at 16 kHz mono. The browser
 * side (client-apps/web) sends 160-sample (10 ms) frames to match WO-1's
 * AudioChunk.samples size, but this module does not assume a fixed frame
 * size — it just accumulates whatever arrives until a caller asks for the
 * buffered audio (T3 will do this on speech-end).
 */

/** One browser connection's accumulated audio state. */
class AudioSession {
  constructor(id) {
    this.id = id;
    /** @type {Float32Array[]} raw frames, oldest first */
    this.frames = [];
    this.totalSamples = 0;
    this.createdAt = Date.now();
    this.lastFrameAt = null;
  }

  /**
   * Accepts one binary WS frame (a Node Buffer) and appends it as PCM.
   * Returns the number of samples in this frame, for logging/telemetry.
   */
  pushFrame(buf) {
    if (buf.length % 4 !== 0) {
      throw new Error(
        `audio frame length ${buf.length} is not a multiple of 4 bytes (Float32)`,
      );
    }
    // Float32Array requires its buffer's byteOffset to be 4-byte aligned.
    // A Buffer view into a larger pooled allocation may not be — copy the
    // bytes out to guarantee correct alignment rather than assume it.
    const aligned = buf.byteOffset % 4 === 0 ? buf : Buffer.from(buf);
    const samples = new Float32Array(
      aligned.buffer,
      aligned.byteOffset,
      aligned.length / 4,
    );
    this.frames.push(samples);
    this.totalSamples += samples.length;
    this.lastFrameAt = Date.now();
    return samples.length;
  }

  /** Concatenates all buffered frames into one Float32Array and clears them. */
  drain() {
    const out = new Float32Array(this.totalSamples);
    let offset = 0;
    for (const frame of this.frames) {
      out.set(frame, offset);
      offset += frame.length;
    }
    this.frames = [];
    this.totalSamples = 0;
    return out;
  }

  /** Non-destructive peek at the current buffered sample count. */
  bufferedSamples() {
    return this.totalSamples;
  }
}

/** Registry of active audio sessions, keyed by an opaque connection id. */
export class AudioSessionRegistry {
  constructor() {
    /** @type {Map<string, AudioSession>} */
    this.sessions = new Map();
  }

  getOrCreate(id) {
    let session = this.sessions.get(id);
    if (!session) {
      session = new AudioSession(id);
      this.sessions.set(id, session);
    }
    return session;
  }

  get(id) {
    return this.sessions.get(id);
  }

  remove(id) {
    this.sessions.delete(id);
  }
}
