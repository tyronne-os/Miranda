//! WO-2 T7 — Nemotron-Flash turn-taking state machine.
//!
//! "Nemotron-Flash" is a role label (per this project's role-slot design,
//! see `pipeline-1-aws-native.md` and the WO-2 tasks.md clarifications) —
//! it is filled here by the NVIDIA NIM API, the same provider T4 already
//! proved live for Pipeline 1's cognitive core (`nvidiaenterprise` vault
//! key, `https://integrate.api.nvidia.com/v1/chat/completions`). It is not
//! Bedrock and not a separate product literally named "Nemotron-Flash."
//!
//! # State machine
//!
//! ```text
//! Idle ──SpeechStart──────────────────────────▶ Listening
//! Listening ──PartialTranscript────────────────▶ ProcessingPartial
//! Listening ──SpeechStart──────────────────────▶ Listening   (re-entry)
//! ProcessingPartial ──FinalTranscript───────────▶ ProcessingFinal  (NIM call starts)
//! ProcessingFinal ──NimResponse─────────────────▶ Idle             (TurnComplete)
//! ProcessingFinal ──SpeechStart─────────────────▶ Listening        (interruption)
//! ```
//!
//! The interruption path is the one genuinely tricky part: a new
//! `SpeechStart` arriving while a NIM call is in flight must not let that
//! call's eventual response reach the caller. This is implemented with a
//! generation counter (same pattern as `TranscribeSessionGuard` in
//! `ace-controller/transcribeBridge.mjs` — deliberately reused rather than
//! inventing a second cancellation mechanism): every call to
//! `handle_event` that starts a NIM call captures the current generation;
//! when that call resolves, its result is only honoured if the generation
//! is still current. `tokio::select!` races the NIM future against a
//! cancellation notification for genuinely early wakeup, but the
//! generation check is what actually decides correctness — `select!`
//! alone cannot prevent a future that's already 99% done from completing
//! and returning stale data before the cancellation branch is polled.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde::Deserialize;
use tokio::sync::Notify;

/// The four states from design.md, named to match the transitions above.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Listening,
    ProcessingPartial,
    ProcessingFinal,
}

/// Events the state machine reacts to. Deliberately not the same type as
/// "what's on `audio_bus`" — this is the supervisor's own event vocabulary,
/// translated from whatever upstream signal (VAD, ASR partial/final,
/// browser WebSocket message) produced it.
#[derive(Debug, Clone)]
pub enum Event {
    SpeechStart,
    PartialTranscript(String),
    FinalTranscript(String),
}

/// What the state machine did in response to one `Event` — the caller
/// broadcasts `TurnComplete`/dispatches to TTS based on this, not by
/// inspecting `State` directly, so the *decision* stays in one place.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// No externally-visible effect (e.g. Listening -> Listening re-entry).
    None,
    /// A partial transcript was accepted; a caller may want to surface a
    /// "processing" UI cue but must not dispatch to TTS yet.
    PartialAccepted,
    /// The final transcript is being routed to the cognitive core.
    /// `generation` is the token this specific call is bound to — a
    /// caller driving `handle_event` in a loop does not need this, but a
    /// test or a caller wiring in real cancellation does.
    RoutingStarted { generation: u64 },
    /// The turn is complete: NIM responded, and this is a live (not
    /// superseded) result. This is the one variant that should trigger a
    /// `TurnComplete` broadcast and dispatch to TTS/motion.
    TurnComplete { transcript: String, reply: String },
    /// A NIM call resolved but was superseded by a newer `SpeechStart`
    /// before it finished — correctly discarded, not an error.
    Superseded,
    /// The cognitive-core call itself failed (network error, non-2xx,
    /// malformed response). Distinct from `Superseded`: this is a real
    /// failure the caller should probably surface, not silently drop.
    RoutingFailed(String),
}

