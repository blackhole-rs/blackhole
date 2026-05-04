mod protocol;
mod server;
mod state;

use anyhow::Result;
use clap::Parser;
use std::net::SocketAddr;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "blackhole-mailbox", about = "Blackhole rendezvous (mailbox) server", version)]
struct Args {
    /// Address to bind the WebSocket listener on.
    #[arg(long, env = "BLACKHOLE_MAILBOX_LISTEN", default_value = "0.0.0.0:4000")]
    listen: SocketAddr,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_target(false)
        .init();

    let args = Args::parse();
    let state = state::Shared::new();
    server::run(args.listen, state).await
}
