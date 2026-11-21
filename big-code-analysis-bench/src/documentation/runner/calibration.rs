use std::fmt;
use std::time::Duration;

use super::{
    HarnessError, MIN_SOURCE_SAMPLE, Mode, SOURCE_MAX_ITERATIONS, SOURCE_TARGET_SAMPLE,
    check_budget, context, require,
};

/// One tested repetition count, with observed durations in execution order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalibrationStage {
    /// Power-of-two repetitions actually requested from each batch.
    pub iterations: u32,
    /// Three samples on completion; only the observed prefix on failure.
    pub samples: Vec<Duration>,
}

/// Decision at the end of the retained calibration prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalibrationOutcome {
    /// Calibration has not yet produced a terminal decision.
    Pending,
    /// The first stage whose median is at least two milliseconds.
    TargetReached,
    /// The cap median resolves at 200 microseconds but misses the target.
    CapLimited,
    /// The cap median remains below 200 microseconds.
    Unresolved,
    /// The next batch failed without yielding an elapsed duration.
    OperationFailed,
    /// The last retained batch exceeded the per-operation budget.
    BudgetExceeded,
}

impl CalibrationOutcome {
    fn label(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::TargetReached => "target-reached",
            Self::CapLimited => "cap-limited",
            Self::Unresolved => "unresolved",
            Self::OperationFailed => "operation-failed",
            Self::BudgetExceeded => "budget-exceeded",
        }
    }
}

/// Smoke preparation or the bounded premeasurement batch ladder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Calibration {
    /// One semantic setup execution, without a calibration claim.
    Smoke {
        /// Actual elapsed setup time, absent if the operation failed.
        elapsed: Option<Duration>,
    },
    /// At most 21 tested counts and 63 actual elapsed samples.
    Batches {
        /// Counts and ordered samples, including any failing-stage prefix.
        stages: Vec<CalibrationStage>,
        /// Terminal decision, or pending during preparation.
        outcome: CalibrationOutcome,
    },
}

impl Calibration {
    pub(super) fn new(mode: Mode) -> Self {
        if mode == Mode::Smoke {
            Self::Smoke { elapsed: None }
        } else {
            Self::Batches {
                stages: Vec::new(),
                outcome: CalibrationOutcome::Pending,
            }
        }
    }

    pub(super) fn validate(
        &self,
        mode: Mode,
        iterations: u32,
        budget: Duration,
    ) -> Result<(), HarnessError> {
        match self {
            Self::Smoke { elapsed } => {
                require(
                    mode == Mode::Smoke && iterations == 1,
                    "invalid smoke preparation",
                )?;
                let elapsed =
                    elapsed.ok_or_else(|| HarnessError("missing smoke preparation".into()))?;
                check_budget("smoke preparation", 0, elapsed, 1, budget)
            }
            Self::Batches { stages, outcome } => {
                require(mode != Mode::Smoke, "smoke must not claim calibration")?;
                validate_stages(stages, *outcome, iterations, budget)
            }
        }
    }
}

fn median(samples: &[Duration]) -> Result<Duration, HarnessError> {
    let mut times: [Duration; 3] = samples
        .try_into()
        .map_err(|_| HarnessError("calibration stage requires three samples".into()))?;
    times.sort_unstable();
    Ok(times[1])
}

fn validate_stages(
    stages: &[CalibrationStage],
    outcome: CalibrationOutcome,
    iterations: u32,
    budget: Duration,
) -> Result<(), HarnessError> {
    require(
        !stages.is_empty() && stages.len() <= 21,
        "invalid calibration stage count",
    )?;
    for (at, stage) in stages.iter().enumerate() {
        require(
            stage.iterations == 1 << at && stage.iterations <= SOURCE_MAX_ITERATIONS,
            "invalid calibration count ladder",
        )?;
        let observed = median(&stage.samples)?;
        for &elapsed in &stage.samples {
            check_budget("calibration", at, elapsed, stage.iterations, budget)?;
        }
        if at + 1 < stages.len() {
            require(
                observed < SOURCE_TARGET_SAMPLE,
                "calibration continued after qualifying stage",
            )?;
        } else {
            require(
                iterations == stage.iterations,
                "calibration and scheduled counts differ",
            )?;
            let valid = match outcome {
                CalibrationOutcome::TargetReached => observed >= SOURCE_TARGET_SAMPLE,
                CalibrationOutcome::CapLimited => {
                    stage.iterations == SOURCE_MAX_ITERATIONS
                        && (MIN_SOURCE_SAMPLE..SOURCE_TARGET_SAMPLE).contains(&observed)
                }
                _ => false,
            };
            require(valid, "invalid calibration outcome")?;
        }
    }
    Ok(())
}

