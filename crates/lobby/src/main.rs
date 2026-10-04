//! Runs the lobby. `-addr :8096` says where it listens.

use std::net::SocketAddr;

use anyhow::{Context, Result, bail};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let mut addr = ":8096".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-addr" => addr = args.next().context("-addr takes an address")?,
            other => bail!("unknown argument {other}"),
        }
    }
    // `:8096` is every address, as the cluster's other apps write it.
    let addr = if addr.starts_with(':') { format!("0.0.0.0{addr}") } else { addr };
    let addr: SocketAddr = addr.parse().with_context(|| format!("{addr} is not an address such as :8096"))?;
    let listener = tokio::net::TcpListener::bind(addr).await.with_context(|| format!("listening on {addr}"))?;
    println!("lobby listening on {addr}");
    axum::serve(listener, lobby::app()).with_graceful_shutdown(stopped()).await.context("serving")
}

/// Done when the cluster, or a Ctrl-C, asks the server to stop.
async fn stopped() {
    use tokio::signal::unix::{SignalKind, signal};
    let Ok(mut term) = signal(SignalKind::terminate()) else {
        // Without the handler the server still runs; it is only stopped less gently.
        return std::future::pending().await;
    };
    tokio::select! {
        _ = term.recv() => {}
        _ = tokio::signal::ctrl_c() => {}
    }
}
