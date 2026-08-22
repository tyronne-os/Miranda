use miranda_transport::{DataChannelHub, ServerConfig, TelemetryHub, TransportServer};

#[tokio::main]
async fn main() {
    let data_hub = DataChannelHub::new();
    let tele_hub = TelemetryHub::new(0);
    let cfg = ServerConfig::default();
    let server = TransportServer::new(cfg, data_hub, tele_hub);
    server.run().await.unwrap();
}
