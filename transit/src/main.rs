mod server;

use anyhow::Result;
use clap::Parser;
use std::{net::SocketAddr, time::Duration};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(
    name = "blackhole-transit",
    about = "Blackhole transit relay server",
    version
)]
struct Args {
    /// Address to bind the TCP listener on.
    #[arg(long, env = "BLACKHOLE_TRANSIT_LISTEN", default_value = "0.0.0.0:4001")]
    listen: SocketAddr,

    /// Seconds to hold an unpaired connection waiting for its peer before dropping it.
    #[arg(long, env = "BLACKHOLE_TRANSIT_WAIT_SECS", default_value_t = 60)]
    wait_secs: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let args = Args::parse();
    server::run(args.listen, Duration::from_secs(args.wait_secs)).await
}
