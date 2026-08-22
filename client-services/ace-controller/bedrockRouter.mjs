/**
 * WO-2 Pipeline 1, T4 — Amazon Bedrock Converse routing.
 *
 * Takes the final transcript from transcribeBridge.mjs (T3) and calls
 * Amazon Bedrock's Converse API to generate EVE's response text. Model ID
 * defaults to amazon.nova-pro-v1:0 per the CAT-5 protocol's Pipeline 1
 * routing table — this is a data-plane model choice (which LLM answers
 * EVE's conversation), unrelated to which model is driving this Kiro
 * session.
 *
 * Credentials: retrieved via the AMANDA vault MCP (`get_key("aws")`) at
 * call time, never hardcoded. Same pattern as transcribeBridge.mjs.
 */

export const DEFAULT_MODEL_ID = "amazon.nova-pro-v1:0";

export const SUPERVISOR_SYSTEM_PROMPT = `You are EVE, a real-time digital human companion.
Respond in 1-2 sentences. Be warm, present, and conversational.
Never describe your own actions. Never say "As an AI."`;

/**
 * Calls Bedrock's Converse API with one transcript turn and returns the
 * response text. `client` is injected (not constructed internally) so
 * tests can supply a mock without real AWS credentials — the production
 * caller in run.mjs constructs a real `BedrockRuntimeClient`.
 *
 * Kept as a plain function taking a pre-built client, rather than a class
 * that also owns client construction, so T7's later Nemotron-Flash swap
 * (a different provider entirely) doesn't need to inherit or fight this
 * class's AWS-specific constructor — see the "swap points" design goal in
 * pipeline-1-aws-native.md.
 */
export async function routeToConverse({ client, ConverseCommand, transcript, modelId }) {
  const trimmed = String(transcript ?? "").trim();
  if (!trimmed) {
    return { text: "", modelId: modelId ?? DEFAULT_MODEL_ID };
  }

  const response = await client.send(
    new ConverseCommand({
      modelId: modelId ?? DEFAULT_MODEL_ID,
      messages: [{ role: "user", content: [{ text: trimmed }] }],
      system: [{ text: SUPERVISOR_SYSTEM_PROMPT }],
    }),
  );

  const text = response.output?.message?.content?.[0]?.text ?? "";
  return { text, modelId: modelId ?? DEFAULT_MODEL_ID };
}

/**
 * WO-2 T4 — the full T2 -> T3 -> T4 orchestration for one utterance.
 *
 * Drains the given AudioSession, runs it through Transcribe (emitting
 * partials via `onPartial`), and if a non-empty final transcript results,
 * routes it to Bedrock Converse. Returns a `TurnComplete`-shaped result
 * whether or not the AWS calls are real or mocked — the caller (run.mjs)
 * is responsible for broadcasting it and for real timestamp logging.
 *
 * All AWS SDK classes are injected so this function has no hard
 * dependency on live credentials to be tested. The real caller in run.mjs
 * passes the real `TranscribeStreamingClient` / `StartStreamTranscriptionCommand`
 * / `BedrockRuntimeClient` / `ConverseCommand` from the AWS SDK, constructed
 * with a real vault-provided credential set.
 */
export async function runTurn({
  session,
  onPartial,
  transcribeUtterance,
  TranscribeStreamingClient,
  StartStreamTranscriptionCommand,
  transcribeClientConfig,
  bedrockClient,
  ConverseCommand,
  modelId,
  sampleRateHz = 16_000,
}) {
  const t0 = Date.now();
  const samples = session.drain();
  const tRingWrite = Date.now() - t0; // "ring-write" timestamp per tasks.md's naming, though Pipeline 1 has no real ring — this marks when the buffered audio was handed off.

  const { text: transcript } = await transcribeUtterance({
    samples,
    sampleRateHz,
    onPartial,
    TranscribeStreamingClient,
    StartStreamTranscriptionCommand,
    clientConfig: transcribeClientConfig,
  });
  const tTranscribeFinal = Date.now() - t0;

  if (!transcript.trim()) {
    return {
      turnComplete: false,
      transcript: "",
      reply: "",
      timestamps: { tRingWrite, tTranscribeFinal, tTurnComplete: null },
    };
  }

  const { text: reply, modelId: usedModel } = await routeToConverse({
    client: bedrockClient,
    ConverseCommand,
    transcript,
    modelId,
  });
  const tTurnComplete = Date.now() - t0;

  return {
    turnComplete: true,
    transcript,
    reply,
    modelId: usedModel,
    timestamps: { tRingWrite, tTranscribeFinal, tTurnComplete },
  };
}
