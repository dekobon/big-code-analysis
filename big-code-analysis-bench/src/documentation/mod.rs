//! Shared source-documentation registry, operations and class-specific gate.

mod attachment_validation;
pub mod combined;
mod fixtures;
mod operation;
pub mod runner;
#[cfg(test)]
mod tests;
mod validation;
#[cfg(all(test, unix))]
mod workflow_tests;

pub use fixtures::{REGISTRY, SourceProbe, StatisticClass};
pub use operation::{Parsed, Prepared, SourceCounts};

/// Benchmark setup, operation or measurement failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessError(pub String);

impl std::fmt::Display for HarnessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for HarnessError {}

pub(super) fn failure(error: impl std::fmt::Debug) -> HarnessError {
    HarnessError(format!("{error:?}"))
}

pub(super) fn require(condition: bool, message: impl Into<String>) -> Result<(), HarnessError> {
    if condition {
        Ok(())
    } else {
        Err(HarnessError(message.into()))
    }
}
