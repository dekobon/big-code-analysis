/// `POST /comment` — comment-stripping endpoint.
pub mod comment;
/// Opt-in CORS policy and middleware (`--cors`, #694).
pub mod cors;
/// `POST /function` — function-extraction endpoint.
pub mod function;
/// `POST /metrics` — metric-computation endpoint.
pub mod metrics;
/// `Accept`-header content negotiation (JSON / YAML / CBOR), #657.
mod negotiate;
/// HTTP server bootstrapping and shared state.
pub mod server;
/// `POST /vcs` — change-history (VCS) metrics endpoint.
pub mod vcs;
