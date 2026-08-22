/**
 * WO-2 Pipeline 1, T3 — Amazon Transcribe Streaming bridge.
 *
 * Takes the in-process PCM buffer T2 accumulated (an AudioSession's
 * drained Float32Array) and streams it to Amazon Transcribe Streaming over
 * the AWS SDK's HTTP/2 connection. Emits partial results as they arrive
 * (drives EVE's "processing" micro-expression) and the final result once
 * Transcribe settles on stable text.
 *
 * Credentials: retrieved via the AMANDA vault MCP (`get_key("aws")`) at
 * call time, never hardcoded, never read from a committed .env value.
 */

/**
 * Converts a Float32Array of PCM samples in [-1, 1] into the 16-bit signed
 * little-endian PCM bytes Transcribe Streaming expects for `MediaEncoding:
 * 'pcm'`. Clamps out-of-range samples rather than wrapping, since a wrapped
 * sample is audibly worse (a loud click) than a clamped one (brief clipping).
 */
export function float32ToPcm16(samples) {
  const out = Buffer.alloc(samples.length * 2);
  for (let i = 0; i < samples.length; i++) {
    const clamped = Math.max(-1, Math.min(1, samples[i]));
    const intSample = Math.round(clamped * 32767);
    out.writeInt16LE(intSample, i * 2);
  }
  return out;
}

/**
 * Wraps a single Float32Array buffer as the one-shot AsyncIterable the AWS
 * SDK's `AudioStream` parameter expects. Real streaming (many chunks over
 * time) would yield multiple times; T2's current buffering model hands us
 * one complete utterance at speech-end, so one yield is correct here.
 */
async function* singleChunkAudioStream(pcm16) {
  yield { AudioEvent: { AudioChunk: pcm16 } };
}

/**
 * Runs one Transcribe Streaming session against a single buffered
 * utterance. Calls `onPartial(text)` for every `IsPartial: true` result and
 * resolves with the final stable transcript once received.
 *
 * `clientFactory` and `commandFactory` are injectable so tests can supply a
 * mock without needing real AWS credentials or network access — the
 * production caller (see wireAudioPipeline in run.mjs, T4) passes the real
 * AWS SDK classes.
 */
export async function transcribeUtterance({
  samples,
  sampleRateHz,
  onPartial,
  TranscribeStreamingClient,
  StartStreamTranscriptionCommand,
  clientConfig,
}) {
  if (samples.length === 0) {
    return { text: "", partialCount: 0 };
  }

  const client = new TranscribeStreamingClient(clientConfig ?? { region: "us-east-1" });
  const pcm16 = float32ToPcm16(samples);

  const command = new StartStreamTranscriptionCommand({
    LanguageCode: "en-US",
    MediaSampleRateHertz: sampleRateHz,
    MediaEncoding: "pcm",
    AudioStream: singleChunkAudioStream(pcm16),
    EnablePartialResultsStabilization: true,
    PartialResultsStability: "medium",
  });

  const response = await client.send(command);
  let finalText = "";
  let partialCount = 0;

  for await (const event of response.TranscriptResultStream ?? []) {
    const results = event.TranscriptEvent?.Transcript?.Results ?? [];
    for (const result of results) {
      const text = result.Alternatives?.[0]?.Transcript ?? "";
      if (result.IsPartial) {
        partialCount += 1;
        onPartial?.(text);
      } else {
        finalText = text;
      }
    }
  }

  return { text: finalText, partialCount };
}

/**
 * Per-connection re-entrancy guard: WO-2 REQ says a new speech-start while
 * a Transcribe stream is active for that session must close the current
 * stream gracefully and open a new one, not silently interleave two.
 *
 * Since `transcribeUtterance` above is a single-shot call over one
 * complete buffered utterance (not a genuinely long-lived stream), "close
 * gracefully" here means: abandon interest in the in-flight call's result
 * rather than awaiting/broadcasting it, and let a new call start
 * immediately. `TranscribeSessionGuard` tracks an increasing generation
 * number per session id so a stale in-flight call's result is detected and
 * dropped instead of racing a newer one to the broadcast.
 */
export class TranscribeSessionGuard {
  constructor() {
    /** @type {Map<string, number>} sessionId -> current generation */
    this.generations = new Map();
  }

  /** Call when starting a new utterance for this session; returns a token
   * to check against later. */
  begin(sessionId) {
    const next = (this.generations.get(sessionId) ?? 0) + 1;
    this.generations.set(sessionId, next);
    return next;
  }

  /** True if `token` is still the most recent generation for this session
   * — i.e. no newer speech-start has superseded this call. */
  isCurrent(sessionId, token) {
    return this.generations.get(sessionId) === token;
  }
}
