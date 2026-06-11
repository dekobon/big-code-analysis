// bca: suppress-file(halstead)
// Cohesive bag of small flag value-types + their parsers; the
// file-level halstead.effort is a many-small-items aggregation
// artifact (Halstead is non-linear over a shared vocabulary), not
// per-function logic complexity — every function here clears the gate.

//! Value types for `bca check`'s threshold-tier and CI-presentation
//! flags (issues #385/#666/#683/#688). Split out of `lib.rs` so the
//! parsing / resolution logic for `--tier`, `--exit-codes`,
//! `--github-annotations`, and `--summary-file` lives in one cohesive
//! module instead of inflating the top-level crate root.

use std::path::PathBuf;
use std::str::FromStr;

use clap::ValueEnum;

/// Which threshold tier `bca check` gates against (issue #375).
///
/// `Hard` (the default) uses the `[thresholds]` table verbatim and
/// ignores any `[thresholds.soft]` overrides. `Soft` is the
/// early-warning tier: it merges `[thresholds.soft]` on top of
/// `[thresholds]` (per-metric soft limits, absolute or `"<ratio>x"`
/// scale-relative), and — when no soft table is configured — falls
/// back to scaling every limit by the soft tier's ratio (the `RATIO`
/// in `--tier=soft=RATIO`, default 0.95). See the resolution order on
/// [`CheckArgs::tier`](crate::CheckArgs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum Tier {
    #[default]
    Hard,
    Soft,
}

impl Tier {
    /// Lowercase wire name, used by `--print-effective-config`.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Hard => "hard",
            Self::Soft => "soft",
        }
    }
}

/// The `--tier` argument value (issue #688). Carries the threshold-tier
/// strictness *and*, for the soft tier, the early-warning scale ratio it
/// applies. Folding the old standalone `--headroom` flag into the tier
/// retires the four-knob precedence model: the soft tier IS its ratio,
/// and the hard tier has none.
///
/// Parsed from `--tier <hard|soft|soft=RATIO>`:
/// - `hard` (the default) — gate against `[thresholds]` verbatim.
/// - `soft` — early-warning tier at the default ratio (0.95).
/// - `soft=0.90` — early-warning tier scaling each limit by `0.90`.
///
/// `RATIO` must lie in `(0, 1]`; `soft=1.0` disables scaling (a soft
/// tier that still consults a `[thresholds.soft]` table but applies no
/// blanket multiplier).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) enum TierSpec {
    #[default]
    Hard,
    /// Soft tier. `None` ratio means "use the default 0.95 when no
    /// `[thresholds.soft]` table is configured"; `Some(r)` pins the
    /// blanket scale ratio.
    Soft(Option<f64>),
}

impl TierSpec {
    /// The coarse [`Tier`] (hard vs soft) this spec selects.
    pub(crate) fn tier(self) -> Tier {
        match self {
            Self::Hard => Tier::Hard,
            Self::Soft(_) => Tier::Soft,
        }
    }

    /// The blanket soft-tier scale ratio, if one was pinned. `None` for
    /// the hard tier and for a bare `soft` (which defers to the
    /// downstream default when no `[thresholds.soft]` table is present).
    pub(crate) fn ratio(self) -> Option<f64> {
        match self {
            Self::Hard => None,
            Self::Soft(r) => r,
        }
    }
}

/// Parse and range-check the `RATIO` in `--tier=soft=RATIO` to `(0, 1]`.
/// Split out of [`TierSpec::from_str`] so each carries one responsibility
/// (parse-vs-dispatch) and neither accumulates the other's exit points.
fn parse_soft_ratio(ratio_str: &str) -> Result<f64, String> {
    let ratio: f64 = ratio_str
        .parse()
        .map_err(|_| format!("soft ratio must be a number; got {ratio_str:?}"))?;
    if crate::thresholds::is_valid_scale_ratio(ratio) {
        Ok(ratio)
    } else {
        Err(format!("soft ratio must be in (0, 1]; got {ratio}"))
    }
}

impl FromStr for TierSpec {
    type Err = String;

    /// Parse `hard`, `soft`, or `soft=<RATIO>`. The ratio is validated to
    /// `(0, 1]` here so a bad `--tier=soft=2` is a usage error (exit 1,
    /// routed through `exit_clap_error`) with a per-flag message, matching
    /// how `--headroom` used to be range-checked downstream.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || format!("expected `hard`, `soft`, or `soft=<ratio>`; got {s:?}");
        match s {
            "hard" => Ok(Self::Hard),
            "soft" => Ok(Self::Soft(None)),
            _ => {
                let ratio_str = s.strip_prefix("soft=").ok_or_else(invalid)?;
                let ratio = parse_soft_ratio(ratio_str)?;
                Ok(Self::Soft(Some(ratio)))
            }
        }
    }
}

/// Exit-code style for `bca check` (issue #385/#666). `Default` keeps
/// the stable 0/1/2 contract; `Tiered` opts into the 2-5 severity split.
/// The value-taking `--exit-codes <default|tiered>` flag and the
/// `[check] exit_codes` manifest key share this vocabulary; the CLI
/// value overrides the manifest in either direction.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[clap(rename_all = "lower")]
pub(crate) enum ExitCodes {
    #[default]
    Default,
    Tiered,
}

/// Tri-state for a CI behaviour that auto-detects a GitHub Actions
/// environment variable (issue #683). Mirrors
/// [`ColorWhen`](crate::ColorWhen): `Auto` detects the env signal,
/// `Always` forces the behaviour on, `Never` suppresses it even inside a
/// workflow step.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[clap(rename_all = "lower")]
pub(crate) enum CiDetect {
    /// Enable when the GitHub Actions env signal is present.
    #[default]
    Auto,
    /// Always enable, even outside a GHA step.
    Always,
    /// Never enable, even inside a GHA step.
    Never,
}

impl CiDetect {
    /// Resolve to a yes/no decision given whether the auto-detect env
    /// signal is present. The `env_signal` argument is injected so the
    /// precedence (`always` > `never` > env autodetect) is unit-testable
    /// without mutating the process environment.
    pub(crate) fn enabled_with(self, env_signal: bool) -> bool {
        match self {
            CiDetect::Always => true,
            CiDetect::Never => false,
            CiDetect::Auto => env_signal,
        }
    }
}

/// Value of `--summary-file` (issue #683). A file path appends the
/// markdown digest there unconditionally; the keyword `auto` (the
/// default when the flag is omitted) defers to `$GITHUB_STEP_SUMMARY`;
/// `never` suppresses the digest even inside a GHA step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SummaryFile {
    /// Append to `$GITHUB_STEP_SUMMARY` when that env var is set.
    Auto,
    /// Suppress the digest unconditionally.
    Never,
    /// Append to this explicit path.
    Path(PathBuf),
}

impl FromStr for SummaryFile {
    type Err = std::convert::Infallible;

    /// Parse the keywords `auto` / `never`, treating anything else as a
    /// literal path. A path literally named `auto` or `never` can be
    /// disambiguated with a `./` prefix (`./never`), mirroring the
    /// `--check-exclude-from -` convention.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "auto" => Self::Auto,
            "never" => Self::Never,
            _ => Self::Path(PathBuf::from(s)),
        })
    }
}
