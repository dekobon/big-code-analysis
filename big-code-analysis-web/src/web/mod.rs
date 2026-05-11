/// `POST /comment` — comment-stripping endpoint.
pub mod comment;
/// `POST /function` — function-extraction endpoint.
pub mod function;
/// `POST /metrics` — metric-computation endpoint.
pub mod metrics;
/// HTTP server bootstrapping and shared state.
pub mod server;
