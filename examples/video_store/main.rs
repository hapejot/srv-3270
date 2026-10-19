//! TN3270-Server fuer den video-store OData-Service.
//!
//! Start: `cargo run --example video_store`
//!
//! Umgebungsvariablen:
//!   VIDEO3270_BIND  Bind-Adresse (Standard 0.0.0.0:3270)
//!   ODATA_URL       OData-Basis (Standard http://nuc:3035/Service)
//!   ODATA_USER      Wert fuer den Header X-Forwarded-User (Standard 3270)
//!   RUST_LOG        Log-Filter (Standard info); z.B. RUST_LOG=debug

mod odata;
mod views;

use anyhow::Result;
use srv_3270::Server;
use tracing_subscriber::EnvFilter;

use views::{DetailView, ListView};

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let bind = env_or("VIDEO3270_BIND", "0.0.0.0:3270");
    let odata = odata::Odata::new(
        env_or("ODATA_URL", "http://nuc:3035/Service"),
        env_or("ODATA_USER", "3270"),
    )?;
    Server::new(odata, "list")
        .view("list", ListView::default)
        .view("detail", || DetailView)
        .serve(bind)
        .await
}