pub(super) fn calibrate(
    evidence: &mut Calibration,
    id: &str,
    size: usize,
    budget: Duration,
    mut measure: impl FnMut(u32) -> Result<Duration, HarnessError>,
) -> Result<u32, HarnessError> {
    let Calibration::Batches { stages, outcome } = evidence else {
        let elapsed = measure(1).map_err(|error| {
            context(
                id,
                size,
                HarnessError(format!("smoke preparation count 1 sample 0: {error}")),
            )
        })?;
        *evidence = Calibration::Smoke {
            elapsed: Some(elapsed),
        };
        check_budget(id, size, elapsed, 1, budget)?;
        return Ok(1);
    };
    require(
        stages.is_empty() && *outcome == CalibrationOutcome::Pending,
        "calibration already started",
    )?;
    let mut count = 1;
    loop {
        let at = stages.len();
        stages.push(CalibrationStage {
            iterations: count,
            samples: Vec::with_capacity(3),
        });
        for sample in 0..3 {
            let elapsed = measure(count).map_err(|error| {
                *outcome = CalibrationOutcome::OperationFailed;
                HarnessError(format!("{id} size {size}: calibration stage {at} count {count} sample {sample}: {error}"))
            })?;
            stages[at].samples.push(elapsed);
            check_budget(id, size, elapsed, count, budget).map_err(|error| {
                *outcome = CalibrationOutcome::BudgetExceeded;
                HarnessError(format!(
                    "calibration stage {at} count {count} sample {sample}: {error}"
                ))
            })?;
        }
        let observed = median(&stages[at].samples)?;
        if observed >= SOURCE_TARGET_SAMPLE {
            *outcome = CalibrationOutcome::TargetReached;
            return Ok(count);
        }
        if count == SOURCE_MAX_ITERATIONS {
            if observed >= MIN_SOURCE_SAMPLE {
                *outcome = CalibrationOutcome::CapLimited;
                return Ok(count);
            }
            *outcome = CalibrationOutcome::Unresolved;
            return Err(HarnessError(format!(
                "CalibrationUnresolved: {id} size {size}, stage {at} count {count} sample 2, median-ns {} < {}",
                observed.as_nanos(),
                MIN_SOURCE_SAMPLE.as_nanos()
            )));
        }
        count *= 2;
    }
}

pub(super) fn write_calibration(
    f: &mut impl fmt::Write,
    evidence: &Calibration,
    budget: Duration,
) -> fmt::Result {
    writeln!(f, "    per-operation-budget-ns: {}", budget.as_nanos())?;
    match evidence {
        Calibration::Smoke { elapsed } => {
            write!(f, "    smoke-preparation-ns:")?;
            if let Some(elapsed) = elapsed {
                write!(f, " {}", elapsed.as_nanos())?;
            }
            writeln!(f, "; no calibration or performance claim")
        }
        Calibration::Batches { stages, outcome } => {
            for (at, stage) in stages.iter().enumerate() {
                write!(f, "    calibration-stage-ns: {at} {}", stage.iterations)?;
                for sample in &stage.samples {
                    write!(f, " {}", sample.as_nanos())?;
                }
                writeln!(f)?;
            }
            if let Some(stage) = stages.last() {
                write!(
                    f,
                    "    calibration-final-ns: {} {}",
                    stages.len() - 1,
                    stage.iterations
                )?;
                for sample in &stage.samples {
                    write!(f, " {}", sample.as_nanos())?;
                }
                writeln!(f, " {}", outcome.label())?;
            }
            Ok(())
        }
    }
}