/// Pure state-transition core, with no I/O. Every method that isn't
/// `handle_speech_start`/`handle_partial`/`handle_final` is a plain
/// synchronous state check — the async NIM call itself lives in
/// `NemotronFlashClient` below, kept deliberately separate so the state
/// machine's correctness can be tested without any network access at all.
pub struct TurnMachine {
    state: State,
    /// Bumped on every `SpeechStart`. A NIM call captures this value when
    /// it starts; if the value has moved by the time the call resolves,
    /// that call's result is stale and must be discarded.
    generation: Arc<AtomicU64>,
    /// Signalled on interruption so an in-flight NIM call's `select!` can
    /// wake up promptly instead of only being caught by the generation
    /// check after the network call already completed on its own.
    cancel: Arc<Notify>,
}

impl TurnMachine {
    pub fn new() -> Self {
        Self {
            state: State::Idle,
            generation: Arc::new(AtomicU64::new(0)),
            cancel: Arc::new(Notify::new()),
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    /// Current generation — the value a NIM call must be checked against
    /// before its result is honoured.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// True iff `gen` is still the current generation — i.e. no newer
    /// `SpeechStart` has superseded whatever call captured `gen`.
    pub fn is_current(&self, gen: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == gen
    }

    /// A clonable handle to this machine's cancellation signal, for a
    /// caller that wants to race a real async NIM call against
    /// interruption via `tokio::select!` (see `route_turn` below for the
    /// reference implementation of that pattern).
    pub fn cancel_signal(&self) -> Arc<Notify> {
        Arc::clone(&self.cancel)
    }

    /// Synchronous transition logic. All three real events funnel through
    /// here; the `Event` enum exists so callers don't need three
    /// differently-named methods, but transitions are still fully
    /// explicit per state, not a generic table, so an invalid
    /// state/event pairing is a compile-visible `match` arm rather than a
    /// silently-ignored lookup miss.
    pub fn handle_event(&mut self, event: Event) -> Outcome {
        match (self.state, event) {
            (State::Idle, Event::SpeechStart) => {
                self.state = State::Listening;
                Outcome::None
            }
            (State::Listening, Event::SpeechStart) => {
                // Re-entry while already listening: extend the utterance
                // window. Per design.md — this is not an interruption,
                // there is nothing in flight to cancel yet.
                Outcome::None
            }
            (State::Listening, Event::PartialTranscript(_)) => {
                self.state = State::ProcessingPartial;
                Outcome::PartialAccepted
            }
            (State::ProcessingPartial, Event::PartialTranscript(_)) => {
                // Additional partials while still mid-utterance: stay in
                // ProcessingPartial, no separate outcome needed each time.
                Outcome::PartialAccepted
            }
            (State::ProcessingPartial, Event::FinalTranscript(_)) => {
                self.state = State::ProcessingFinal;
                let gen = self.generation.load(Ordering::SeqCst);
                Outcome::RoutingStarted { generation: gen }
            }
            (State::ProcessingFinal, Event::SpeechStart) => {
                // Interruption: bump the generation so the in-flight NIM
                // call's eventual result is recognized as stale, wake
                // anything selecting on the cancel signal, and return to
                // Listening for the new utterance.
                self.generation.fetch_add(1, Ordering::SeqCst);
                self.cancel.notify_waiters();
                self.state = State::Listening;
                Outcome::None
            }
            // Any other (state, event) pairing is a no-op by design
            // rather than a panic: e.g. a stray PartialTranscript while
            // Idle (no SpeechStart seen yet) is dropped, not treated as a
            // protocol violation — REQ's "silence -> no dispatch" case.
            (_, _) => Outcome::None,
        }
    }

    /// Called once a NIM call this machine started actually resolves.
    /// Separated from `handle_event` because this is where the
    /// generation check happens — `handle_event` only ever *starts* a
    /// call (synchronously, instantly), it never awaits one.
    pub fn resolve_final(&mut self, generation: u64, result: Result<(String, String), String>) -> Outcome {
        if !self.is_current(generation) {
            return Outcome::Superseded;
        }
        match result {
            Ok((transcript, reply)) => {
                self.state = State::Idle;
                Outcome::TurnComplete { transcript, reply }
            }
            Err(e) => {
                self.state = State::Idle;
                Outcome::RoutingFailed(e)
            }
        }
    }
}

impl Default for TurnMachine {
    fn default() -> Self {
        Self::new()
    }
}

/// NVIDIA NIM chat completion response shape — only the fields this
/// module reads. `#[serde(default)]` fields tolerate a shorter response
/// than expected rather than failing deserialization outright, since a
/// malformed-but-parseable response should surface as "empty reply", not
/// as an opaque JSON error.
#[derive(Debug, Deserialize)]
struct NimResponse {
    #[serde(default)]
    choices: Vec<NimChoice>,
}

#[derive(Debug, Deserialize)]
struct NimChoice {
    message: NimMessage,
}

#[derive(Debug, Deserialize)]
struct NimMessage {
    #[serde(default)]
    content: String,
}

/// Thin client for the NVIDIA NIM chat completions endpoint — the same
/// endpoint and auth pattern T4's `bedrockRouter.mjs`-replacement
/// (`nvidiaChat`/`nvidiaChatMessages` in `run.mjs`) already proved live.
/// This is the Rust-side equivalent for Pipeline 2's native supervisor.
pub struct NemotronFlashClient {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl NemotronFlashClient {
    /// `api_key` should come from the AMANDA vault (`nvidiaenterprise` or
    /// `nvidia` provider), never hardcoded — this constructor does not
    /// fetch it itself, mirroring `bedrockRouter.mjs`'s design of taking
    /// credentials as a parameter rather than reaching into a secrets
    /// store internally, which keeps this module testable without vault
    /// access.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: api_key.into(),
            base_url: "https://integrate.api.nvidia.com/v1".to_string(),
            model: "nvidia/nemotron-mini-4b-instruct".to_string(),
        }
    }

    /// Overrides the base URL — exists solely so a test can point this at
    /// a local mock server instead of the real NIM endpoint.
    #[cfg(test)]
    fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// Sends one transcript as a single-turn chat completion and returns
    /// the reply text. No conversation memory is kept here — WO-2's scope
    /// is turn-taking, not persona/memory management (that already exists
    /// separately in `ace-controller`'s `eveChat` for Pipeline 1).
    pub async fn route(&self, transcript: &str) -> Result<String, String> {
        let body = serde_json::json!({
            "model": self.model,
            "messages": [{ "role": "user", "content": transcript }],
            "max_tokens": 200,
        });

        let res = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("NIM request failed: {e}"))?;

        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(format!("NIM returned HTTP {status}: {}", text.chars().take(200).collect::<String>()));
        }

