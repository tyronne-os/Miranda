//! Axum HTTP + WebSocket server wiring.
//!
//! One [`axum::Router`] serves both endpoints:
//!
//! - `GET /data` — upgrades to a binary WebSocket; the hub sends 312-byte
//!   frame packets at 60 FPS.
//! - `GET /telemetry` — upgrades to a text WebSocket; the hub sends JSON
//!   snapshots at ~2 Hz.
//! - `GET /health` — plain JSON health check; never requires upgrade.
//!
//! # Usage
//!
//! ```rust,no_run
//! use std::net::SocketAddr;
//! use std::sync::Arc;
//! use miranda_transport::{TransportServer, ServerConfig, DataChannelHub, TelemetryHub};
//! use miranda_transport::telemetry::DispatchSource;
//!
//! #[tokio::main]
//! async fn main() {
//!     let data_hub = DataChannelHub::new();
//!     let tele_hub = TelemetryHub::new(0);
//!     let cfg = ServerConfig::default();
//!     let server = TransportServer::new(cfg, data_hub, tele_hub);
//!     server.run().await.unwrap();
//! }
//! ```

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::{HeaderValue, Method},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde_json::json;
use tokio::time;
use tower_http::cors::CorsLayer;

use crate::frame::PACKET_SIZE;
use crate::hub::DataChannelHub;
use crate::telemetry::TelemetryHub;
/// Transport server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Address to bind. Defaults to `127.0.0.1:9090`.
    pub bind: SocketAddr,
    /// Telemetry snapshot interval. Defaults to 500 ms.
    pub telemetry_interval: Duration,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:9090".parse().unwrap(),
            telemetry_interval: Duration::from_millis(500),
        }
    }
}

/// Shared state injected into every axum handler.
struct AppState {
    data_hub: DataChannelHub,
    tele_hub: TelemetryHub,
    telemetry_interval: Duration,
}

/// The combined transport server.
pub struct TransportServer {
    cfg: ServerConfig,
    data_hub: DataChannelHub,
    tele_hub: TelemetryHub,
}

impl TransportServer {
    pub fn new(cfg: ServerConfig, data_hub: DataChannelHub, tele_hub: TelemetryHub) -> Self {
        Self { cfg, data_hub, tele_hub }
    }

    /// Binds the server and runs it until the process exits.
    ///
    /// This is async but never returns in normal operation. Use
    /// [`tokio::spawn`] or `tokio::select!` to run it alongside other tasks.
    pub async fn run(self) -> std::io::Result<()> {
        let state = Arc::new(AppState {
            data_hub: self.data_hub,
            tele_hub: self.tele_hub,
            telemetry_interval: self.cfg.telemetry_interval,
        });

        let app = Router::new()
            .route("/health", get(health_handler))
            .route("/data", get(data_ws_handler))
            .route("/telemetry", get(telemetry_ws_handler))
            .layer(build_cors_layer())
            .with_state(state);

        let listener = tokio::net::TcpListener::bind(self.cfg.bind).await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::AddrInUse, e))?;

        axum::serve(listener, app).await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }
}

/// Builds the CORS policy for the plain `GET /health` endpoint.
///
/// The WebSocket endpoints (`/data`, `/telemetry`) do NOT need this layer —
/// the WebSocket handshake is not subject to the same-origin/CORS
/// restriction that plain `fetch()` requests are, which is why the browser
/// client's WebSocket connections worked before this layer existed while
/// its `fetch("/health")` call failed with a CORS error.
///
/// Explicit origin allowlist, matching the pattern already used by
/// `client-services/ace-controller/run.mjs` for the same reason: a
/// wildcard (`Access-Control-Allow-Origin: *`) would also work for a
/// GET-only, unauthenticated health check, but naming the exact dev-server
/// origins keeps the policy legible and makes it obvious this is a local
/// development harness, not a public API — the moment a real deployment
/// origin needs to be added, it has to be added by name here, not silently
/// covered by a wildcard already in place.
fn build_cors_layer() -> CorsLayer {
    let allowed_origins = [
        "http://127.0.0.1:5173",
        "http://localhost:5173",
        "http://127.0.0.1:4173",
        "http://localhost:4173",
    ]
    .into_iter()
    .map(|o| o.parse::<HeaderValue>().expect("static origin is valid"))
    .collect::<Vec<_>>();

    CorsLayer::new()
        .allow_origin(allowed_origins)
        .allow_methods([Method::GET])
}

