//! Traces of the requests the lobby answers, sent to a collector over OTLP, as the
//! cluster's other apps send theirs: one span a request, batched, over gRPC without
//! TLS since the collector is inside the cluster.

use anyhow::{Context, Result};
use axum::extract::{MatchedPath, Request};
use axum::middleware::Next;
use axum::response::Response;
use opentelemetry::trace::{Span, SpanKind, Status, Tracer};
use opentelemetry::{KeyValue, global};
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::SdkTracerProvider;

/// Starts sending spans to the collector at `endpoint` (a host and port). The
/// provider it returns is to be shut down when the server stops, which sends what is
/// still waiting.
pub fn init(service: &'static str, endpoint: &str) -> Result<SdkTracerProvider> {
    let exporter = SpanExporter::builder().with_tonic().with_endpoint(format!("http://{endpoint}")).build().context("making the trace exporter")?;
    let provider = SdkTracerProvider::builder().with_batch_exporter(exporter).with_resource(Resource::builder().with_service_name(service).build()).build();
    global::set_tracer_provider(provider.clone());
    Ok(provider)
}

/// A span for each request, named for the route it matched. With no collector set
/// the spans go nowhere. The cluster's own probing and scraping is left out: it
/// comes several times a minute for ever and says nothing.
pub async fn trace(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();
    if path == "/healthz" || path == "/metrics" {
        return next.run(request).await;
    }
    let method = request.method().to_string();
    let route = request.extensions().get::<MatchedPath>().map_or("unmatched", MatchedPath::as_str).to_string();
    let tracer = global::tracer("lobby");
    let mut span = tracer
        .span_builder(format!("{method} {route}"))
        .with_kind(SpanKind::Server)
        .with_attributes([KeyValue::new("http.request.method", method), KeyValue::new("http.route", route), KeyValue::new("url.path", path)])
        .start(&tracer);
    let response = next.run(request).await;
    let status = response.status();
    span.set_attribute(KeyValue::new("http.response.status_code", status.as_u16() as i64));
    if status.is_server_error() {
        span.set_status(Status::error(status.to_string()));
    }
    span.end();
    response
}
