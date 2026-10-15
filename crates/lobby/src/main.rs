//! Runs the lobby. `-addr :8096` says where it listens, and `-otel-endpoint host:port`
//! where its traces are sent; without one it sends none. Both are read as the
//! cluster's other apps read theirs.

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let flags = homelab::flags::Flags::new()
        .string("addr", ":8096", "listen address")
        .string(
            "otel-endpoint",
            "",
            "host:port of an OTLP/gRPC trace collector; tracing is disabled if empty",
        )
        .parse();

    homelab::logging::init();

    let traces = match homelab::trace::init("blockracers", flags.get("otel-endpoint")) {
        Ok(t) => t,
        Err(err) => {
            tracing::error!(error = format!("{err:#}"), "init tracing");
            std::process::exit(1);
        }
    };
    let served = homelab::serve::run(flags.get("addr"), lobby::app()).await;
    // Once the server has stopped, so what is left to send is the last of it.
    traces.shutdown().await;
    if let Err(err) = served {
        tracing::error!(error = format!("{err:#}"), "serve");
        std::process::exit(1);
    }
}