// ── Handlers ────────────────────────────────────────────────────────────────

async fn health_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(json!({
        "ok": true,
        "dataSubscribers": state.data_hub.subscriber_count(),
        "telemetrySubscribers": state.tele_hub.subscriber_count(),
        "framesBroadcast": state.data_hub.frames_broadcast(),
        "circuitBreaker": format!("{:?}", state.tele_hub.circuit_breaker()),
    }))
}

async fn data_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| data_ws_task(socket, state.data_hub.clone()))
}

async fn data_ws_task(mut socket: WebSocket, hub: DataChannelHub) {
    let (mut rx, _dropped) = hub.subscribe();

    loop {
        match rx.recv().await {
            Some(pkt) => {
                debug_assert_eq!(pkt.len(), PACKET_SIZE);
                if socket.send(Message::Binary(pkt.to_vec().into())).await.is_err() {
                    break;
                }
            }
            None => break, // hub shut down
        }
    }
}

async fn telemetry_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| {
        telemetry_ws_task(socket, state.tele_hub.clone(), state.telemetry_interval)
    })
}

async fn telemetry_ws_task(
    mut socket: WebSocket,
    hub: TelemetryHub,
    interval: Duration,
) {
    let mut rx = hub.subscribe();
    let mut tick = time::interval(interval);
    tick.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = tick.tick() => {
                // Drain any stale snapshots from the channel; only send the
                // latest one. This keeps a slow browser from receiving a
                // burst of old snapshots when it catches up.
                let mut latest = None;
                while let Ok(snap) = rx.try_recv() {
                    latest = Some(snap);
                }
                if let Some(json) = latest {
                    if socket.send(Message::Text(json.into())).await.is_err() {
                        break;
                    }
                }
            }
            // Also forward anything the hub pushed between ticks (e.g. an
            // alert) without waiting for the next scheduled tick.
            Some(json) = rx.recv() => {
                if socket.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::time::Duration;

    use bytes::BytesMut;
    use miranda_core::{BlendshapeFrame, KinematicTransformFrame, BLENDSHAPE_COUNT, KINEMATIC_JOINT_COUNT, Quaternion};

    fn blend(ts: u64) -> BlendshapeFrame {
        BlendshapeFrame { timestamp_us: ts, weights: [0.0; BLENDSHAPE_COUNT] }
    }
    fn kin(ts: u64) -> KinematicTransformFrame {
        KinematicTransformFrame {
            timestamp_us: ts,
            joints: [Quaternion::IDENTITY; KINEMATIC_JOINT_COUNT],
            head_pitch_deg: 0.0,
            clavicle_rise: 0.0,
            _reserved: [0; 8],
        }
    }

    #[test]
    fn default_config_binds_to_loopback() {
        let cfg = ServerConfig::default();
        assert_eq!(cfg.bind.ip().to_string(), "127.0.0.1");
        assert_eq!(cfg.bind.port(), 9090);
        assert_eq!(cfg.telemetry_interval, Duration::from_millis(500));
    }

    /// Verify the server starts, the health endpoint is reachable, and a
    /// data-plane subscriber receives a frame. Uses an OS-assigned port so
    /// the test does not conflict with other tests or running instances.
    #[tokio::test]
    async fn server_health_endpoint_responds_ok() {
        let data_hub = DataChannelHub::new();
        let tele_hub = TelemetryHub::new(0);
        let cfg = ServerConfig {
            bind: "127.0.0.1:0".parse().unwrap(), // OS-assigned port
            ..Default::default()
        };

        let listener = tokio::net::TcpListener::bind(cfg.bind).await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let state = Arc::new(AppState {
            data_hub: data_hub.clone(),
            tele_hub: tele_hub.clone(),
            telemetry_interval: cfg.telemetry_interval,
        });
        let app = Router::new()
            .route("/health", get(health_handler))
            .route("/data", get(data_ws_handler))
            .route("/telemetry", get(telemetry_ws_handler))
            .layer(build_cors_layer())
            .with_state(state);

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        // Give the server a moment to start.
        tokio::time::sleep(Duration::from_millis(50)).await;

        let url = format!("http://127.0.0.1:{port}/health");
        let resp = reqwest::get(&url).await.expect("health request failed");
        assert!(resp.status().is_success());
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["ok"], true);
    }

    /// The specific defect this layer exists to fix: a browser page served
    /// from one origin (Vite, port 5173) calling `fetch()` against this
    /// server's different origin (port 9090) must succeed, not fail with a
    /// CORS error. `reqwest` (used above) does not enforce CORS — only real
    /// browsers do — so this test sends the `Origin` header a browser would
    /// send and asserts the server answers with a matching
    /// `Access-Control-Allow-Origin`, which is the actual signal a browser
    /// checks before it allows the response through to JS.
    #[tokio::test]
    async fn health_endpoint_allows_the_vite_dev_origin() {
        let data_hub = DataChannelHub::new();
        let tele_hub = TelemetryHub::new(0);
        let cfg = ServerConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            ..Default::default()
        };

        let listener = tokio::net::TcpListener::bind(cfg.bind).await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let state = Arc::new(AppState {
            data_hub: data_hub.clone(),
            tele_hub: tele_hub.clone(),
            telemetry_interval: cfg.telemetry_interval,
        });
        let app = Router::new()
            .route("/health", get(health_handler))
            .layer(build_cors_layer())
            .with_state(state);

        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        tokio::time::sleep(Duration::from_millis(50)).await;

        let client = reqwest::Client::new();
        let resp = client
            .get(format!("http://127.0.0.1:{port}/health"))
            .header("Origin", "http://127.0.0.1:5173")
            .send()
            .await
            .expect("request failed");

        assert!(resp.status().is_success());
        let allow_origin = resp
            .headers()
            .get("access-control-allow-origin")
            .expect("missing Access-Control-Allow-Origin header — a browser would reject this response");
        assert_eq!(allow_origin, "http://127.0.0.1:5173");
    }

    /// An origin NOT on the allowlist must not be echoed back — proves this
    /// is a real allowlist, not an accidental wildcard-everything policy.
    #[tokio::test]
    async fn health_endpoint_does_not_allow_an_unlisted_origin() {
        let data_hub = DataChannelHub::new();
        let tele_hub = TelemetryHub::new(0);
        let cfg = ServerConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            ..Default::default()
        };

        let listener = tokio::net::TcpListener::bind(cfg.bind).await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let state = Arc::new(AppState {
            data_hub: data_hub.clone(),
            tele_hub: tele_hub.clone(),
            telemetry_interval: cfg.telemetry_interval,
        });
        let app = Router::new()
            .route("/health", get(health_handler))
            .layer(build_cors_layer())
            .with_state(state);

        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        tokio::time::sleep(Duration::from_millis(50)).await;

        let client = reqwest::Client::new();
        let resp = client
            .get(format!("http://127.0.0.1:{port}/health"))
            .header("Origin", "http://evil.example")
            .send()
            .await
            .expect("request failed");

        assert!(
            resp.headers().get("access-control-allow-origin").is_none(),
            "an unlisted origin must not receive an Access-Control-Allow-Origin header"
        );
    }

    /// A data WebSocket subscriber receives correctly sized binary frames.
    #[tokio::test]
    async fn data_websocket_delivers_binary_frames() {
        use futures_util::StreamExt;
        let data_hub = DataChannelHub::new();
        let tele_hub = TelemetryHub::new(0);
        let cfg = ServerConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            ..Default::default()
        };

        let listener = tokio::net::TcpListener::bind(cfg.bind).await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let state = Arc::new(AppState {
            data_hub: data_hub.clone(),
            tele_hub: tele_hub.clone(),
            telemetry_interval: cfg.telemetry_interval,
        });
        let app = Router::new()
            .route("/data", get(data_ws_handler))
            .with_state(state);

        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Connect a WebSocket client.
        use tokio_tungstenite::tungstenite;
        let (mut ws, _) = tokio_tungstenite::connect_async(
            format!("ws://127.0.0.1:{port}/data")
        ).await.expect("ws connect failed");

        // Allow time for the hub to register the subscriber.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Broadcast a frame from the hub side.
        let mut scratch = BytesMut::with_capacity(PACKET_SIZE);
        data_hub.broadcast(&blend(42), &kin(42), &mut scratch);

        // Wait for the WebSocket to deliver it.
        let msg = tokio::time::timeout(Duration::from_millis(500), ws.next()).await
            .expect("timeout waiting for ws message")
            .expect("ws stream ended")
            .expect("ws error");

        match msg {
            tungstenite::Message::Binary(data) => {
                assert_eq!(data.len(), PACKET_SIZE,
                    "expected {PACKET_SIZE} bytes, got {}", data.len());
                assert_eq!(&data[..4], b"MRD1", "missing MRD1 magic");
            }
            other => panic!("expected binary frame, got {other:?}"),
        }
    }

    /// Telemetry WebSocket delivers JSON with the expected shape.
    #[tokio::test]
    async fn telemetry_websocket_delivers_json_snapshot() {
        use crate::telemetry::DispatchSource;
        use futures_util::StreamExt;
        struct MockSrc;
        impl DispatchSource for MockSrc {
            fn frames_published(&self) -> u64 { 120 }
            fn frames_dropped(&self) -> u64 { 0 }
            fn late_frames(&self) -> u64 { 0 }
            fn publish_failures(&self) -> u64 { 0 }
            fn mean_build_us(&self) -> f64 { 10.0 }
            fn max_build_us(&self) -> f64 { 100.0 }
            fn audio_chunks_consumed(&self) -> u64 { 60 }
        }

        let data_hub = DataChannelHub::new();
        let tele_hub = TelemetryHub::new(0);
        let cfg = ServerConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            telemetry_interval: Duration::from_millis(50), // fast for testing
        };

        let listener = tokio::net::TcpListener::bind(cfg.bind).await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let tele_for_push = tele_hub.clone();
        let data_for_state = data_hub.clone();
        let state = Arc::new(AppState {
            data_hub: data_for_state,
            tele_hub: tele_hub.clone(),
            telemetry_interval: cfg.telemetry_interval,
        });
        let app = Router::new()
            .route("/telemetry", get(telemetry_ws_handler))
            .with_state(state);

        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        tokio::time::sleep(Duration::from_millis(50)).await;

        use tokio_tungstenite::tungstenite;
        let (mut ws, _) = tokio_tungstenite::connect_async(
            format!("ws://127.0.0.1:{port}/telemetry")
        ).await.expect("ws connect failed");

        // Push a snapshot — the ws task will drain it on the next tick.
        tokio::time::sleep(Duration::from_millis(20)).await;
        tele_for_push.publish_snapshot(1_000_000, &MockSrc, 0, 1, 0, 0);

        let msg = tokio::time::timeout(Duration::from_millis(500), ws.next()).await
            .expect("timeout")
            .expect("stream ended")
            .expect("ws error");

        match msg {
            tungstenite::Message::Text(json) => {
                let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
                assert_eq!(parsed["framesPublished"], 120);
                assert_eq!(parsed["circuitBreaker"], "closed");
            }
            other => panic!("expected text frame, got {other:?}"),
        }
    }
}
