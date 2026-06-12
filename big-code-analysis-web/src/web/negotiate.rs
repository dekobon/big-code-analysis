//! Accept-header content negotiation for the structured endpoints (#657).
//!
//! Every analysis endpoint that returns a structured value (`/v1/ast`,
//! `/v1/comment` JSON, `/v1/function`, `/v1/metrics`, and the three
//! `/v1/vcs*` routes) computes one `Serialize` response and then renders
//! it in the format the client asked for via `Accept`. JSON remains the
//! default; YAML and CBOR reuse the *same* serializers the CLI drives for
//! `-O yaml|cbor`, so a value serialized over the wire is byte-identical
//! to the CLI output for the same input.
//!
//! Before #657 the `Accept` header was silently ignored: a client sending
//! `Accept: application/yaml` got a `200` with `Content-Type:
//! application/json` and no signal its preference was dropped. Now an
//! unsupported concrete media type answers `406 Not Acceptable` through
//! the same uniform `{error, error_kind, id}` envelope every other error
//! uses, listing the supported types.

use actix_web::http::header::{ACCEPT, HeaderValue};

/// The structured serialization a client requested via `Accept`.
///
/// TOML and CSV are deliberately excluded: TOML is awkward for the deeply
/// nested arrays-of-tables the space tree produces, and CSV is flat /
/// tabular and only maps onto the vcs ranking. The three variants here are
/// the tree-shaped serializers that round-trip the recursive `spaces`
/// output without loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Format {
    /// `application/json` — the default when `Accept` is absent or `*/*`.
    Json,
    /// `application/yaml` (also `text/yaml`, `application/x-yaml`).
    Yaml,
    /// `application/cbor`.
    Cbor,
}

impl Format {
    /// Canonical `Content-Type` header value echoed for this format.
    ///
    /// JSON matches the JSON extractor's emitted content type exactly so
    /// the negotiated JSON path is indistinguishable from the pre-#657
    /// `.json(...)` path for existing clients.
    pub(super) fn content_type(self) -> &'static str {
        match self {
            Format::Json => "application/json",
            Format::Yaml => "application/yaml",
            Format::Cbor => "application/cbor",
        }
    }

    /// Serializes `value` into this format, returning the encoded bytes.
    ///
    /// JSON and YAML are text serializers; CBOR is binary. A serialization
    /// failure is surfaced as `Err(())` so the caller renders a uniform
    /// `500` rather than a partially-written body — no `unwrap`/`expect`
    /// on the response path.
    pub(super) fn encode<T: serde::Serialize>(self, value: &T) -> Result<Vec<u8>, ()> {
        match self {
            Format::Json => serde_json::to_vec(value).map_err(|_| ()),
            Format::Yaml => serde_yaml::to_string(value)
                .map(String::into_bytes)
                .map_err(|_| ()),
            Format::Cbor => {
                let mut buf = Vec::new();
                ciborium::into_writer(value, &mut buf)
                    .map(|()| buf)
                    .map_err(|_| ())
            }
        }
    }
}

/// Picks the response [`Format`] from one `Accept` header token's essence.
///
/// `essence` is a lowercased `type/subtype` string already stripped of
/// parameters (the `q=` weight is handled by [`negotiate`]). Returns
/// `None` for a concrete type this server does not serialize, so the
/// caller can keep scanning the remaining `Accept` entries before deciding
/// on a `406`. `*/*` and `application/*` resolve to JSON — a wildcard means
/// the client expressed no concrete preference, so the default applies.
fn format_for_essence(essence: &str) -> Option<Format> {
    match essence {
        "application/json" | "*/*" | "application/*" => Some(Format::Json),
        // Accept the registered `application/yaml` (RFC 9512) plus the two
        // historical aliases clients and the CLI ecosystem still emit.
        "application/yaml" | "text/yaml" | "application/x-yaml" => Some(Format::Yaml),
        "application/cbor" => Some(Format::Cbor),
        _ => None,
    }
}

/// Resolves the response [`Format`] for a request's `Accept` header value.
///
/// `None` (no `Accept`, or a header that names only unsupported types)
/// signals a `406` to the caller. An absent or empty header, or any header
/// that includes a wildcard or `application/json`, yields
/// [`Format::Json`]. Among the supported entries the one with the highest
/// `q` weight wins; ties keep the first-listed entry, so an explicit
/// `application/yaml` is honored over an equally-weighted later type.
pub(super) fn negotiate(accept: Option<&HeaderValue>) -> Option<Format> {
    // No `Accept` header at all is the unconstrained case: default to JSON,
    // exactly as an API client that sends no preference expects.
    let Some(raw) = accept.and_then(|value| value.to_str().ok()) else {
        return Some(Format::Json);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Some(Format::Json);
    }

    // Track the best supported match by q-weight, preserving list order on
    // ties (the first-listed entry of equal weight wins). `q` is parsed
    // best-effort: a malformed weight falls back to 1.0, matching the
    // lenient posture of every major HTTP server.
    let mut best: Option<(Format, f32)> = None;
    let mut saw_unsupported = false;
    for entry in trimmed.split(',') {
        let mut parts = entry.split(';');
        let Some(media) = parts.next() else { continue };
        let essence = media.trim().to_ascii_lowercase();
        if essence.is_empty() {
            continue;
        }
        let Some(format) = format_for_essence(&essence) else {
            saw_unsupported = true;
            continue;
        };
        let quality = parts
            .find_map(|param| {
                let param = param.trim();
                param
                    .strip_prefix("q=")
                    .or_else(|| param.strip_prefix("Q="))
                    .and_then(|q| q.parse::<f32>().ok())
            })
            .unwrap_or(1.0);
        // `q=0` explicitly refuses a type, so it can never be selected.
        if quality > 0.0 && best.is_none_or(|(_, best_q)| quality > best_q) {
            best = Some((format, quality));
        }
    }

    match best {
        Some((format, _)) => Some(format),
        // Every entry named a concrete unsupported type (or refused
        // everything with `q=0`): a 406 is the honest answer.
        None if saw_unsupported => None,
        // No supported and no unsupported entry seen (e.g. an all-`q=0`
        // wildcard) — fall back to the JSON default rather than 406.
        None => Some(Format::Json),
    }
}

