mod postgres_store;
mod protocol;
mod server;
mod state;

use anyhow::Result;
use clap::Parser;
use std::net::SocketAddr;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(
    name = "blackhole-mailbox",
    about = "Blackhole rendezvous (mailbox) server",
    version
)]
struct Args {
    /// Address to bind the WebSocket listener on.
    #[arg(long, env = "BLACKHOLE_MAILBOX_LISTEN", default_value = "0.0.0.0:4000")]
    listen: SocketAddr,

    /// Postgres connection URL. If unset, an in-memory store is used (state is lost on restart).
    #[arg(long, env = "DATABASE_URL")]
    database_url: Option<String>,
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

    let store: state::DynStore = match args.database_url.as_deref() {
        Some(url) => {
            info!("using Postgres store");
            postgres_store::PostgresStore::connect(url).await?
        },
        None => {
            info!("using in-memory store (state is not persistent)");
            state::InMemoryStore::new()
        },
    };

    server::run(args.listen, store).await
}
