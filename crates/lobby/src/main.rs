//! Runs the lobby. `--addr 0.0.0.0:8096` says where it listens, and
//! `--otel-endpoint host:port` where its traces are sent; without one it sends
//! none.

use std::net::SocketAddr;

use clap::Parser;

/// The lobby a racing game lists its online sessions on.
#[derive(Parser)]
struct Args {
    /// Address to listen on.
    #[arg(long, default_value = "0.0.0.0:8096")]
    addr: SocketAddr,
    /// host:port of an OTLP/gRPC trace collector; none are sent when empty.
    #[arg(long, default_value = "")]
    otel_endpoint: String,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let traces = homelab::init("blockracers", &args.otel_endpoint)?;
    let served = homelab::serve::run(args.addr, lobby::app()).await;
    // Once the server has stopped, so what is left to send is the last of it.
    traces.shutdown().await;
    served
}