/// Reads and negotiates the `Accept` header of `req`.
///
/// Thin convenience wrapper over [`negotiate`] so handlers do not reach
/// into `req.headers()` themselves.
pub(super) fn from_request(req: &actix_web::HttpRequest) -> Option<Format> {
    negotiate(req.headers().get(ACCEPT))
}

/// Human-readable list of the media types content negotiation honors,
/// used in the `406` body so a client sees exactly what to send.
pub(super) const SUPPORTED_MEDIA_TYPES: &str =
    "application/json, application/yaml, application/cbor";

#[cfg(test)]
mod tests {
    use super::*;

    fn accept(value: &str) -> Option<HeaderValue> {
        HeaderValue::from_str(value).ok()
    }

    #[test]
    fn absent_accept_defaults_to_json() {
        assert_eq!(negotiate(None), Some(Format::Json));
    }

    #[test]
    fn empty_and_wildcard_default_to_json() {
        assert_eq!(negotiate(accept("").as_ref()), Some(Format::Json));
        assert_eq!(negotiate(accept("*/*").as_ref()), Some(Format::Json));
        assert_eq!(
            negotiate(accept("application/*").as_ref()),
            Some(Format::Json)
        );
    }

    #[test]
    fn concrete_supported_types_resolve() {
        assert_eq!(
            negotiate(accept("application/json").as_ref()),
            Some(Format::Json)
        );
        assert_eq!(
            negotiate(accept("application/yaml").as_ref()),
            Some(Format::Yaml)
        );
        assert_eq!(
            negotiate(accept("application/cbor").as_ref()),
            Some(Format::Cbor)
        );
    }

    #[test]
    fn yaml_aliases_resolve() {
        assert_eq!(negotiate(accept("text/yaml").as_ref()), Some(Format::Yaml));
        assert_eq!(
            negotiate(accept("application/x-yaml").as_ref()),
            Some(Format::Yaml)
        );
    }

    #[test]
    fn parameters_and_case_are_ignored() {
        assert_eq!(
            negotiate(accept("Application/YAML; charset=utf-8").as_ref()),
            Some(Format::Yaml)
        );
    }

    #[test]
    fn unsupported_concrete_type_is_406() {
        assert_eq!(negotiate(accept("application/xml").as_ref()), None);
        assert_eq!(negotiate(accept("text/html").as_ref()), None);
    }

    #[test]
    fn q_weights_select_the_highest() {
        // YAML outranks JSON despite being listed second.
        assert_eq!(
            negotiate(accept("application/json;q=0.5, application/yaml;q=0.9").as_ref()),
            Some(Format::Yaml)
        );
    }

    #[test]
    fn q_zero_refuses_a_type() {
        // JSON is refused; the only positive-weight supported entry is CBOR.
        assert_eq!(
            negotiate(accept("application/json;q=0, application/cbor").as_ref()),
            Some(Format::Cbor)
        );
    }

    #[test]
    fn mixed_supported_and_unsupported_prefers_supported() {
        assert_eq!(
            negotiate(accept("text/html, application/yaml").as_ref()),
            Some(Format::Yaml)
        );
    }

    #[test]
    fn first_listed_wins_on_equal_weight() {
        assert_eq!(
            negotiate(accept("application/yaml, application/cbor").as_ref()),
            Some(Format::Yaml)
        );
        assert_eq!(
            negotiate(accept("application/cbor, application/yaml").as_ref()),
            Some(Format::Cbor)
        );
    }

    #[test]
    fn encode_roundtrips_each_format() {
        #[derive(serde::Serialize)]
        struct Sample {
            n: u32,
            s: &'static str,
        }
        let value = Sample { n: 7, s: "hi" };

        let json = Format::Json.encode(&value).unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&json).unwrap();
        assert_eq!(parsed["n"], serde_json::json!(7));

        let yaml = Format::Yaml.encode(&value).unwrap();
        let parsed: serde_yaml::Value = serde_yaml::from_slice(&yaml).unwrap();
        assert_eq!(parsed["n"], serde_yaml::Value::from(7));

        let cbor = Format::Cbor.encode(&value).unwrap();
        let parsed: ciborium::Value = ciborium::from_reader(cbor.as_slice()).unwrap();
        assert!(parsed.is_map());
    }
}
