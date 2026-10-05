//! Runs the lobby. `-addr :8096` says where it listens, and `-otel-endpoint host:port`
//! where its traces are sent; without one it sends none.

use std::net::SocketAddr;

use anyhow::{Context, Result, bail};

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<()> {
    let mut addr = ":8096".to_string();
    let mut collector = String::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-addr" => addr = args.next().context("-addr takes an address")?,
            "-otel-endpoint" => {
                collector = args
                    .next()
                    .context("-otel-endpoint takes a host and port")?
            }
            other => bail!("unknown argument {other}"),
        }
    }
    // `:8096` is every address, as the cluster's other apps write it.
    let addr = if addr.starts_with(':') {
        format!("0.0.0.0{addr}")
    } else {
        addr
    };
    let addr: SocketAddr = addr
        .parse()
        .with_context(|| format!("{addr} is not an address such as :8096"))?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("listening on {addr}"))?;
    let traces = if collector.is_empty() {
        None
    } else {
        Some(lobby::tracing::init("blockracers", &collector)?)
    };
    println!("lobby listening on {addr}");
    let served = axum::serve(listener, lobby::app())
        .with_graceful_shutdown(stopped())
        .await
        .context("serving");
    if let Some(traces) = traces {
        // Off the runtime's own threads: sending what is left waits on them.
        let sent = tokio::task::spawn_blocking(move || traces.shutdown())
            .await
            .context("sending the last traces")?;
        if let Err(error) = sent {
            eprintln!("the last traces were not sent: {error}");
        }
    }
    served
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