        let parsed: NimResponse = res
            .json()
            .await
            .map_err(|e| format!("NIM response was not valid JSON: {e}"))?;

        Ok(parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default())
    }
}

/// Reference implementation of the cancellable routing call the module
/// doc comment describes: races the NIM call against the machine's
/// cancellation signal so an interruption is noticed as early as
/// possible, then double-checks the generation before trusting the
/// result — because `select!` only guarantees prompt *wakeup* on
/// cancellation, not that the NIM future didn't already finish and
/// produce a value on the same poll.
pub async fn route_turn(
    machine: &TurnMachine,
    client: &NemotronFlashClient,
    generation: u64,
    transcript: String,
) -> Result<(String, String), String> {
    let cancel = machine.cancel_signal();
    let outcome = tokio::select! {
        result = client.route(&transcript) => {
            result.map(|reply| (transcript, reply))
        }
        _ = cancel.notified() => {
            Err("interrupted".to_string())
        }
    };
    // The generation check the doc comment promises: `select!` resolves
    // whichever branch is ready first, but if the NIM call and the
    // cancellation notification become ready on the same poll, `select!`
    // is free to pick either — it is not guaranteed to prefer
    // cancellation. Without this check, that race could let a just-barely
    // stale NIM response through as `Ok(..)` instead of being caught as
    // `Superseded` by the caller. Re-checking here, after `select!` has
    // already resolved, closes that window: a generation mismatch is
    // reported the same way an explicit interruption is, so the caller
    // (which calls `resolve_final` on this result) treats both
    // uniformly.
    if !machine.is_current(generation) {
        return Err("interrupted".to_string());
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `test_normal_turn`: partial -> final -> TurnComplete emitted.
    /// Exact name and behavior from the WO-2 task spec.
    #[test]
    fn test_normal_turn() {
        let mut m = TurnMachine::new();
        assert_eq!(m.state(), State::Idle);

        assert_eq!(m.handle_event(Event::SpeechStart), Outcome::None);
        assert_eq!(m.state(), State::Listening);

        assert_eq!(
            m.handle_event(Event::PartialTranscript("hel".into())),
            Outcome::PartialAccepted
        );
        assert_eq!(m.state(), State::ProcessingPartial);

        let outcome = m.handle_event(Event::FinalTranscript("hello eve".into()));
        let gen = match outcome {
            Outcome::RoutingStarted { generation } => generation,
            other => panic!("expected RoutingStarted, got {other:?}"),
        };
        assert_eq!(m.state(), State::ProcessingFinal);

        // Simulate the NIM call resolving normally (mocked — no network).
        let final_outcome = m.resolve_final(gen, Ok(("hello eve".into(), "Hi there!".into())));
        assert_eq!(
            final_outcome,
            Outcome::TurnComplete { transcript: "hello eve".into(), reply: "Hi there!".into() }
        );
        assert_eq!(m.state(), State::Idle, "must return to Idle after TurnComplete");
    }

    /// `test_interruption`: start turn -> SpeechStart interrupts mid-NIM
    /// call -> the FIRST call's eventual result must be discarded
    /// (Superseded), not emitted as a second TurnComplete.
    #[test]
    fn test_interruption() {
        let mut m = TurnMachine::new();
        m.handle_event(Event::SpeechStart);
        m.handle_event(Event::PartialTranscript("hel".into()));
        let outcome = m.handle_event(Event::FinalTranscript("hello".into()));
        let first_gen = match outcome {
            Outcome::RoutingStarted { generation } => generation,
            other => panic!("expected RoutingStarted, got {other:?}"),
        };
        assert_eq!(m.state(), State::ProcessingFinal);

        // A new SpeechStart arrives while the first NIM call is still
        // (hypothetically) in flight — this is the interruption.
        assert_eq!(m.handle_event(Event::SpeechStart), Outcome::None);
        assert_eq!(m.state(), State::Listening, "interruption returns to Listening");

        // The stale first call finally resolves. It must be discarded.
        let stale_result = m.resolve_final(first_gen, Ok(("hello".into(), "stale reply".into())));
        assert_eq!(stale_result, Outcome::Superseded);
        assert_eq!(
            m.state(),
            State::Listening,
            "a superseded result must not move the state machine at all"
        );

        // Continue the new (second) turn to completion, proving the
        // machine is still fully functional after an interruption.
        let outcome2 = m.handle_event(Event::PartialTranscript("goodbye".into()));
        assert_eq!(outcome2, Outcome::PartialAccepted);
        let outcome3 = m.handle_event(Event::FinalTranscript("goodbye eve".into()));
        let second_gen = match outcome3 {
            Outcome::RoutingStarted { generation } => generation,
            other => panic!("expected RoutingStarted, got {other:?}"),
        };
        assert_ne!(second_gen, first_gen, "the second turn must have a fresh generation");

        let final_outcome = m.resolve_final(second_gen, Ok(("goodbye eve".into(), "Goodbye!".into())));
        assert_eq!(
            final_outcome,
            Outcome::TurnComplete { transcript: "goodbye eve".into(), reply: "Goodbye!".into() }
        );
    }

    /// `test_silence_no_dispatch`: silence frames -> no TurnComplete.
    /// Modeled as: no events at all fire while Idle, and a stray
    /// PartialTranscript/FinalTranscript arriving without a prior
    /// SpeechStart (silence — nothing to transcribe) must not dispatch
    /// anything either.
    #[test]
    fn test_silence_no_dispatch() {
        let mut m = TurnMachine::new();

        // No events at all: state never leaves Idle.
        assert_eq!(m.state(), State::Idle);

        // A stray final transcript with no prior SpeechStart/partial (the
        // "silence" case — nothing was actually being listened to) must
        // be a no-op, not a dispatch.
        let outcome = m.handle_event(Event::FinalTranscript("phantom".into()));
        assert_eq!(outcome, Outcome::None);
        assert_eq!(m.state(), State::Idle, "must not have moved out of Idle");

        // A stray partial with no prior SpeechStart: same — no-op.
        let outcome2 = m.handle_event(Event::PartialTranscript("phantom".into()));
        assert_eq!(outcome2, Outcome::None);
        assert_eq!(m.state(), State::Idle);
    }

    /// A real NIM-call failure (network error, bad status, etc.) must
    /// surface as `RoutingFailed`, not silently become an empty
    /// `TurnComplete` — a caller needs to be able to tell "the model said
    /// nothing" apart from "the call itself failed."
    #[test]
    fn resolve_final_surfaces_real_errors() {
        let mut m = TurnMachine::new();
        m.handle_event(Event::SpeechStart);
        m.handle_event(Event::PartialTranscript("hi".into()));
        let outcome = m.handle_event(Event::FinalTranscript("hi eve".into()));
        let gen = match outcome {
            Outcome::RoutingStarted { generation } => generation,
            other => panic!("expected RoutingStarted, got {other:?}"),
        };

        let result = m.resolve_final(gen, Err("NIM returned HTTP 500".into()));
        assert_eq!(result, Outcome::RoutingFailed("NIM returned HTTP 500".into()));
        assert_eq!(m.state(), State::Idle, "must still return to Idle after a failure");
    }

    /// Real, live async test: the `route_turn` cancellation race against
    /// a genuinely slow mock NIM server, using a real tokio runtime and a
    /// real (localhost) HTTP server — not a bare unit-level mock of
    /// `NemotronFlashClient::route`, so the actual `select!`/`Notify`
    /// wiring is exercised, not just asserted correct by inspection.
    #[tokio::test]
    async fn interruption_cancels_in_flight_nim_call_promptly() {
        use std::time::Duration;

        // A local TCP listener that never responds — simulates a NIM call
        // that would take far longer than the test should wait, so a
        // passing test proves the cancellation path actually won the
        // race rather than the call just happening to finish fast.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind local test listener");
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            // Accept and hold the connection open for the whole test
            // without writing a response, so the client genuinely hangs
            // waiting on headers rather than getting a fast connection
            // reset. The stream must stay alive (not be dropped) for this
            // to work — closing it, even without responding, still
            // unblocks the client with an error rather than a hang, which
            // was the actual bug in an earlier version of this test.
            if let Ok((stream, _)) = listener.accept().await {
                tokio::time::sleep(Duration::from_secs(5)).await;
                std::mem::drop(stream);
            }
        });

        let m = TurnMachine::new();
        let client = NemotronFlashClient::new("test-key").with_base_url(format!("http://{addr}"));

        let cancel = m.cancel_signal();
        let route_future = route_turn(&m, &client, m.generation(), "hello".into());

        tokio::pin!(route_future);

        // Fire the cancellation shortly after starting the call — long
        // enough that the request has genuinely been sent, short enough
        // that a real NIM round trip would not have completed yet.
        let cancel_at = tokio::time::sleep(Duration::from_millis(50));

        tokio::select! {
            result = &mut route_future => {
                panic!("NIM call should not have completed before cancellation: {result:?}");
            }
            _ = cancel_at => {
                cancel.notify_waiters();
            }
        }

        // Now the already-in-progress route_future must resolve via the
        // cancellation branch, not hang forever.
        let outcome = tokio::time::timeout(Duration::from_secs(2), route_future)
            .await
            .expect("route_turn must resolve promptly after cancellation, not hang");
        assert_eq!(outcome, Err("interrupted".to_string()));
    }
}
