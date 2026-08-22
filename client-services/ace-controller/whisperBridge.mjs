/**
 * WO-2 Pipeline 1, T3 pivot — OpenAI Whisper ASR bridge.
 *
 * STRATEGIC PIVOT (Rule 5): Amazon Transcribe Streaming is blocked on this
 * AWS account (UnrecognizedClientException across every payload size,
 * reproduced directly against AWS outside this server — not a code
 * defect, an account-level credential/trust-review issue). transcribeBridge.mjs
 * is left completely intact for reactivation once that clears. This module
 * fills the same "Riva ASR" role slot with OpenAI's Whisper REST API
 * instead, using the vault's verified `openai` key.
 *
 * Real tradeoff, stated plainly: Whisper's `/v1/audio/transcriptions`
 * endpoint is a single-shot REST call over a complete audio buffer, not a
 * streaming protocol. There is no partial-result equivalent here — WO-2
 * REQ-4's partial/final distinction (drives EVE's "processing"
 * micro-expression mid-utterance) does not exist on this path. Only a
 * final transcript is ever produced. This is an explicit, accepted
 * regression versus the original Transcribe-based design, not an
 * oversight — restoring partial results would mean either AWS unblocking
 * or adopting OpenAI's separate Realtime (WebSocket) API, a larger change.
 */

const WHISPER_ENDPOINT = "https://api.openai.com/v1/audio/transcriptions";
const WHISPER_MODEL = "whisper-1";

/**
 * Wraps 16-bit PCM samples in a minimal WAV container. Whisper's REST
 * endpoint requires a real audio file format, not a raw PCM byte stream —
 * unlike Transcribe Streaming, which accepts raw PCM directly.
 */
function pcm16ToWav(pcm16, sampleRateHz) {
  const header = Buffer.alloc(44);
  header.write("RIFF", 0);
  header.writeUInt32LE(36 + pcm16.length, 4);
  header.write("WAVE", 8);
  header.write("fmt ", 12);
  header.writeUInt32LE(16, 16); // fmt chunk size
  header.writeUInt16LE(1, 20); // PCM format
  header.writeUInt16LE(1, 22); // mono
  header.writeUInt32LE(sampleRateHz, 24);
  header.writeUInt32LE(sampleRateHz * 2, 28); // byte rate (mono, 16-bit)
  header.writeUInt16LE(2, 32); // block align
  header.writeUInt16LE(16, 34); // bits per sample
  header.write("data", 36);
  header.writeUInt32LE(pcm16.length, 40);
  return Buffer.concat([header, pcm16]);
}

/**
 * Converts Float32 PCM samples in [-1, 1] to 16-bit signed LE PCM.
 * Duplicated from transcribeBridge.mjs's float32ToPcm16 deliberately
 * rather than imported — this module must stand alone as a real
 * alternative implementation, not a thin wrapper coupled to the AWS
 * bridge's internals, since the two may need to diverge independently
 * once AWS unblocks and both paths run side by side in a quad-test.
 */
function float32ToPcm16(samples) {
  const out = Buffer.alloc(samples.length * 2);
  for (let i = 0; i < samples.length; i++) {
    const clamped = Math.max(-1, Math.min(1, samples[i]));
    out.writeInt16LE(Math.round(clamped * 32767), i * 2);
  }
  return out;
}

/**
 * Transcribes one complete buffered utterance via OpenAI Whisper.
 *
 * No `onPartial` callback — see the module-level tradeoff note above.
 * `apiKey` and `fetchImpl`/`FormDataImpl`/`BlobImpl` are all injectable so
 * this is testable without real network access or a real OpenAI key,
 * same dependency-injection pattern as transcribeBridge.mjs.
 */
export async function transcribeUtteranceWhisper({
  samples,
  sampleRateHz,
  apiKey,
  fetchImpl = fetch,
  FormDataImpl = FormData,
  BlobImpl = Blob,
  endpoint = WHISPER_ENDPOINT,
  model = WHISPER_MODEL,
}) {
  if (samples.length === 0) {
    return { text: "", partialCount: 0 };
  }
  if (!apiKey) {
    throw new Error("OpenAI API key required for Whisper transcription");
  }

  const wav = pcm16ToWav(float32ToPcm16(samples), sampleRateHz);

  const form = new FormDataImpl();
  form.append("file", new BlobImpl([wav], { type: "audio/wav" }), "utterance.wav");
  form.append("model", model);

  const res = await fetchImpl(endpoint, {
    method: "POST",
    headers: { Authorization: `Bearer ${apiKey}` },
    body: form,
  });

  if (!res.ok) {
    const body = await res.text().catch(() => "");
    throw new Error(`Whisper request failed: HTTP ${res.status} ${body.slice(0, 200)}`);
  }

  const data = await res.json();
  // partialCount is always 0 here — kept in the return shape so this
  // function is drop-in compatible with transcribeUtterance()'s result
  // shape at the call site in run.mjs.
  return { text: data.text ?? "", partialCount: 0 };
}
