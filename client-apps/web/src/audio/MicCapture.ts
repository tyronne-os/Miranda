// WO-2 Pipeline 1 — browser-side microphone capture via getUserMedia().
//
// Captures 16 kHz mono PCM and streams it over the existing THE VANITY
// WebSocket (ace-controller's /ws) as binary frames. ace-controller holds
// the audio in its own process memory (same pattern as its existing
// eveChat/nodeChats session state) and bridges it to Amazon Transcribe
// Streaming — see .kiro/specs/wo2-acoustic-ingress-routing/tasks.md T2/T3.
//
// Architecture note: under Pipeline 1, the browser NEVER calls AWS SDKs
// directly (no SigV4 credentials in client JS) and NEVER touches POSIX
// shared memory (/dev/shm) — both are impossible from inside the V8
// sandbox, and neither is actually needed here. Pipeline 1 stays entirely
// in-process on the ace-controller side. Only Pipeline 2 (native cpal
// capture in miranda-audio) writes into the WO-1 /dev/shm ring buffer.

import { aceController } from "@/lib/ace/controllerClient";

/** Samples per outgoing frame — matches WO-1's AudioChunk.samples (10 ms @ 16 kHz). */
const FRAME_SAMPLES = 160;
const SAMPLE_RATE_HZ = 16_000;

export class MicCapture {
    private stream: MediaStream | null = null;
    private ctx: AudioContext | null = null;
    private source: MediaStreamAudioSourceNode | null = null;
    private processor: ScriptProcessorNode | null = null;
    /** Carries leftover samples between onaudioprocess calls when the
     * browser's buffer size isn't a multiple of FRAME_SAMPLES. */
    private residual: Float32Array = new Float32Array(0);

    get isCapturing() {
        return this.stream !== null;
    }

    /**
     * Requests mic access and starts streaming 160-sample PCM frames to
     * ace-controller. Throws if getUserMedia is denied or unavailable —
     * callers must handle that (e.g. show a permission-denied UI state).
     */
    async start(): Promise<void> {
        if (this.stream) return; // already capturing

        this.stream = await navigator.mediaDevices.getUserMedia({
            audio: {
                sampleRate: SAMPLE_RATE_HZ,
                channelCount: 1,
                echoCancellation: true,
            },
        });

        // AudioContext's actual sampleRate is a hint, not a guarantee — some
        // browsers ignore the requested rate. We do not resample here (out
        // of scope for T2); Transcribe's bridge (T3) assumes 16 kHz, so if a
        // browser silently resamples, this is a manual-verification issue
        // to catch during T4's real-mic check, not something to paper over
        // with a fake success path here.
        this.ctx = new AudioContext({ sampleRate: SAMPLE_RATE_HZ });
        this.source = this.ctx.createMediaStreamSource(this.stream);
        // 1024-sample buffer: smallest widely-supported ScriptProcessor size
        // that doesn't glitch under normal scheduling load. We re-slice it
        // into FRAME_SAMPLES (160) chunks before sending.
        this.processor = this.ctx.createScriptProcessor(1024, 1, 1);
        this.source.connect(this.processor);
        // ScriptProcessorNode requires being connected to a destination to
        // actually fire onaudioprocess in most browsers, even though we
        // don't want to play the mic input back out loud.
        this.processor.connect(this.ctx.destination);

        this.processor.onaudioprocess = (event: AudioProcessingEvent) => {
            const pcm = event.inputBuffer.getChannelData(0);
            this.emitFrames(pcm);
        };
    }

    /** Stops capture and releases the mic. Safe to call even if not started. */
    stop(): void {
        this.processor?.disconnect();
        this.source?.disconnect();
        this.stream?.getTracks().forEach((track) => track.stop());
        void this.ctx?.close();
        this.processor = null;
        this.source = null;
        this.stream = null;
        this.ctx = null;
        this.residual = new Float32Array(0);
    }

    /** Tells ace-controller the current utterance is finished (per real VAD, not this class). */
    signalSpeechEnd(): void {
        aceController.signalSpeechEnd();
    }

    /**
     * Slices an incoming ScriptProcessor buffer into fixed FRAME_SAMPLES
     * chunks (carrying any remainder to the next call) and sends each as a
     * binary WebSocket frame via the shared ace-controller client.
     */
    private emitFrames(pcm: Float32Array): void {
        const combined = new Float32Array(this.residual.length + pcm.length);
        combined.set(this.residual, 0);
        combined.set(pcm, this.residual.length);

        let offset = 0;
        while (offset + FRAME_SAMPLES <= combined.length) {
            const frame = combined.slice(offset, offset + FRAME_SAMPLES);
            aceController.sendAudioFrame(frame);
            offset += FRAME_SAMPLES;
        }
        this.residual = combined.slice(offset);
    }
}

export const micCapture = new MicCapture();
